//! 低级鼠标钩子 —— 鼠标手势的输入源。
//!
//! 和 `keyhook.rs` 同一套纪律：
//!   1. 钩子必须跑在有消息循环的线程里；
//!   2. 回调有超时限制，所以这里**只做计算**，具体动作丢给上层 spawn；
//!   3. 吞事件返回 `LRESULT(1)`，放行 `CallNextHookEx`。
//!
//! 为什么"不吞按下、只吞抬起"：
//!   绝大多数程序的右键菜单是在 `WM_RBUTTONUP` 才弹的，所以只要在抬手时
//!   吞掉，菜单就不会出现；而按住右键不放的正常操作（游戏、3D 软件里转视角）
//!   在按下阶段完全不受影响。反过来若吞掉按下，那些程序会进入"半按下"状态，
//!   问题更大。
//!
//! 状态机里所有加锁的地方都用**块作用域**：`parking_lot` 的 guard 不是重入的，
//! 读锁没放掉就写锁会直接死锁，而这个线程卡死 = 整个系统鼠标卡死。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use parking_lot::RwLock;
use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    MSLLHOOKSTRUCT, MSG, WH_MOUSE_LL, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN,
    WM_RBUTTONUP, WM_XBUTTONDOWN, WM_XBUTTONUP,
};

use crate::config::GestureConfig;

/// 钩子吐出的事件。上层（`gesture.rs`）据此画轨迹、匹配动作。
#[derive(Debug, Clone)]
pub enum Event {
    /// 拖动距离越过阈值，正式进入手势模式
    Begin { x: i32, y: i32 },
    /// 手势过程中的每一次移动
    Move { x: i32, y: i32 },
    /// 抬手。`pattern` 形如 "DR"
    End { pattern: String },
    /// 手势被放弃（动了但没划出方向）
    Cancel,
}

type Sink = Box<dyn Fn(Event) + Send + Sync>;

static SINK: OnceLock<Sink> = OnceLock::new();
static CFG: OnceLock<RwLock<GestureConfig>> = OnceLock::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);

#[derive(Default)]
struct Track {
    /// 触发键是否按着
    down: bool,
    /// 是否已越过阈值（进入手势模式）
    gesture: bool,
    /// 按下的位置
    origin: (i32, i32),
    /// 上一次"转向"的基准点
    anchor: (i32, i32),
    last_dir: Option<char>,
    pattern: String,
}

static TRACK: RwLock<Option<Track>> = RwLock::new(None);

/// 刷新配置。设置页改了触发键/阈值之后调这个，钩子立刻按新配置走。
pub fn update_config(cfg: GestureConfig) {
    match CFG.get() {
        Some(lock) => *lock.write() = cfg,
        None => {
            let _ = CFG.set(RwLock::new(cfg));
        }
    }
}

pub fn install(sink: impl Fn(Event) + Send + Sync + 'static) {
    let _ = SINK.set(Box::new(sink));
    if INSTALLED.swap(true, Ordering::SeqCst) {
        return;
    }

    std::thread::spawn(|| unsafe {
        use windows::Win32::Foundation::HINSTANCE;
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;

        let hinst = match GetModuleHandleW(None) {
            Ok(h) => HINSTANCE(h.0),
            Err(e) => {
                log::error!("获取模块句柄失败，鼠标手势不可用: {e}");
                return;
            }
        };
        match SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_proc), Some(hinst), 0) {
            Ok(_) => log::info!("鼠标手势钩子已安装"),
            Err(e) => {
                log::error!("鼠标钩子安装失败（可能被安全软件拦截）: {e}");
                return;
            }
        }

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&mut msg);
        }
    });
}

enum Trigger {
    Right,
    Middle,
    X1,
}

/// 读一次配置。关掉时返回 None，钩子只放行不做事。
fn current() -> Option<(Trigger, i32, i32)> {
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
    Some((
        trigger,
        cfg.threshold.clamp(8, 200),
        cfg.step.clamp(16, 300),
    ))
}

fn emit(ev: Event) {
    if let Some(sink) = SINK.get() {
        sink(ev);
    }
}

fn on_down(x: i32, y: i32) {
    *TRACK.write() = Some(Track {
        down: true,
        gesture: false,
        origin: (x, y),
        anchor: (x, y),
        last_dir: None,
        pattern: String::new(),
    });
}

fn on_move(x: i32, y: i32, threshold: i32, step: i32) {
    // 第一段：更新状态，算出要不要发 Begin
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
    {
        let mut guard = TRACK.write();
        let Some(t) = guard.as_mut() else { return };
        if !t.gesture {
            return;
        }
        let (dx, dy) = (x - t.anchor.0, y - t.anchor.1);
        if dx.abs() >= step || dy.abs() >= step {
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
            }
            // 锚点跟着走：同一方向连续移动只会记一次
            t.anchor = (x, y);
        }
    }

    emit(Event::Move { x, y });
}

/// 触发键抬起。返回 true 表示要把这次抬起吞掉。
fn on_up() -> bool {
    let taken = {
        let mut guard = TRACK.write();
        guard.take()
    };
    let Some(t) = taken else { return false };
    if !t.down || !t.gesture {
        // 只是点了一下右键 → 完全放行，菜单照常弹
        return false;
    }
    if t.pattern.is_empty() {
        // 动了但没划出方向（手抖）→ 也放行，否则这次右键就白按了
        emit(Event::Cancel);
        return false;
    }
    emit(Event::End { pattern: t.pattern });
    true
}

unsafe extern "system" fn mouse_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code >= 0 {
        if let Some((trigger, threshold, step)) = current() {
            let ms = &*(lparam.0 as *const MSLLHOOKSTRUCT);
            let msg = wparam.0 as u32;

            let (down_msg, up_msg) = match trigger {
                Trigger::Right => (WM_RBUTTONDOWN, WM_RBUTTONUP),
                Trigger::Middle => (WM_MBUTTONDOWN, WM_MBUTTONUP),
                Trigger::X1 => (WM_XBUTTONDOWN, WM_XBUTTONUP),
            };
            // 侧键要确认是 XBUTTON1（靠近手心那个）
            let is_x1 = if matches!(trigger, Trigger::X1) {
                ((ms.mouseData >> 16) & 0xFFFF) == 1
            } else {
                true
            };

            if msg == down_msg && is_x1 {
                on_down(ms.pt.x, ms.pt.y);
            } else if msg == up_msg && is_x1 {
                if on_up() {
                    return LRESULT(1);
                }
            } else if msg == WM_MOUSEMOVE {
                on_move(ms.pt.x, ms.pt.y, threshold, step);
            }
        } else {
            // 配置被关掉时清掉残留状态（读锁先放掉再取写锁）
            let has = TRACK.read().is_some();
            if has {
                *TRACK.write() = None;
            }
        }
    }
    CallNextHookEx(None, code, wparam, lparam)
}

/// 钩子是否真的装上了（设置页用来显示状态）。
pub fn is_installed() -> bool {
    INSTALLED.load(Ordering::SeqCst)
}
