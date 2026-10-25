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
use std::sync::{Mutex, OnceLock};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};

const PANEL: &str = "panel";

/// 面板逻辑尺寸（DIP）。高度只是上限，屏幕矮的时候会按比例压。
const PANEL_W: f64 = 408.0;
const PANEL_H: f64 = 470.0;

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

pub fn toggle(app: &AppHandle) {
    if is_visible(app) {
        hide(app);
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

    let (pos, size) = placement(app);

    // 面板是复用的：第一次创建后一直藏着，再次呼出只是 show + 重新定位。
    // 这样第二次开始是「瞬间出现」，不会有 webview 冷启动的那 200ms 空白。
    if let Some(win) = app.get_webview_window(PANEL) {
        let _ = win.set_position(pos);
        let _ = win.set_size(size);
        let _ = win.show();
        let _ = win.set_focus();
        let _ = win.emit_to(PANEL, "panel://open", ());
        return;
    }

    let built = WebviewWindowBuilder::new(app, PANEL, WebviewUrl::App("panel.html".into()))
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
        .build();

    let win = match built {
        Ok(w) => w,
        Err(e) => {
            log::error!("创建剪贴板面板失败: {e}");
            return;
        }
    };

    let _ = win.set_position(pos);
    let _ = win.set_size(size);

    // 点到别处就收起 —— 和系统 Win+V 的手感一致，不需要用户额外按 Esc
    let handle = app.clone();
    win.on_window_event(move |event| {
        if let tauri::WindowEvent::Focused(false) = event {
            if let Some(w) = handle.get_webview_window(PANEL) {
                let _ = w.hide();
            }
        }
    });

    let _ = win.show();
    let _ = win.set_focus();

    // 首次创建时 webview 还在加载，这时候发事件会丢；
    // 前端挂载时自己也会拉一次列表，这里只是补一次「重置搜索框」的时机。
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(200));
        if let Some(w) = handle.get_webview_window(PANEL) {
            let _ = w.emit_to(PANEL, "panel://open", ());
        }
    });
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
    let kind = crate::clipboard::paste(&svc, id)?;

    if copy_only || target == 0 {
        return Ok(kind);
    }

    std::thread::spawn(move || {
        // 等面板真正消失。窗口隐藏是异步的，这里抢跑的话
        // Ctrl+V 会打在我们自己身上，用户看到的就是「点了没反应」。
        std::thread::sleep(std::time::Duration::from_millis(90));
        match crate::win32::focus::activate(target) {
            Ok(()) => {
                std::thread::sleep(std::time::Duration::from_millis(110));
                if let Err(e) = crate::win32::input::paste_hotkey() {
                    log::warn!("模拟粘贴失败: {e}");
                }
            }
            Err(e) => log::warn!("还原焦点失败（内容已在剪贴板，可手动 Ctrl+V）: {e}"),
        }
    });

    Ok(kind)
}
