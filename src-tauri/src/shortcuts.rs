use crate::error::AppResult;
use crate::state::AppState;
use crate::types::HotkeyResult;
use std::sync::Arc;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// 注册全部全局快捷键。
///
/// 注册失败必须如实记录——被别的软件占用了却静默失败，
/// 用户只会以为"这功能坏了"。
pub fn register_all(app: &AppHandle, state: &Arc<AppState>) -> AppResult<()> {
    let cfg = state.settings.read().hotkeys.clone();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();

    let mut results = Vec::new();

    macro_rules! bind {
        ($accel:expr, $label:expr, $mode:expr) => {{
            let accel = $accel.clone();
            if accel.trim().is_empty() {
                results.push(HotkeyResult {
                    action: $label.to_string(),
                    accelerator: String::new(),
                    ok: false,
                    reason: Some("未设置".into()),
                });
            } else {
                let mode = $mode;
                let outcome = gs.on_shortcut(accel.as_str(), move |app, _sc, event| {
                    if event.state() == ShortcutState::Pressed {
                        crate::commands::capture::trigger(app, mode);
                    }
                });
                match outcome {
                    Ok(_) => results.push(HotkeyResult {
                        action: $label.to_string(),
                        accelerator: accel,
                        ok: true,
                        reason: None,
                    }),
                    Err(e) => {
                        log::warn!("快捷键 {} ({}) 注册失败: {e}", $label, accel);
                        results.push(HotkeyResult {
                            action: $label.to_string(),
                            accelerator: accel,
                            ok: false,
                            reason: Some(format!("注册失败: {e}")),
                        })
                    }
                }
            }
        }};
    }

    bind!(cfg.region_capture, "区域截图", "region");
    bind!(cfg.fullscreen_capture, "全屏截图", "fullscreen");
    bind!(cfg.active_window_capture, "窗口截图", "active-window");
    bind!(cfg.scroll_capture, "长截图", "scroll");

    // 面板类快捷键动作不同，单独处理
    {
        let accel = cfg.clipboard_panel.clone();
        if !accel.trim().is_empty() {
            let outcome = gs.on_shortcut(accel.as_str(), |app, _sc, event| {
                if event.state() == ShortcutState::Pressed {
                    crate::panel::toggle(app);
                }
            });
            if let Err(e) = &outcome {
                log::warn!("快捷键 剪贴板面板 ({accel}) 注册失败: {e}");
            }
            results.push(HotkeyResult {
                action: "剪贴板面板".into(),
                accelerator: accel,
                ok: outcome.is_ok(),
                reason: outcome.err().map(|e| format!("注册失败: {e}")),
            });
        }
    }

    // 贴图：把剪贴板里的图片直接钉到屏幕上
    {
        let accel = cfg.pin_from_clipboard.clone();
        if !accel.trim().is_empty() {
            let outcome = gs.on_shortcut(accel.as_str(), |app, _sc, event| {
                if event.state() == ShortcutState::Pressed {
                    // 剪贴板贴图是异步命令（要建窗口，不能卡在事件线程上）
                    let handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(e) =
                            crate::commands::clipboard::clipboard_pin_current(handle).await
                        {
                            log::warn!("贴图失败: {e}");
                        }
                    });
                }
            });
            if let Err(e) = &outcome {
                log::warn!("快捷键 剪贴板贴图 ({accel}) 注册失败: {e}");
            }
            results.push(HotkeyResult {
                action: "剪贴板贴图".into(),
                accelerator: accel,
                ok: outcome.is_ok(),
                reason: outcome.err().map(|e| format!("注册失败: {e}")),
            });
        }
    }

    let ok_count = results.iter().filter(|r| r.ok).count();
    log::info!("快捷键注册 {}/{}", ok_count, results.len());
    *state.hotkey_results.write() = results;
    Ok(())
}

pub fn unregister_all(app: &AppHandle) {
    let _ = app.global_shortcut().unregister_all();
}
