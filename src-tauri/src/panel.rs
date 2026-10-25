//! 剪贴板悬浮面板 —— 全项目里最像「系统功能」的一块。
//!
//! 设计目标就一条：**在哪按快捷键，就在哪粘贴**。
//!
//! 流程（顺序错一步就不work）：
//!   1. 按 Win+V / Alt+V 时，**先**记下当前前台窗口；
//!   2. 再弹面板（这一步会抢走焦点）；
//!   3. 用户点条目 → 内容写进系统剪贴板 → 收起面板；
//!   4. 把焦点还给第 1 步记下的窗口 → 模拟 Ctrl+V。
//!
//! 第 1 步和第 4 步是这套体验的全部价值。少了它们，用户只是「在面板里点了下」，
//! 还得自己回原窗口按 Ctrl+V —— 那和看历史记录没区别。

use crate::error::{AppError, AppResult};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};

const PANEL: &str = "panel";

/// 面板逻辑尺寸（DIP）。高度只是上限，屏幕矮的时候会按比例压。
const PANEL_W: f64 = 408.0;
const PANEL_H: f64 = 470.0;

/// 刚 show 出来这段时间内，忽略「失焦就自动收起」。
///
/// 窗口从隐藏变可见时，Windows 会先发一次 Focused(false)（WA_INACTIVE），
/// 然后再决定给不给焦点。把这第一次当成「用户点到别处了」直接藏起来，
/// 面板就在用户眼前闪一下不见了 —— 表现就是「按了 Win+V 没反应」。
/// 实测第一下 100% 命中，第二下才正常，正是用户说的「要按好几次」。
const AUTOHIDE_GRACE_MS: i64 = 800;

/// 最近一次 show 的时间戳（毫秒）
static SHOWN_AT: AtomicI64 = AtomicI64::new(0);

fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// 呼出面板时的前台窗口句柄。0 表示「没有可粘贴的目标」，此时只能复制。
static TARGET: OnceLock<Mutex<isize>> = OnceLock::new();

fn cell() -> &'static Mutex<isize> {
    TARGET.get_or_init(|| Mutex::new(0))
}

pub fn target() -> isize {
    cell().lock().map(|g| *g).unwrap_or(0)
}

fn set_target(hwnd: isize) {
    if let Ok(mut g) = cell().lock() {
        *g = hwnd;
    }
}

pub fn is_visible(app: &AppHandle) -> bool {
    app.get_webview_window(PANEL)
        .and_then(|w| w.is_visible().ok())
        .unwrap_or(false)
}

/// 面板自己的窗口句柄（0 表示还没建）
fn panel_hwnd(app: &AppHandle) -> isize {
    app.get_webview_window(PANEL)
        .and_then(|w| w.hwnd().ok())
        .map(|h| h.0 as isize)
        .unwrap_or(0)
}

/// 面板现在是不是前台。用来区分「用户想关掉」和「用户想把它叫回来」。
pub fn is_focused(app: &AppHandle) -> bool {
    let h = panel_hwnd(app);
    h != 0 && crate::win32::focus::foreground_hwnd() == h
}

pub fn toggle(app: &AppHandle) {
    if is_visible(app) {
        if is_focused(app) {
            hide(app);
        } else {
            // 可见但没焦点：焦点没抢到，或者用户点回原窗口后又按了 Win+V。
            // 这时候再按一下应该是「把它叫回来重新抢焦点」，不是关掉 ——
            // 关掉的话用户第二下就白按了，和我们正在修的毛病一模一样。
            show(app);
        }
    } else {
        show(app);
    }
}

pub fn hide(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(PANEL) {
        let _ = win.hide();
    }
}

pub fn show(app: &AppHandle) {
    // 记目标必须放在弹窗之前 —— 弹窗一出现，前台窗口就变成我们自己了
    if !is_visible(app) {
        let fg = crate::win32::focus::foreground_hwnd();
        if fg != 0 && !crate::win32::focus::is_own_window(fg) {
            set_target(fg);
            log::debug!("剪贴板面板目标窗口: {fg:#x}");
        } else {
            // 从 SnipForge 自己的界面呼出，没有外部窗口可粘，退化成「只复制」
            set_target(0);
        }
    }

    // 打开前主动扫一次剪贴板。
    // 「刚 Ctrl+C 就按 Win+V」是最高频的用法，500ms 轮询多半还没跑到，
    // 列表里就会缺这一条 —— 用户看到的正是「按了没反应 / 粘出来是上一次的内容」。
    crate::clipboard::sync_now(app);

    // 标记"刚弹出来"，让紧接着的那次 Focused(false) 别把面板藏掉
    SHOWN_AT.store(now_ms(), Ordering::SeqCst);

    let (pos, size) = placement(app);

    // 面板是复用的：第一次创建后一直藏着，再次呼出只是 show + 重新定位。
    // 这样第二次开始是「瞬间出现」，不会有 webview 冷启动的那 200ms 空白。
    if let Some(win) = app.get_webview_window(PANEL) {
        let _ = win.set_position(pos);
        let _ = win.set_size(size);
        let _ = win.show();
        take_focus(&win);
        let _ = win.emit_to(PANEL, "panel://open", ());
        return;
    }

    let win = match build_panel(app, pos, size) {
        Ok(w) => w,
        Err(e) => {
            log::error!("创建剪贴板面板失败: {e}");
            return;
        }
    };

    let _ = win.show();
    take_focus(&win);

    // 首次创建时 webview 还在加载，这时候发事件会丢；
    // 前端挂载时自己也会拉一次列表，这里只是补一次「重置搜索框」的时机。
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        // webview 加载完往往会把焦点又还回去，这时候再抢一次。
        // 前提是面板还显示着、而且焦点确实没在面板上（避免从用户手里抢回来）。
        if let Some(w) = handle.get_webview_window(PANEL) {
            if w.is_visible().unwrap_or(false) && !is_focused(&handle) {
                let target = target();
                // 只有当焦点落回"原窗口"时才补抢：说明这次 show 没抢到
                if target != 0 && crate::win32::focus::foreground_hwnd() == target {
                    take_focus(&w);
                }
            }
            let _ = w.emit_to(PANEL, "panel://open", ());
        }
    });
}

/// 把面板顶到前台并确认拿到焦点。
///
/// 只调 `set_focus()` 不够：窗口从隐藏变可见时，系统的前台锁会把
/// `SetForegroundWindow` 静默拒掉，面板于是「挂在屏幕上但按键进不去」。
/// `activate_wait` 会在被拒时用 AttachThreadInput 借前台资格再试，并确认结果。
fn take_focus(win: &tauri::WebviewWindow) {
    let _ = win.set_focus();
    let Ok(h) = win.hwnd() else { return };
    let raw = h.0 as isize;
    if crate::win32::focus::foreground_hwnd() == raw {
        return;
    }
    if crate::win32::focus::activate_wait(raw, 240) {
        log::debug!("面板已拿到焦点");
    } else {
        log::warn!("面板没能拿到焦点，按键可能落到原窗口");
    }
}

/// 创建面板窗口（隐藏）。已存在就直接返回。
fn build_panel(
    app: &AppHandle,
    pos: PhysicalPosition<i32>,
    size: PhysicalSize<u32>,
) -> tauri::Result<tauri::WebviewWindow> {
    let win = WebviewWindowBuilder::new(app, PANEL, WebviewUrl::App("panel.html".into()))
        .title("SnipForge")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(true)
        .visible(false)
        .shadow(false)
        .accept_first_mouse(true)
        .inner_size(PANEL_W, PANEL_H)
        .build()?;

    let _ = win.set_position(pos);
    let _ = win.set_size(size);

    // 点到别处就收起 —— 和系统 Win+V 的手感一致，不需要用户额外按 Esc
    let handle = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(false) = event {
            // 刚弹出来的那次失焦是握手噪声，不是用户点走了
            if now_ms() - SHOWN_AT.load(Ordering::SeqCst) < AUTOHIDE_GRACE_MS {
                return;
            }
            if let Some(w) = handle.get_webview_window(PANEL) {
                let _ = w.hide();
            }
        }
    });

    Ok(win)
}

/// 启动时先把面板建好（保持隐藏）。
///
/// 第一次按 Win+V 才去建窗口的话，webview 冷启动那几百毫秒里面板是空白的，
/// 用户这时敲回车什么也不会发生 —— 又一次「按了没反应」。
/// 预先建好之后，稳态内存和用过一次之后完全一样（27MB 上下），
/// 代价只是启动时多初始化一个隐藏 webview。
pub fn prewarm(app: &AppHandle) {
    if app.get_webview_window(PANEL).is_some() {
        return;
    }
    // 位置随便给，真正显示时 show() 会按光标重新摆
    match build_panel(
        app,
        PhysicalPosition::new(0, 0),
        PhysicalSize::new(
            (PANEL_W as u32).max(280),
            (PANEL_H as u32).max(160),
        ),
    ) {
        Ok(_) => log::info!("剪贴板面板已预热（隐藏）"),
        Err(e) => log::warn!("预建剪贴板面板失败: {e}"),
    }
}

/// 光标附近找一块放得下的位置。
///
/// 全部用物理像素算：面板可能在任意 DPI 的屏上调出，
/// 混用 DIP 和物理像素必然出现"屏幕边缘溢出"。
fn placement(app: &AppHandle) -> (PhysicalPosition<i32>, PhysicalSize<u32>) {
    let cursor = app
        .cursor_position()
        .unwrap_or(PhysicalPosition::new(0.0, 0.0));
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());

    let (mx, my, mw, mh, scale) = match monitor {
        Some(m) => {
            let p = m.position();
            let s = m.size();
            (p.x, p.y, s.width as i32, s.height as i32, m.scale_factor())
        }
        None => (0, 0, 1920, 1080, 1.0),
    };

    let w = (PANEL_W * scale).round().max(280.0) as i32;
    let h = ((PANEL_H * scale).round() as i32).min((mh as f64 * 0.78) as i32);
    let gap = (12.0 * scale).round() as i32;

    // 水平方向贴光标右侧一点（不用居中：居中会让光标挡住列表首项）
    let mut x = cursor.x.round() as i32 - w / 3;
    let mut y = cursor.y.round() as i32 + gap;
    if y + h > my + mh {
        y = cursor.y.round() as i32 - h - gap; // 下方放不下就翻到光标上方
    }

    let max_x = (mx + mw - w - 6).max(mx + 6);
    let max_y = (my + mh - h - 6).max(my + 6);
    x = x.clamp(mx + 6, max_x);
    y = y.clamp(my + 6, max_y);

    (
        PhysicalPosition::new(x, y),
        PhysicalSize::new(w as u32, h.max(160) as u32),
    )
}

/// 目标窗口的进程名，给面板显示「粘贴到 记事本」这类提示。
pub fn target_app() -> String {
    let t = target();
    if t == 0 {
        return String::new();
    }
    crate::win32::window::exe_of_hwnd(t)
}

/// 面板里点了一条记录：写剪贴板 → 关面板 → 还原焦点 → Ctrl+V。
pub fn paste_item(app: &AppHandle, id: &str, copy_only: bool) -> AppResult<String> {
    let svc = app
        .try_state::<std::sync::Arc<crate::clipboard::ClipboardService>>()
        .map(|s| s.inner().clone())
        .ok_or_else(|| AppError::Msg("剪贴板服务未初始化".into()))?;

    let target = target();

    // 先收面板：立刻消失比等图片解码完再消失手感好得多
    hide(app);
    if let Some(item) = svc.store.find_by_id(id) {
        log::debug!(
            "粘贴请求: id={id} kind={} 内容={:?} 目标={target:#x}",
            item.kind,
            item.text.as_deref().unwrap_or("").chars().take(20).collect::<String>()
        );
    }
    let kind = crate::clipboard::paste(&svc, id)?;

    if copy_only || target == 0 {
        return Ok(kind);
    }

    let handle = app.clone();
    std::thread::spawn(move || {
        // 等面板**真的**不可见再动手。
        // 窗口隐藏是异步的，以前这里写死 90ms 盲等 —— 机器一忙就抢跑，
        // Ctrl+V 打在我们自己身上，用户看到的就是「点了没反应」。
        let deadline = std::time::Instant::now() + std::time::Duration::from_millis(400);
        while is_visible(&handle) && std::time::Instant::now() < deadline {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }

        // 焦点没抢回来就绝不注入：那会把按键打到别的窗口上，
        // 比「什么都不发生」更糟。内容已经在剪贴板里，用户可以手动 Ctrl+V。
        if !crate::win32::focus::activate_wait(target, 450) {
            log::warn!("还原焦点失败，未注入 Ctrl+V（内容已在剪贴板，可手动粘贴）");
            return;
        }
        // 个别程序在拿到焦点的头几十毫秒里还没准备好接收输入
        std::thread::sleep(std::time::Duration::from_millis(35));
        if let Err(e) = crate::win32::input::paste_hotkey() {
            log::warn!("模拟粘贴失败: {e}");
        }
    });

    Ok(kind)
}
