//! SnipForge —— Windows 端截图 / 录屏 / 剪贴板 / 图床 / 图片处理 一体化工具。
//!
//! 分层约定（改代码前先读）：
//!   - `commands/` 是薄壳：参数校验 → 调 service → 返回，不写业务逻辑
//!   - `win32/` 是唯一 import `windows` crate 的地方，上层只碰干净的 Rust 类型
//!   - `capture/` `clipboard/` `uploader/` 互不直接调用，跨模块通信走 `hooks`
//!   - 第三方扩展只通过 `uploader::Provider` trait 或声明式 JSON 接入

// release 二进制必须内嵌前端资源。
//
// 注意：Tauri v2 CLI **不会**自动打开本 crate 的 `custom-protocol` 特性 ——
// 它只带 `tauri/custom-protocol`（依赖的特性），`cfg(feature = "custom-protocol")`
// 仍然是假的。所以必须显式传：
//     npx tauri build --features custom-protocol
//     cargo build --release --features custom-protocol
// 忘记传就会打出一个去连 devUrl（127.0.0.1:5183）的 exe，用户看到"拒绝连接"，
// 还以为是软件坏了（这个坑真踩过）。与其发出去一个白屏 exe，不如让构建直接失败。
#[cfg(all(not(debug_assertions), not(feature = "custom-protocol")))]
compile_error!(
    "release 构建缺少 custom-protocol 特性：exe 会去连 devUrl 而不是内嵌前端。
     正确命令：`npx tauri build --features custom-protocol`
     或：`cargo build --release --features custom-protocol`。"
);

mod autostart;
mod bootstrap;
mod capture;
mod clipboard;
mod commands;
mod devtools;
mod error;
mod features;
mod gesture;
mod hooks;
mod hosts;
mod icons;
mod logger;
mod net;
mod ocr;
mod panel;
mod paths;
mod plugins;
mod record;
mod secrets;
mod shortcuts;
mod state;
mod tray;
mod update;
mod translate;
mod types;
mod uploader;
mod vault;
mod win32;
mod windows;

pub use snipforge_core::config;
pub use snipforge_core::pattern;
pub use devtools::*;

pub fn run() {
    crate::win32::ensure_single_instance(autostart::silent_start());
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(bootstrap::setup)
        .on_window_event(bootstrap::on_window_event)
        .invoke_handler(tauri::generate_handler![
            // 设置
            commands::settings_get,
            commands::app_show_main,
            commands::settings_patch,
            commands::gesture_trace,
            commands::gesture_match,
            commands::features_list,
            commands::gesture_defaults,
            commands::autostart_status,
            commands::settings_reset,
            commands::settings_pick_dir,
            // 版本检查
            commands::update::update_check,
            commands::update::update_cached,
            commands::update::update_open,
            // 关于页外链
            commands::about::about_open,
            commands::about::about_links,
            commands::pick_file,
            commands::app_info,
            commands::app_open_dir,
            commands::app_log,
            commands::app_open_path,
            // hosts
            commands::hosts::hosts_info,
            commands::hosts::hosts_save,
            commands::hosts::hosts_backups,
            commands::hosts::hosts_restore,
            commands::hosts::hosts_reveal,
            commands::hosts::hosts_open_backup_dir,
            // 截图
            commands::capture::capture_start,
            commands::capture::capture_frame,
            commands::capture::capture_windows,
            commands::capture::capture_element,
            commands::capture::cursor_pos,
            commands::capture::capture_commit,
            commands::capture::capture_pin,
            commands::capture::capture_ocr,
            commands::capture::capture_cancel,
            commands::capture::capture_pending,
            commands::capture::capture_scroll,
            commands::capture::capture_abort_scroll,
            commands::capture::capture_close_editor,
            // 图片
            commands::image::image_save,
            commands::image::image_load_file,
            commands::image::image_save_as,
            // 录屏
            commands::record::record_start,
            commands::record::record_start_region,
            commands::record::record_pick_region,
            commands::record::record_start_fullscreen,
            commands::record::record_begin,
            commands::record::record_pause,
            commands::record::record_resume,
            commands::record::record_toggle_pause,
            commands::record::record_stop,
            commands::record::record_cancel,
            commands::record::record_status,
            commands::record::record_camera_list,
            commands::record::record_recent,
            commands::record::record_set_lens,
            commands::record::record_set_interactive,
            commands::image::image_copy,
            commands::image::image_pin,
            commands::image::image_ocr,
            commands::image::translate_text,
            commands::image::translate_test,
            commands::image::ocr_close,
            commands::image::ocr_take_image,
            commands::image::image_ocr_languages,
            commands::image::ocr_engines,
            commands::image::ocr_open_engine_dir,
            commands::image::ocr_download_paddle,
            commands::image::ocr_install_local,
            commands::image::image_info,
            commands::image::image_open_folder,

            // 图标提取
            commands::icons::icon_extract,
            commands::icons::icon_build_ico,
            // 剪贴板
            commands::clipboard::clipboard_list,
            commands::clipboard::clipboard_thumb,
            commands::clipboard::clipboard_image_data,
            commands::clipboard::clipboard_stats,
            commands::clipboard::clipboard_paste,
            commands::clipboard::clipboard_toggle_pin,
            commands::clipboard::clipboard_remove,
            commands::clipboard::clipboard_clear,
            commands::clipboard::clipboard_copy_text,
            commands::clipboard::clipboard_open_file,
            commands::clipboard::clipboard_pin,
            commands::clipboard::clipboard_pin_current,
            commands::clipboard::clipboard_system_history_get,
            commands::clipboard::clipboard_system_history_set,
            commands::clipboard::clipboard_set_takeover_win_v,
            commands::clipboard::clipboard_takeover_win_v_status,
            // 剪贴板悬浮面板
            commands::clipboard::clipboard_panel_paste,
            commands::clipboard::secrets_list,
            commands::clipboard::secrets_save,
            commands::clipboard::secrets_rename,
            commands::clipboard::secrets_remove,
            commands::clipboard::secret_paste,
            commands::clipboard::clipboard_panel_hide,
            commands::clipboard::clipboard_panel_ready,
            commands::clipboard::clipboard_panel_target,
            // 图床
            commands::upload::upload_providers,
            commands::upload::upload_provider_save,
            commands::upload::upload_set_active,
            commands::upload::upload_image,
            commands::upload::upload_test,
            commands::upload::upload_list_dirs,
            commands::upload::upload_list_images,
            commands::upload::upload_thumb,
            commands::upload::upload_history_list,
            commands::upload::upload_history_remove,
            commands::upload::upload_history_clear,
            // 插件
            commands::plugins::plugins_state,
            commands::plugins::plugins_toggle,
            commands::plugins::plugins_reload,
            commands::plugins::plugins_hooks,
            commands::plugins::plugins_open_dir,
            // 主密码保险箱
            commands::vault::vault_status,
            commands::vault::vault_enable,
            commands::vault::vault_unlock,
            commands::vault::vault_lock,
            commands::vault::vault_change_password,
            commands::vault::vault_disable,
            commands::vault::vault_reset,
        ])
        .run(tauri::generate_context!())
        .expect("SnipForge 启动失败");
}
