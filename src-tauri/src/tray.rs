//! 托盘菜单。
//!
//! 拆成三件事，别混在一起：
//!   - `build_menu`：按当前配置**重建菜单**（快捷键文案、手势开关的勾选状态）
//!   - `register_events`：菜单/图标事件处理。**只注册一次**
//!   - `refresh_menu`：配置变了、只需要刷新菜单时调，不碰事件处理
//!
//! 为什么非要分开：Tauri 的 `TrayIcon::on_menu_event` 是往全局监听列表里
//! `push` 的，不是覆盖。以前 `refresh_menu` 直接复用 `setup`，于是每改一次
//! 快捷键（或手势）就多挂一个处理器 —— 点一次"区域截图"会截 N 张图，
//! 而且旧处理器永远释放不掉。这个坑很难在功能测试里发现，只能靠结构避免。

use crate::commands::record::Fmt;
use crate::error::AppResult;
use crate::state::AppState;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::{AppHandle, Manager};

/// 事件处理是否已经挂过
static EVENTS_REGISTERED: AtomicBool = AtomicBool::new(false);

/// 启动时调一次：建菜单 + 挂事件。
pub fn setup(app: &AppHandle, state: &Arc<AppState>) -> AppResult<()> {
    build_menu(app, state)?;
    register_events(app);
    Ok(())
}

/// 只刷新菜单（快捷键文案、手势勾选状态变了之后调）。
///
/// 事件处理不会重复注册 —— 见文件头。
pub fn refresh_menu(app: &AppHandle, state: &Arc<AppState>) {
    if let Err(e) = build_menu(app, state) {
        log::warn!("刷新托盘菜单失败: {e}");
    }
}

fn build_menu(app: &AppHandle, state: &Arc<AppState>) -> AppResult<()> {
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
    let a_record = accel(&hk.toggle_recording);
    let a_record_gif = accel(&hk.toggle_recording_gif);

    let region = MenuItem::with_id(app, "capture_region", "区域截图", true, a_region.as_deref())?;
    let full = MenuItem::with_id(app, "capture_full", "全屏截图", true, a_full.as_deref())?;
    let scroll =
        MenuItem::with_id(app, "capture_scroll", "滚动长截图", true, a_scroll.as_deref())?;
    let sep1 = PredefinedMenuItem::separator(app)?;
    let panel = MenuItem::with_id(app, "panel_clipboard", "剪贴板面板", true, a_panel.as_deref())?;
    let history = MenuItem::with_id(app, "show_clipboard", "剪贴板管理", true, None::<&str>)?;

    // 手势总开关：托盘是常驻入口，临时"别管我的右键"时不用翻设置页。
    // 用勾选项而不是两个菜单项：状态和动作是同一处，不会出现"点了却不知道现在开没开"。
    let gesture_on = state.settings.read().gestures.enabled;
    let gesture = CheckMenuItem::with_id(
        app,
        "gesture_toggle",
        "鼠标手势",
        true,
        gesture_on,
        None::<&str>,
    )?;

    let sep_rec = PredefinedMenuItem::separator(app)?;
    // 视频和 GIF 分成两个子菜单：点哪个录哪个，不用先进软件改设置
    let vid_region = MenuItem::with_id(app, "rec_mp4_region", "录制区域…", true, a_record.as_deref())?;
    let vid_full = MenuItem::with_id(app, "rec_mp4_full", "录制全屏", true, None::<&str>)?;
    let gif_region =
        MenuItem::with_id(app, "rec_gif_region", "录制区域…", true, a_record_gif.as_deref())?;
    let gif_full = MenuItem::with_id(app, "rec_gif_full", "录制全屏", true, None::<&str>)?;
    let rec_video = Submenu::with_items(app, "录制视频 (MP4)", true, &[&vid_region, &vid_full])?;
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
            &gesture,
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
    log::debug!("托盘菜单已刷新（鼠标手势={}）", if gesture_on { "开" } else { "关" });
    Ok(())
}

fn register_events(app: &AppHandle) {
    // 只挂一次：重复挂的后果见文件头
    if EVENTS_REGISTERED.swap(true, Ordering::SeqCst) {
        return;
    }

    let Some(tray) = app.tray_by_id("main-tray") else {
        EVENTS_REGISTERED.store(false, Ordering::SeqCst);
        return;
    };

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
            "gesture_toggle" => {
                // 菜单回调在主线程：这里只翻配置 + 重画菜单，不碰窗口
                let h = handle.clone();
                tauri::async_runtime::spawn(async move {
                    if let Some(st) = h.try_state::<Arc<AppState>>() {
                        let on = !st.settings.read().gestures.enabled;
                        if let Err(e) = crate::commands::settings::set_gestures_enabled(&h, &st, on)
                        {
                            log::warn!("切换手势开关失败: {e}");
                        }
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
}
