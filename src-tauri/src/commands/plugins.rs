use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::types::PluginState;
use std::sync::Arc;

#[tauri::command]
pub fn plugins_state(state: tauri::State<'_, Arc<AppState>>) -> AppResult<PluginState> {
    Ok(crate::plugins::state_of(&state))
}

#[tauri::command]
pub fn plugins_toggle(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
    enabled: bool,
) -> AppResult<PluginState> {
    {
        let mut cfg = state.settings.write();
        cfg.plugin_enabled.insert(id.clone(), enabled);
    }
    state.persist_settings()?;
    Ok(crate::plugins::reload(&app, state.inner()))
}

#[tauri::command]
pub fn plugins_reload(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> AppResult<PluginState> {
    Ok(crate::plugins::reload(&app, state.inner()))
}

/// 当前有哪些钩子点、各挂了几个处理器。
///
/// 插件页用它展示"扩展点清单"：以前钩子点是写死在文档里的一串字符串，
/// 早就和代码对不上了；现在从注册表读，加一个点页面自动多一行。
#[tauri::command]
pub fn plugins_hooks(state: tauri::State<'_, Arc<AppState>>) -> AppResult<Vec<crate::hooks::HookPoint>> {
    Ok(state.hooks.points())
}

#[tauri::command]
pub fn plugins_open_dir(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    which: Option<String>,
) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let dir = match which.as_deref() {
        Some("builtin") => state.paths.builtin_plugins.clone(),
        _ => state.paths.plugins.clone(),
    };
    std::fs::create_dir_all(&dir)?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| AppError::Msg(e.to_string()))
}
