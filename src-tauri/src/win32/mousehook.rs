//! 低级鼠标钩子 —— 鼠标手势的输入源。
//!
//! 三条纪律（都在 keyhook.rs 里踩过，这里不能破）：
//!   1. 钩子必须跑在有消息循环的线程里；
//!   2. 回调有超时限制，超时会被系统**静默摘钩** —— 所以回调里只做计算，
//!      画轨迹/执行动作全部异步，绝不能同步等别的线程；
//!   3. 吞事件返回 `LRESULT(1)`，放行 `CallNextHookEx`。
//!
//! 为什么"吞掉按下"而不是"只吞抬起"：
//!   只吞抬起时，按下已经送到目标程序了。有些程序在右键按下就进入"捕获鼠标"
//!   状态，抬起被我们吞掉后它一直等不到，表现就是**之后左右键都失灵** ——
//!   这正是用户报的"会导致无法进行鼠标的操作"。
//!   改成吞下按：目标程序一个事件都收不到，状态永远干净；
//!   如果用户只是点了一下（没划成手势），我们在抬起时**补发**一次右键点击。
//!
//! 全屏游戏/视频/远程桌面里自动让路：那些场景手势只会帮倒忙，
//! 而且吞掉右键真的会影响操作。
//!
//! 轨迹点在这里留一份（`Track::pts`）：方向串只能表达折线，用户真正想要的
//! "画一个 S" 得靠形状识别（`win32::shape`）。一旦认出来就锁定成 `"S"`，
//! 后面继续挪鼠标也不会再往方向串里塞字符 —— 曲线本来就会糊出一串 DLD。
//!
//! 状态机里所有加锁的地方都用**块作用域**：`parking_lot` 的 guard 不是重入的，
//! 读锁没放掉就写锁会直接死锁，而这个线程卡死 = 整个系统鼠标卡死。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, OnceLock};

use parking_lot::RwLock;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    MSLLHOOKSTRUCT, MSG, WH_MOUSE_LL, WM_LBUTTONDOWN, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE,
    WM_RBUTTONDOWN, WM_RBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use crate::config::GestureConfig;

/// 钩子吐出的事件。
#[derive(Debug, Clone)]
pub enum Event {
    /// 拖动距离越过阈值，正式进入手势模式
    Begin { x: i32, y: i32 },
    /// 手势过程中的每一次移动
    Move { x: i32, y: i32 },
    /// 方向串变了（用来实时更新屏幕上的动作名）
    Turn { pattern: String },
    /// 抬手。`pattern` 形如 "DR"；`stroke` 是这一笔的原始轨迹，
    /// 给"自定义手势"按形状匹配用（方向串表达不了 M、W、Z 这些）。
    End {
        pattern: String,
        stroke: Vec<(i32, i32)>,
    },
    /// 手势被放弃（动了但没划出方向、或者中途按了左键）
    Cancel,
}

type Sink = Box<dyn Fn(Event) + Send + Sync>;

static SINK: OnceLock<Sink> = OnceLock::new();
static CFG: OnceLock<RwLock<GestureConfig>> = OnceLock::new();
/// 安装线程是否已经起过（避免重复起线程）
static SPAWNED: AtomicBool = AtomicBool::new(false);
/// 钩子**确实装上了**。和 SPAWNED 分开：以前只要线程起了就报"已安装"，
/// 被安全软件拦掉时界面显示"手势已启用"，用户划半天毫无反应。
static INSTALLED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Track {
    /// 触发键是否按着
    down: bool,
    /// 是否已越过阈值（进入手势模式）
    gesture: bool,
    /// 这次按下有没有被我们吞掉（吞了才需要补发点击）
    swallowed: bool,
    /// 中途按了左键 → 放弃手势，且不补发右键
    canceled: bool,
    origin: (i32, i32),
    anchor: (i32, i32),
    last_dir: Option<char>,
    pattern: String,
    /// 这一笔的原始轨迹（形状识别用）
    pts: Vec<(i32, i32)>,
    /// 已认出的形状。认出来就锁定，不再走方向判定
    shape: Option<char>,
    /// 已认出的自定义手势（bindings 下标）
    custom: Option<usize>,
}

static TRACK: RwLock<Option<Track>> = RwLock::new(None);

/// 刷新配置。设置页改了规则之后调这个。
pub fn update_config(cfg: GestureConfig) {
    match CFG.get() {
        Some(lock) => *lock.write() = cfg,
        None => {
            let _ = CFG.set(RwLock::new(cfg));
        }
    }
    rebuild_hot();
}

pub fn install(sink: impl Fn(Event) + Send + Sync + 'static) {
    let _ = SINK.set(Box::new(sink));
    if SPAWNED.swap(true, Ordering::SeqCst) {
        return;
    }

    std::thread::spawn(|| unsafe {
        use windows::Win32::Foundation::HINSTANCE;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;

        let hinst = match GetModuleHandleW(None) {
            Ok(h) => HINSTANCE(h.0),
            Err(e) => {
                log::error!("获取模块句柄失败，鼠标手势不可用: {e}");
                SPAWNED.store(false, Ordering::SeqCst);
                return;
            }
        };
        match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(hinst), 0) {
            Ok(_) => {
                INSTALLED.store(true, Ordering::SeqCst);
                log::info!("鼠标手势钩子已安装");
            }
            Err(e) => {
                log::error!("鼠标钩子安装失败（可能被安全软件拦截）: {e}");
                SPAWNED.store(false, Ordering::SeqCst);
                return;
            }
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&mut msg);
        }
        // 消息循环退出 = 钩子没了，状态必须跟着说真话
        INSTALLED.store(false, Ordering::SeqCst);
    });
}

enum Trigger {
    Right,
    Middle,
    X1,
}

/// 钩子需要的全部配置，一次读出来，避免回调里反复加锁
struct Snapshot {
    trigger: Trigger,
    threshold: i32,
    step: i32,
    pause_fullscreen: bool,
    swallow_down: bool,
    excluded: Vec<String>,
    /// 自定义手势：(在 bindings 里的下标, 规范笔画)。
    /// 下标只用来给浮标显示"这一划会执行什么"，真正执行时按笔画重算。
    custom: Vec<(usize, Vec<[f32; 2]>)>,
}

fn snapshot() -> Option<Snapshot> {
    let lock = CFG.get()?;
    let cfg = lock.read();
    if !cfg.enabled {
        return None;
    }
    let trigger = match cfg.trigger.as_str() {
        "middle" => Trigger::Middle,
        "x1" => Trigger::X1,
        _ => Trigger::Right,
    };
    let custom = cfg
        .bindings
        .iter()
        .enumerate()
        .filter(|(_, b)| b.enabled && b.points.len() >= 8)
        .map(|(i, b)| (i, b.points.clone()))
        .collect();
    Some(Snapshot {
        trigger,
        threshold: cfg.threshold.clamp(8, 200),
        step: cfg.step.clamp(16, 300),
        pause_fullscreen: cfg.pause_fullscreen,
        swallow_down: cfg.swallow_down,
        excluded: cfg.excluded.clone(),
        custom,
    })
}

fn emit(ev: Event) {
    if let Some(sink) = SINK.get() {
        sink(ev);
    }
}

/// 预编译好的"热路径配置"。
///
/// 鼠标钩子每秒要处理上百个事件，而 `snapshot()` 每次都 `clone()` 出
/// `excluded` 和**全部自定义手势模板**（`Vec<Vec<[f32;2]>>`）——
/// 这是持续不断的堆分配，而低层钩子有严格超时，超时会被系统静默摘钩。
/// 所以配置在这里编译成不可变快照，钩子只做 `Arc::clone`（引用计数 +1），
/// 真正的深拷贝只在配置变更时发生一次。
static HOT: OnceLock<RwLock<Option<Arc<Snapshot>>>> = OnceLock::new();

fn hot() -> Option<Arc<Snapshot>> {
    HOT.get()?.read().clone()
}

fn rebuild_hot() {
    let snap = snapshot().map(Arc::new);
    match HOT.get() {
        Some(lock) => *lock.write() = snap,
        None => {
            let _ = HOT.set(RwLock::new(snap));
        }
    }
}

/// 这次按下要不要管？
///
/// 全屏（游戏/视频/演示/远程桌面）和用户排除的程序一律让路 ——
/// 在这些地方手势不是"便利"，而是"捣乱"。
fn should_handle(cfg: &Snapshot) -> bool {
    if cfg.pause_fullscreen && crate::win32::window::foreground_is_fullscreen() {
        return false;
    }
    if !cfg.excluded.is_empty() {
        let exe = crate::win32::window::foreground_exe_cached();
        if !exe.is_empty() && cfg.excluded.iter().any(|e| e.eq_ignore_ascii_case(&exe)) {
            return false;
        }
    }
    true
}

/// 触发键按下。返回 true = 吞掉这次按下。
fn on_down(x: i32, y: i32, swallow: bool) -> bool {
    *TRACK.write() = Some(Track {
        down: true,
        gesture: false,
        swallowed: swallow,
        canceled: false,
        origin: (x, y),
        anchor: (x, y),
        last_dir: None,
        pattern: String::new(),
        pts: vec![(x, y)],
        shape: None,
        custom: None,
    });
    swallow
}

fn on_move(x: i32, y: i32, threshold: i32, step: i32, custom: &[(usize, Vec<[f32; 2]>)]) {
    // 第一段：更新状态，顺带算出要不要发 Begin
    let began = {
        let mut guard = TRACK.write();
        let Some(t) = guard.as_mut() else { return };
        if !t.down {
            return;
        }
        if !t.gesture {
            let (dx, dy) = (x - t.origin.0, y - t.origin.1);
            if dx.abs().max(dy.abs()) < threshold {
                return;
            }
            t.gesture = true;
            t.anchor = (x, y);
            // 起手那 30px 也要进轨迹：S 的开始被砍掉一截，识别会明显变差
            if t.pts.last() != Some(&(x, y)) {
                t.pts.push((x, y));
            }
            Some(t.origin)
        } else {
            None
        }
    }; // 锁在这里放开，后面才能安全地回调上层

    if let Some((ox, oy)) = began {
        emit(Event::Begin { x: ox, y: oy });
        emit(Event::Move { x, y });
        return;
    }

    // 第二段：方向判定
    let turned = {
        let mut guard = TRACK.write();
        let Some(t) = guard.as_mut() else { return };
        if !t.gesture {
            return;
        }
        // ---- 轨迹累积 + 形状识别 ----
        // 每 4 个点试一次：既够及时（浮标能当场显示"截图"），又不至于
        // 每帧都算一遍把钩子拖慢（钩子超时会被系统静默摘钩）。
        if t
            .pts
            .last()
            .map_or(true, |l| (l.0 - x).abs().max((l.1 - y).abs()) >= 2)
            && t.pts.len() < crate::win32::shape::MAX_POINTS
        {
            t.pts.push((x, y));
        }
        let mut changed: Option<String> = None;
        // 自定义笔画优先：用户特意画的那个形状，比"方向串 / S"更具体
        if t.custom.is_none() && !custom.is_empty() && t.pts.len() >= 12 && t.pts.len() % 4 == 0
        {
            let path = crate::win32::shape::canonical_stroke(&t.pts);
            let mut best: Option<(f32, usize)> = None;
            for (idx, tpl) in custom {
                let d = crate::win32::shape::stroke_distance(&path, tpl);
                if best.map_or(true, |(bd, _)| d < bd) {
                    best = Some((d, *idx));
                }
            }
            if let Some((d, idx)) = best {
                if d <= crate::win32::shape::CUSTOM_MATCH {
                    t.custom = Some(idx);
                    t.pattern = format!("#{idx}");
                    changed = Some(t.pattern.clone());
                }
            }
        }
        if changed.is_none() && t.shape.is_none() && t.custom.is_none() && t.pts.len() >= 12 && t.pts.len() % 4 == 0 {
            if let Some(c) = crate::win32::shape::recognize(&t.pts) {
                t.shape = Some(c);
                // 曲线糊出来的方向串（DLD 之类）作废，直接以形状为准
                t.pattern.clear();
                t.pattern.push(c);
                changed = Some(t.pattern.clone());
            }
        }

        let (dx, dy) = (x - t.anchor.0, y - t.anchor.1);
        // 已经认出形状就不再记方向，否则浮标会在 "S 截图" 和 "未绑定" 之间乱跳
        if changed.is_none()
            && t.shape.is_none()
            && t.custom.is_none()
            && (dx.abs() >= step || dy.abs() >= step)
        {
            let d = if dx.abs() > dy.abs() {
                if dx > 0 {
                    'R'
                } else {
                    'L'
                }
            } else if dy > 0 {
                'D'
            } else {
                'U'
            };
            if Some(d) != t.last_dir {
                t.last_dir = Some(d);
                t.pattern.push(d);
                changed = Some(t.pattern.clone());
            }
            // 锚点跟着走：同一方向连续移动只记一次
            t.anchor = (x, y);
        }
        changed
    };

    emit(Event::Move { x, y });
    if let Some(pattern) = turned {
        emit(Event::Turn { pattern });
    }
}

/// 触发键抬起。返回 true = 吞掉这次抬起。
fn on_up() -> bool {
    let taken = {
        let mut guard = TRACK.write();
        guard.take()
    };
    let Some(t) = taken else { return false };
    if !t.down {
        return false;
    }
    // 没吞按下（swallowDown 关掉）：抬起要放行，右键菜单交给系统
    if !t.swallowed {
        if t.gesture && !t.pattern.is_empty() {
            emit(Event::End {
                pattern: t.pattern,
                stroke: t.pts,
            });
        }
        return false;
    }

    // 中途按了左键：用户的意图已经变了，什么都别做（也不补发右键）
    if t.canceled {
        emit(Event::Cancel);
        return true;
    }

    if t.gesture && !t.pattern.is_empty() {
        // 一笔结束时把轨迹指标记一行：形状手势最难查的就是"用户说没反应"，
        // 有了这几个数字（尺寸、弦长、模板距离）才知道是该调阈值还是该改算法。
        log::debug!(
            "轨迹 {} 形状={:?}",
            crate::win32::shape::diagnose(&t.pts),
            t.shape
        );
        emit(Event::End {
            pattern: t.pattern,
            stroke: t.pts,
        });
        return true;
    }

    // 没形成手势 = 用户只是点了一下右键。补发一次真正的右键点击，
    // 否则"右键菜单打不开"会变成新的 bug。
    if !t.gesture {
        // 补发：让系统认为这就是一次普通右键点击
        if let Err(e) = crate::win32::input::inject_right_click() {
            log::warn!("补发右键失败: {e}");
        }
        return true;
    }

    // 动了但没划出方向（手抖到刚过阈值）：不补发 ——
    // 在鼠标已经移开的位置弹出右键菜单反而更莫名其妙
    emit(Event::Cancel);
    true
}

/// 手势进行中按下左键 → 放弃这次手势
fn cancel_if_gesturing() {
    let mut guard = TRACK.write();
    if let Some(t) = guard.as_mut() {
        if t.down {
            t.canceled = true;
            t.gesture = false;
            t.pattern.clear();
        }
    }
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);

        // 自己补发的点击：立刻放行，绝不处理（否则无限递归）
        if ms.dwExtraInfo == crate::win32::input::INJECT_MAGIC {
            return CallNextHookEx(None, code, wparam, lparam);
        }

        let Some(cfg) = hot() else {
            // 配置关掉了：清掉残留状态（读锁先放，再取写锁）
            let has = TRACK.read().is_some();
            if has {
                *TRACK.write() = None;
            }
            return CallNextHookEx(None, code, wparam, lparam);
        };

        let msg = wparam.0 as u32;
        let (down_msg, up_msg) = match cfg.trigger {
            Trigger::Right => (WM_RBUTTONDOWN, WM_RBUTTONUP),
            Trigger::Middle => (WM_MBUTTONDOWN, WM_MBUTTONUP),
            Trigger::X1 => (WM_XBUTTONDOWN, WM_XBUTTONUP),
        };
        // 侧键要确认是 XBUTTON1（靠手心那个）
        let is_x1 = if matches!(cfg.trigger, Trigger::X1) {
            ((ms.mouseData >> 16) & 0xFFFF) == 1
        } else {
            true
        };

        if msg == down_msg && is_x1 {
            // 全屏/排除程序：完全不介入，让系统照常处理
            if !should_handle(&cfg) {
                log::debug!("手势: 本次按下被忽略（全屏或排除程序）");
                return CallNextHookEx(None, code, wparam, lparam);
            }
            if on_down(ms.pt.x, ms.pt.y, cfg.swallow_down) {
                return LRESULT(1);
            }
        } else if msg == up_msg && is_x1 {
            if on_up() {
                return LRESULT(1);
            }
        } else if msg == WM_MOUSEMOVE {
            on_move(ms.pt.x, ms.pt.y, cfg.threshold, cfg.step, &cfg.custom);
        } else if msg == WM_LBUTTONDOWN {
            cancel_if_gesturing();
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// 钩子是否真的装上了。
///
/// 目前没有界面读它，但保留：`INSTALLED` 之前被错当成"线程起来了"，
/// 这个 getter 是那条 bug 的回归锚点（见 install 的注释）。
#[allow(dead_code)]
pub fn is_installed() -> bool {
    INSTALLED.load(Ordering::SeqCst)
}
