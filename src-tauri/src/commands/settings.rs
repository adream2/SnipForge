use crate::config::AppSettings;
use crate::error::{AppError, AppResult};
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

/// 配置落盘后的运行时副作用，统一在这一个函数里做。
///
/// 以前 `settings_patch` 和 `settings_reset` 各写一份，于是 reset 漏了
/// 手势规则、托盘勾选、开机自启注册表三样 —— 用户点完"恢复默认"，
/// 界面变了、行为没变，要重启才对齐。副作用只有一个入口就不会再漏。
fn apply_runtime(
    app: &tauri::AppHandle,
    state: &std::sync::Arc<AppState>,
    patch: &serde_json::Value,
) -> AppResult<()> {
    // 快捷键是"注册到系统"的东西，光改配置不重新注册等于没改
    if patch.get("hotkeys").is_some() {
        if let Err(e) = crate::shortcuts::register_all(app, state) {
            log::warn!("快捷键重新注册失败: {e}");
        }
        // 托盘菜单里带着快捷键文案，必须一起刷新
        crate::tray::refresh_menu(app, state);
    }

    // 手势钩子常驻，只换规则，不用重装
    if patch.get("gestures").is_some_and(|g| !g.is_null()) {
        crate::gesture::update(state.settings.read().gestures.clone());
        crate::tray::refresh_menu(app, state);
        let _ = crate::windows::emit_to_main(app, "app://gesture-changed", serde_json::json!({}));
        log::debug!("手势配置已更新");
    }

    // 开机自启不是"存个 bool 就行"：得真去写 HKCU 的 Run 项
    if let Some(on) = patch
        .pointer("/general/launchAtLogin")
        .and_then(|v| v.as_bool())
    {
        crate::autostart::apply(on)?;
    }

    // 插件总开关变了要重新扫描目录，否则关了还挂着一堆 provider
    if patch.get("pluginEnabled").is_some() || patch.pointer("/general/enablePlugins").is_some() {
        crate::plugins::reload(app, state);
    }

    Ok(())
}

/// 局部更新配置。前端只提交改动的字段，后端做深度合并后落盘。
#[tauri::command]
pub fn settings_patch(
    app: tauri::AppHandle,
    state: tauri::State<'_, std::sync::Arc<AppState>>,
    patch: serde_json::Value,
) -> AppResult<AppSettingsView> {
    // 锁着时凭据字段会被落盘逻辑清空，直接拒绝比"保存成功但值没了"诚实
    if state.vault.is_enabled() && !state.vault.is_unlocked() {
        let touches_secret = patch.pointer("/translate/deeplKey").is_some()
            || patch.pointer("/translate/customHeaders").is_some()
            || patch.pointer("/uploader/providers").is_some();
        if touches_secret {
            return Err(AppError::Msg(
                "这些配置已加密锁定，请先在「设置 → 安全」里解锁".into(),
            ));
        }
    }
    {
        let mut guard = state.settings.write();
        // 合并失败必须原样报错：以前是 if let Ok 静默丢弃，
        // 前端拿到旧配置却以为保存成功（"改了没反应"最难查的一种）
        guard.apply_patch(&patch).map_err(AppError::Msg)?;
    }
    state.persist_settings()?;
    apply_runtime(&app, state.inner(), &patch)?;

    Ok(AppSettingsView {
        settings: state.settings_snapshot(),
    })
}

/// 手势总开关。托盘菜单、设置页共用这一条路径 ——
/// 各自写一遍的话，早晚出现"页面说开了、托盘还停着"。
pub fn set_gestures_enabled(
    app: &tauri::AppHandle,
    state: &std::sync::Arc<AppState>,
    on: bool,
) -> AppResult<bool> {
    {
        let mut g = state.settings.write();
        g.gestures.enabled = on;
    }
    state.persist_settings()?;
    // 钩子常驻，这里只换规则：不需要重装，下一次划就按新状态走
    crate::gesture::update(state.settings.read().gestures.clone());
    crate::tray::refresh_menu(app, state);
    crate::windows::emit_to_main(app, "app://gesture-changed", serde_json::json!({}));
    log::info!("鼠标手势已{}", if on { "启用" } else { "暂停" });
    Ok(on)
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
    // 恢复默认也要走一遍运行时副作用，而且**不止快捷键**：
    // 手势规则、托盘勾选、开机自启注册表、插件加载都得跟着回到默认。
    let everything = serde_json::json!({
        "hotkeys": {},
        "gestures": {},
        "general": { "launchAtLogin": false, "enablePlugins": true },
        "pluginEnabled": {}
    });
    apply_runtime(&app, state.inner(), &everything)?;

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

/// 选择文件（走系统对话框）
#[tauri::command]
pub async fn pick_file(
    app: tauri::AppHandle,
    title: Option<String>,
    extensions: Option<Vec<String>>,
) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;

    let mut dialog = app.dialog().file();
    if let Some(t) = title.filter(|s| !s.is_empty()) {
        dialog = dialog.set_title(t);
    }
    let exts: Vec<String> = extensions.unwrap_or_default();
    if !exts.is_empty() {
        let refs: Vec<&str> = exts.iter().map(|s| s.as_str()).collect();
        dialog = dialog.add_filter("文件", &refs);
    }
    match dialog.blocking_pick_file() {
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

/// 注册表里现在是否真的登记了开机自启。
///
/// 设置页拿它和开关值对比：配置说开着、注册表却没有，就说明写失败了
/// （或者被安全软件清掉了），不该让界面假装一切正常。
#[tauri::command]
pub fn autostart_status() -> bool {
    crate::autostart::is_enabled()
}

/// 把一笔轨迹翻译成方向串（页面「划一个」录制框用）。
///
/// 和钩子共用同一套 8 方向判定 + 短段过滤 —— 前端自己再写一套，
/// 早晚出现"这里录得出来、真划没反应"。
#[tauri::command]
pub fn gesture_trace(points: Vec<[i32; 2]>) -> String {
    let pts: Vec<(i32, i32)> = points.iter().map(|p| (p[0], p[1])).collect();
    crate::gesture::trace_dir(&pts)
}

/// 页面「试划」用：这一划会命中哪一条绑定（返回下标）。
///
/// 走和真手势完全相同的匹配逻辑（含容错），这里试出什么，真划就是什么。
#[tauri::command]
pub fn gesture_match(pattern: String, profile: Option<String>) -> Option<usize> {
    crate::gesture::match_scoped(profile.as_deref(), &pattern)
}
