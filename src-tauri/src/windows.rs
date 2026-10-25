use crate::error::{AppError, AppResult};
use crate::types::{CaptureResult, Rect};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindowBuilder,
};

const MAIN: &str = "main";
const SELECTOR: &str = "selector";
const EDITOR: &str = "editor";
/// 录制覆盖层：精确盖住录制区域，负责鼠标环/放大镜/标注。**必须被录进画面**。
const RECORD_OVERLAY: &str = "record-overlay";
/// 录制控制条。**必须排除在画面之外** —— 否则每段录像都带着一个控制条。
const RECORD_BAR: &str = "record-bar";

/// 点是否落在矩形内（物理像素坐标）
pub fn rect_contains(rect: &Rect, x: i32, y: i32) -> bool {
    x >= rect.x && x < rect.right() && y >= rect.y && y < rect.bottom()
}

/// 主窗口显示/隐藏
pub fn show_main(app: &AppHandle, route: Option<&str>) {
    let Some(win) = app.get_webview_window(MAIN) else {
        log::warn!("主窗口不存在");
        return;
    };
    let _ = win.show();
    let _ = win.unminimize();
    let _ = win.set_focus();
    if let Some(r) = route {
        let _ = win.emit_to(MAIN, "app://navigate", r);
    }
}

pub fn toggle_main(app: &AppHandle) {
    let Some(win) = app.get_webview_window(MAIN) else {
        return;
    };
    let visible = win.is_visible().unwrap_or(false);
    let focused = win.is_focused().unwrap_or(false);
    if visible && focused {
        let _ = win.hide();
    } else {
        let _ = win.show();
        let _ = win.unminimize();
        let _ = win.set_focus();
    }
}

pub fn hide_main(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(MAIN) {
        let _ = win.hide();
    }
}

pub fn emit_to_main(app: &AppHandle, event: &str, payload: impl serde::Serialize + Clone) {
    if let Some(win) = app.get_webview_window(MAIN) {
        let _ = win.emit_to(MAIN, event, payload);
    }
}

/// 在鼠标所在的显示器上打开截图覆盖层。
///
/// 为什么只覆盖一块屏？
///   WebView2 一个窗口只认一个 DPI 基准，窗口横跨两块缩放不同的屏必有错位。
///   逐屏覆盖 + 用物理像素定位，是唯一能保证像素级精确的做法。
///   用户想截另一块屏，把鼠标移过去再按热键即可——这比跨屏框选更符合直觉。
/// 打开区域选择覆盖层。
///
/// `for_record` 为 true 时前端走"录屏"分支：框完直接开录，不进编辑器。
pub fn open_selector(
    app: &AppHandle,
    display_rect: Rect,
    display_id: u32,
    for_record: bool,
) -> AppResult<()> {
    close_selector(app);

    let cursor = app
        .cursor_position()
        .map_err(|e| AppError::Msg(format!("读取鼠标位置失败: {e}")))?;

    let url = if for_record {
        WebviewUrl::App("selector.html?mode=record".into())
    } else {
        WebviewUrl::App("selector.html".into())
    };
    let builder = WebviewWindowBuilder::new(app, SELECTOR, url)
        .title(if for_record {
            "SnipForge Record Region"
        } else {
            "SnipForge Selector"
        })
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(true)
        .visible(false)
        .shadow(false)
        .accept_first_mouse(true);

    let win = builder
        .build()
        .map_err(|e| AppError::Msg(format!("创建覆盖层失败: {e}")))?;

    // 关键：用物理像素定位，绕开 DIP 换算带来的错位
    let _ = win.set_position(PhysicalPosition::new(display_rect.x, display_rect.y));
    let _ = win.set_size(PhysicalSize::new(display_rect.width, display_rect.height));

    let _ = win.emit_to(SELECTOR, "selector://display", display_id);
    let _ = win.show();
    let _ = win.set_focus();
    let _ = cursor;

    Ok(())
}

pub fn close_selector(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(SELECTOR) {
        let _ = win.destroy();
    }
}

/// 打开编辑器窗口并把截图递过去
pub fn open_editor(app: &AppHandle, capture: CaptureResult) -> AppResult<()> {
    if let Some(win) = app.get_webview_window(EDITOR) {
        let _ = win.emit_to(EDITOR, "editor://capture", capture);
        let _ = win.set_focus();
        return Ok(());
    }

    let win = WebviewWindowBuilder::new(app, EDITOR, WebviewUrl::App("editor.html".into()))
        .title("SnipForge 编辑器")
        .inner_size(1180.0, 800.0)
        .min_inner_size(880.0, 600.0)
        .decorations(false)
        .visible(false)
        .build()
        .map_err(|e| AppError::Msg(format!("创建编辑器失败: {e}")))?;

    // 等前端挂好监听再发数据，否则事件会丢
    let payload = capture;
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(320));
        if let Some(w) = handle.get_webview_window(EDITOR) {
            let _ = w.emit_to(EDITOR, "editor://capture", payload);
            let _ = w.show();
            let _ = w.set_focus();
        }
    });

    Ok(())
}

pub fn close_editor(app: &AppHandle) {
    if let Some(win) = app.get_webview_window(EDITOR) {
        let _ = win.destroy();
    }
}

/// 贴图窗口：把一张图钉在屏幕最上层。
///
/// 位置和尺寸都不是随便定的：
///   - 贴光标中心，用户马上能看见；
///   - 尺寸按屏幕 70% 收口，且**绝不放大** —— 截一张 4K 全屏图原尺寸糊脸上，
///     用户第一件事就是找关闭按钮。
pub fn open_pin(app: &AppHandle, data_url: String, img_w: u32, img_h: u32, opacity: f64) -> AppResult<()> {
    let label = format!("pin-{}", chrono::Utc::now().timestamp_millis());
    let (x, y, w, h) = pin_geometry(app, img_w, img_h);

    let win = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("pin.html".into()))
        .title("SnipForge Pin")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(true)
        .shadow(false)
        .inner_size(320.0, 200.0)
        .visible(false)
        .build()
        .map_err(|e| AppError::Msg(format!("创建贴图失败: {e}")))?;

    // 尺寸用物理像素给：多屏不同缩放时 logical 尺寸会算错
    let _ = win.set_size(PhysicalSize::new(w, h));
    let _ = win.set_position(PhysicalPosition::new(x, y));

    let label_clone = label.clone();
    let payload = serde_json::json!({ "dataUrl": data_url, "opacity": opacity });
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(220));
        if let Some(w) = app.get_webview_window(&label_clone) {
            let _ = w.emit_to(&label_clone, "pin://init", payload);
            let _ = w.show();
        }
    });

    Ok(())
}

/// 贴图窗口的初始位置和尺寸（物理像素）
fn pin_geometry(app: &AppHandle, img_w: u32, img_h: u32) -> (i32, i32, u32, u32) {
    let cursor = app
        .cursor_position()
        .unwrap_or(PhysicalPosition::new(0.0, 0.0));
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());

    let (mx, my, mw, mh, dpi) = match monitor {
        Some(m) => {
            let p = m.position();
            let s = m.size();
            (p.x, p.y, s.width as i32, s.height as i32, m.scale_factor())
        }
        None => (0, 0, 1920, 1080, 1.0),
    };

    let iw = img_w.max(1) as f64;
    let ih = img_h.max(1) as f64;
    let fit = (mw as f64 * 0.7 / iw)
        .min(mh as f64 * 0.7 / ih)
        .min(dpi)
        .max(0.05);

    let w = (iw * fit).round().max(48.0) as i32;
    let h = (ih * fit).round().max(36.0) as i32;
    let x = ((cursor.x - w as f64 / 2.0).round() as i32)
        .clamp(mx + 4, (mx + mw - w - 4).max(mx + 4));
    let y = ((cursor.y - h as f64 / 2.0).round() as i32)
        .clamp(my + 4, (my + mh - h - 4).max(my + 4));

    (x, y, w as u32, h as u32)
}


/* ------------------------ 录屏窗口 ------------------------ */

/// 打开录制用的两个窗口。
///
/// 覆盖层和录制区域**等大、同位**，并且自己被录进画面 —— 鼠标环、放大镜、
/// 标注都是它画的，这是"所见即所得"的前提。
/// 控制条反过来：用 `WDA_EXCLUDEFROMCAPTURE` 从捕获里排除，
/// 所以它摆在哪儿都不会出现在视频里。
pub fn open_record_windows(app: &AppHandle, rect: Rect) -> AppResult<()> {
    close_record_windows(app);

    // ---- 覆盖层 ----
    let overlay = WebviewWindowBuilder::new(
        app,
        RECORD_OVERLAY,
        WebviewUrl::App("overlay.html".into()),
    )
    .title("SnipForge Overlay")
    .decorations(false)
    .transparent(true)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focused(false)
    .visible(false)
    .shadow(false)
    .accept_first_mouse(true)
    .build()
    .map_err(|e| AppError::Msg(format!("创建录制覆盖层失败: {e}")))?;

    let _ = overlay.set_position(PhysicalPosition::new(rect.x, rect.y));
    let _ = overlay.set_size(PhysicalSize::new(rect.width, rect.height));
    // 默认不挡鼠标：用户是在操作别的程序，不是操作我们
    let _ = overlay.set_ignore_cursor_events(true);

    // ---- 控制条 ----
    let bar_w = 470.0_f64;
    let bar_h = 56.0_f64;
    let bar = WebviewWindowBuilder::new(app, RECORD_BAR, WebviewUrl::App("record.html".into()))
        .title("SnipForge Record")
        .decorations(false)
        .transparent(true)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .focused(false)
        .visible(false)
        .shadow(false)
        .build()
        .map_err(|e| AppError::Msg(format!("创建录制控制条失败: {e}")))?;

    // 底部居中、区域内部 —— 反正它不会被录进画面
    let bx = rect.x + (rect.width as i32 - bar_w as i32) / 2;
    let by = rect.y + rect.height as i32 - bar_h as i32 - 28;
    let _ = bar.set_position(PhysicalPosition::new(bx, by));
    let _ = bar.set_size(PhysicalSize::new(bar_w as u32, bar_h as u32));
    exclude_from_capture(&bar);

    // 两个窗口都等 webview 起来再显示，否则会闪一下白底
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(120));
        for label in [RECORD_OVERLAY, RECORD_BAR] {
            if let Some(w) = handle.get_webview_window(label) {
                let _ = w.show();
            }
        }
    });

    Ok(())
}

/// 让窗口不出现在任何屏幕捕获里（DXGI 桌面复制也会尊重这个标记）
fn exclude_from_capture(win: &tauri::WebviewWindow) {
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::HWND;
        use windows::Win32::UI::WindowsAndMessaging::{
            SetWindowDisplayAffinity, WDA_EXCLUDEFROMCAPTURE,
        };
        if let Ok(h) = win.hwnd() {
            unsafe {
                let _ = SetWindowDisplayAffinity(HWND(h.0), WDA_EXCLUDEFROMCAPTURE);
            }
        }
    }
    #[cfg(not(windows))]
    let _ = win;
}

pub fn close_record_windows(app: &AppHandle) {
    for label in [RECORD_OVERLAY, RECORD_BAR] {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.close();
        }
    }
}

/// 标注模式要接收鼠标，普通模式要穿透
pub fn set_overlay_interactive(app: &AppHandle, interactive: bool) {
    if let Some(w) = app.get_webview_window(RECORD_OVERLAY) {
        let _ = w.set_ignore_cursor_events(!interactive);
        if interactive {
            let _ = w.set_focus();
        }
    }
}

pub fn emit_to_record_overlay(app: &AppHandle, event: &str, payload: impl serde::Serialize + Clone) {
    if let Some(w) = app.get_webview_window(RECORD_OVERLAY) {
        let _ = w.emit_to(RECORD_OVERLAY, event, payload);
    }
}

pub fn emit_to_record_bar(app: &AppHandle, event: &str, payload: impl serde::Serialize + Clone) {
    if let Some(w) = app.get_webview_window(RECORD_BAR) {
        let _ = w.emit_to(RECORD_BAR, event, payload);
    }
}
