use crate::error::AppResult;
use crate::state::AppState;
use crate::types::HotkeyResult;
use std::sync::Arc;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

/// 把用户手打的别名归一成 tauri 认的名字。
///
/// `Win+F1` 这种写法 tauri 会直接拒绝（`Couldn't recognize "Win"`），
/// 而用户脑子里 Windows 键就是 Win，也常写成 Meta / Cmd。
/// 与其教育用户"请写 Super"，不如在注册前转换一次 —— 转换结果会写回配置，
/// 免得界面显示的键和实际注册的键不是同一个东西。
fn normalize(accel: &str) -> String {
    accel
        .split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(|p| match p.to_ascii_lowercase().as_str() {
            "win" | "windows" | "meta" | "cmd" | "command" | "super" => "Super".into(),
            "control" => "Ctrl".into(),
            "option" => "Alt".into(),
            "esc" => "Escape".into(),
            "ins" => "Insert".into(),
            "del" => "Delete".into(),
            "return" | "enter" => "Enter".into(),
            _ => p.to_string(),
        })
        .collect::<Vec<_>>()
        .join("+")
}

/// 注册全部全局快捷键。
///
/// 注册失败必须如实记录——被别的软件占用了却静默失败，
/// 用户只会以为"这功能坏了"。
pub fn register_all(app: &AppHandle, state: &Arc<AppState>) -> AppResult<()> {
    // 1) 先纠正写法（Win → Super 之类），有变化就落盘
    let mut changed = false;
    {
        let mut s = state.settings.write();
        macro_rules! fix {
            ($field:ident) => {{
                let v = normalize(&s.hotkeys.$field);
                if v != s.hotkeys.$field {
                    log::info!("快捷键写法已纠正: {} → {}", s.hotkeys.$field, v);
                    s.hotkeys.$field = v;
                    changed = true;
                }
            }};
        }
        fix!(region_capture);
        fix!(fullscreen_capture);
        fix!(active_window_capture);
        fix!(scroll_capture);
        fix!(clipboard_panel);
        fix!(pin_from_clipboard);
        fix!(toggle_recording);
        fix!(toggle_recording_gif);
    }
    if changed {
        if let Err(e) = state.persist_settings() {
            log::warn!("纠正快捷键写法后保存失败: {e}");
        }
    }

    let cfg = state.settings.read().hotkeys.clone();
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();

    let mut results = Vec::new();

    macro_rules! bind {
        ($id:expr, $label:expr, $accel:expr, $mode:expr) => {{
            let accel = $accel.clone();
            if accel.trim().is_empty() {
                results.push(HotkeyResult {
                    id: $id.to_string(),
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
                        id: $id.to_string(),
                        action: $label.to_string(),
                        accelerator: accel,
                        ok: true,
                        reason: None,
                    }),
                    Err(e) => {
                        log::warn!("快捷键 {} ({}) 注册失败: {e}", $label, accel);
                        results.push(HotkeyResult {
                            id: $id.to_string(),
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

    bind!("regionCapture", "区域截图", cfg.region_capture, "region");
    bind!("fullscreenCapture", "全屏截图", cfg.fullscreen_capture, "fullscreen");
    bind!(
        "activeWindowCapture",
        "窗口截图",
        cfg.active_window_capture,
        "active-window"
    );
    bind!("scrollCapture", "长截图", cfg.scroll_capture, "scroll");

    // 录屏：视频和 GIF 各一个键，按哪个就录哪种；录着的时候两个键都是"停止"
    for (id, label, accel, fmt) in [
        (
            "toggleRecording",
            "录屏",
            cfg.toggle_recording.clone(),
            crate::commands::record::Fmt::Mp4,
        ),
        (
            "toggleRecordingGif",
            "录屏 GIF",
            cfg.toggle_recording_gif.clone(),
            crate::commands::record::Fmt::Gif,
        ),
    ] {
        if accel.trim().is_empty() {
            results.push(HotkeyResult {
                id: id.into(),
                action: label.into(),
                accelerator: String::new(),
                ok: false,
                reason: Some("未设置".into()),
            });
            continue;
        }
        let outcome = gs.on_shortcut(accel.as_str(), move |app, _sc, event| {
            if event.state() == ShortcutState::Pressed {
                crate::commands::record::toggle(app, fmt);
            }
        });
        if let Err(e) = &outcome {
            log::warn!("快捷键 {label} ({accel}) 注册失败: {e}");
        }
        results.push(HotkeyResult {
            id: id.into(),
            action: label.into(),
            accelerator: accel,
            ok: outcome.is_ok(),
            reason: outcome.err().map(|e| format!("注册失败: {e}")),
        });
    }

    // 面板类快捷键动作不同，单独处理
    {
        let accel = cfg.clipboard_panel.clone();
        if accel.trim().is_empty() {
            results.push(HotkeyResult {
                id: "clipboardPanel".into(),
                action: "剪贴板面板".into(),
                accelerator: String::new(),
                ok: false,
                reason: Some("未设置".into()),
            });
        } else {
            let outcome = gs.on_shortcut(accel.as_str(), |app, _sc, event| {
                if event.state() == ShortcutState::Pressed {
                    crate::panel::toggle(app);
                }
            });
            if let Err(e) = &outcome {
                log::warn!("快捷键 剪贴板面板 ({accel}) 注册失败: {e}");
            }
            results.push(HotkeyResult {
                id: "clipboardPanel".into(),
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
        if accel.trim().is_empty() {
            results.push(HotkeyResult {
                id: "pinFromClipboard".into(),
                action: "剪贴板贴图".into(),
                accelerator: String::new(),
                ok: false,
                reason: Some("未设置".into()),
            });
        } else {
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
                id: "pinFromClipboard".into(),
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
