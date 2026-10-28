use crate::config::AppSettings;
use crate::error::{AppError, AppResult};
use crate::paths::Paths;
use crate::state::AppState;
use crate::types::AppSettingsView;

/// 打开主窗口并跳到指定页面（剪贴板面板上的"设置"按钮用它）
#[tauri::command]
pub fn app_show_main(app: tauri::AppHandle, route: Option<String>) {
    crate::windows::show_main(&app, route.as_deref());
}

/// 读取完整配置
#[tauri::command]
pub fn settings_get(state: tauri::State<'_, std::sync::Arc<AppState>>) -> AppResult<AppSettingsView> {
    Ok(AppSettingsView {
        settings: state.settings_snapshot(),
    })
}

/// 局部更新配置。前端只提交改动的字段，后端做深度合并后落盘。
#[tauri::command]
pub fn settings_patch(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    patch: serde_json::Value,
) -> AppResult<AppSettingsView> {
    // 快捷键是"注册到系统"的东西，光改配置不重新注册等于没改：
    // 以前只有启动时注册一遍，界面却写着"修改后立即生效"，
    // 用户改完按下去毫无反应，只能重启应用 —— 这是最典型的"设置不生效"。
    let hotkeys_changed = patch.get("hotkeys").is_some();
    {
        let mut guard = state.settings.write();
        guard.apply_patch(&patch);
    }
    state.persist_settings()?;

    if hotkeys_changed {
        if let Err(e) = crate::shortcuts::register_all(&app, state.inner()) {
            log::warn!("快捷键重新注册失败: {e}");
        }
    }

    Ok(AppSettingsView {
        settings: state.settings_snapshot(),
    })
}

/// 恢复默认配置（保留 provider 凭据，避免用户手滑把 token 清了）
#[tauri::command]
pub fn settings_reset(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Arc<AppState>>,
) -> AppResult<AppSettingsView> {
    {
        let mut guard = state.settings.write();
        let providers = guard.uploader.providers.clone();
        let plugin_enabled = guard.plugin_enabled.clone();
        let mut fresh = AppSettings::default();
        fresh.uploader.providers = providers;
        fresh.plugin_enabled = plugin_enabled;
        *guard = fresh;
    }
    state.persist_settings()?;
    // 快捷键回到默认值，同样要重新注册
    if let Err(e) = crate::shortcuts::register_all(&app, state.inner()) {
        log::warn!("恢复默认后重新注册快捷键失败: {e}");
    }
    Ok(AppSettingsView {
        settings: state.settings_snapshot(),
    })
}

/// 选择目录（走系统对话框）
#[tauri::command]
pub async fn settings_pick_dir(
    app: tauri::AppHandle,
    default_path: Option<String>,
) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;
    let mut dialog = app.dialog().file().set_title("选择目录");
    if let Some(p) = default_path.filter(|s| !s.is_empty()) {
        dialog = dialog.set_directory(p);
    }
    let picked = dialog.blocking_pick_folder();
    match picked {
        Some(path) => Ok(Some(path.to_string())),
        None => Ok(None),
    }
}

/// 应用信息（版本、数据目录、OCR 语言、快捷键注册状态）
#[tauri::command]
pub fn app_info(state: tauri::State<'_, std::sync::Arc<AppState>>) -> AppResult<crate::types::AppInfo> {
    Ok(crate::types::AppInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        platform: std::env::consts::OS.to_string(),
        data_dir: state.paths.data.to_string_lossy().to_string(),
        ocr_languages: crate::ocr::available_languages(),
        hotkey_results: state.hotkey_results.read().clone(),
    })
}

/// 在资源管理器里打开目录
#[tauri::command]
pub fn app_open_dir(app: tauri::AppHandle, path: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| AppError::Msg(e.to_string()))
}

/// 用默认程序打开文件
#[tauri::command]
pub fn app_open_path(app: tauri::AppHandle, path: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_path(path, None::<&str>)
        .map_err(|e| AppError::Msg(e.to_string()))
}

/// 目标保存目录的最终取值（用户没设就用图片目录/SnipForge）
pub fn resolve_save_dir(state: &AppState) -> std::path::PathBuf {
    let cfg = state.settings.read();
    if cfg.save.dir.trim().is_empty() {
        state.paths.pictures.clone()
    } else {
        std::path::PathBuf::from(&cfg.save.dir)
    }
}

/// 首次启动时确保目录存在
pub fn ensure_paths(paths: &Paths) -> AppResult<()> {
    paths.ensure()?;
    Ok(())
}

/// 前端把错误写到同一份日志里。
///
/// WebView 里的 console 在生产包里看不到，出问题时用户只能说"点了没反应"。
/// 让前端把异常转发到日志文件，排查成本从"猜"降到"看"。
#[tauri::command]
pub fn app_log(level: String, message: String) {
    match level.as_str() {
        "error" => log::error!("[前端] {message}"),
        "warn" => log::warn!("[前端] {message}"),
        _ => log::info!("[前端] {message}"),
    }
}
