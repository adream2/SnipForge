use crate::commands::record::Fmt;
use crate::error::AppResult;
use crate::state::AppState;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager};

/// 构建托盘菜单。
///
/// 托盘是常驻截图工具的主入口——用户大多数时候不开主窗口，
/// 所以每项都带上快捷键，让「按什么键」和「有什么功能」一起呈现，
/// 而不是让人猜。
pub fn setup(app: &AppHandle, state: &Arc<AppState>) -> AppResult<()> {
    let hk = state.settings.read().hotkeys.clone();
    let accel = |s: &str| -> Option<String> {
        let t = s.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    };

    let a_region = accel(&hk.region_capture);
    let a_full = accel(&hk.fullscreen_capture);
    let a_scroll = accel(&hk.scroll_capture);
    let a_panel = accel(&hk.clipboard_panel);
    let a_pin_clip = accel(&hk.pin_from_clipboard);
    let a_record = accel(&hk.toggle_recording);
    let a_record_gif = accel(&hk.toggle_recording_gif);

    let region = MenuItem::with_id(app, "capture_region", "区域截图", true, a_region.as_deref())?;
    let full = MenuItem::with_id(app, "capture_full", "全屏截图", true, a_full.as_deref())?;
    let scroll =
        MenuItem::with_id(app, "capture_scroll", "滚动长截图", true, a_scroll.as_deref())?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let panel =
        MenuItem::with_id(app, "panel_clipboard", "剪贴板面板", true, a_panel.as_deref())?;
    let history =
        MenuItem::with_id(app, "show_clipboard", "剪贴板管理", true, None::<&str>)?;
    let pin_clip =
        MenuItem::with_id(app, "pin_clipboard", "贴剪贴板图片", true, a_pin_clip.as_deref())?;
    let sep_rec = PredefinedMenuItem::separator(app)?;
    // 视频和 GIF 分成两个子菜单：点哪个录哪个，不用先进软件改设置
    let vid_region =
        MenuItem::with_id(app, "rec_mp4_region", "录制区域…", true, a_record.as_deref())?;
    let vid_full = MenuItem::with_id(app, "rec_mp4_full", "录制全屏", true, None::<&str>)?;
    let gif_region =
        MenuItem::with_id(app, "rec_gif_region", "录制区域…", true, a_record_gif.as_deref())?;
    let gif_full = MenuItem::with_id(app, "rec_gif_full", "录制全屏", true, None::<&str>)?;
    let rec_video =
        Submenu::with_items(app, "录制视频 (MP4)", true, &[&vid_region, &vid_full])?;
    let rec_gif = Submenu::with_items(app, "录制 GIF", true, &[&gif_region, &gif_full])?;
    let rec_stop = MenuItem::with_id(app, "record_stop", "停止录制", true, None::<&str>)?;
    let tools = MenuItem::with_id(app, "show_tools", "图片工具", true, None::<&str>)?;
    let upload = MenuItem::with_id(app, "show_upload", "图床设置", true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "show_settings", "设置…", true, None::<&str>)?;
    let sep2 = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "退出 SnipForge", true, None::<&str>)?;

    let menu = Menu::with_items(
        app,
        &[
            &region,
            &full,
            &scroll,
            &sep1,
            &panel,
            &history,
            &pin_clip,
            &sep_rec,
            &rec_video,
            &rec_gif,
            &rec_stop,
            &tools,
            &upload,
            &settings,
            &sep2,
            &quit,
        ],
    )?;

    let Some(tray) = app.tray_by_id("main-tray") else {
        log::warn!("未找到托盘图标，检查 tauri.conf.json 的 trayIcon 配置");
        return Ok(());
    };

    tray.set_menu(Some(menu))?;
    tray.set_tooltip(Some("SnipForge · 截图 / 剪贴板 / 图床"))?;

    let handle = app.clone();
    tray.on_menu_event(move |_app, event| {
        let id = event.id().as_ref();
        match id {
            "capture_region" => crate::commands::capture::trigger(&handle, "region"),
            "capture_full" => crate::commands::capture::trigger(&handle, "fullscreen"),
            "capture_scroll" => crate::commands::capture::trigger(&handle, "scroll"),
            // 托盘菜单回调跑在主线程，而 panel::show 里的 cursor_position() /
            // monitor_from_point() 会把请求投递到主线程再等结果 —— 直接调用就自锁。
            // 所以一律丢给异步运行时，这里立刻返回。
            "panel_clipboard" => {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    crate::panel::toggle(&h);
                });
            }
            "pin_clipboard" => {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::commands::clipboard::clipboard_pin_current(h).await {
                        log::warn!("贴剪贴板图片失败: {e}");
                    }
                });
            }
            "rec_mp4_region" | "rec_gif_region" => {
                let fmt = if id == "rec_gif_region" { Fmt::Gif } else { Fmt::Mp4 };
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::commands::record::trigger_region_record(&h, fmt) {
                        log::warn!("录制区域失败: {e}");
                    }
                });
            }
            "rec_mp4_full" | "rec_gif_full" => {
                let fmt = if id == "rec_gif_full" { Fmt::Gif } else { Fmt::Mp4 };
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::commands::record::start_fullscreen(&h, fmt) {
                        log::warn!("录制全屏失败: {e}");
                    }
                });
            }
            "record_stop" => {
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Err(e) = crate::commands::record::stop_recording(&h) {
                        log::warn!("停止录制失败: {e}");
                    }
                });
            }
            "show_clipboard" => crate::windows::show_main(&handle, Some("clipboard")),
            "show_tools" => crate::windows::show_main(&handle, Some("tools")),
            "show_upload" => crate::windows::show_main(&handle, Some("upload")),
            "show_settings" => crate::windows::show_main(&handle, Some("settings")),
            "quit" => {
                handle.exit(0);
            }
            _ => {}
        }
    });

    // 左键单击托盘 = 显示/隐藏主窗口
    let handle2 = app.clone();
    tray.on_tray_icon_event(move |_tray, event| {
        if let tauri::tray::TrayIconEvent::Click {
            button: tauri::tray::MouseButton::Left,
            button_state: tauri::tray::MouseButtonState::Up,
            ..
        } = event
        {
            crate::commands::capture::toggle_main(&handle2);
        }
    });

    Ok(())
}

/// 快捷键改了之后刷新菜单上的显示
pub fn refresh_menu(app: &AppHandle, state: &Arc<AppState>) {
    if let Err(e) = setup(app, state) {
        log::warn!("刷新托盘菜单失败: {e}");
    }
}
