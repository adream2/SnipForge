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

    // 手势配置改了要立刻生效（钩子常驻，只换规则，不用重装）
    if let Some(g) = patch.get("gestures") {
        if !g.is_null() {
            let cfg = state.settings.read().gestures.clone();
            crate::gesture::update(cfg);
            // 托盘的"鼠标手势"勾选状态跟着走，不然页面里暂停了、托盘还打着勾
            crate::tray::refresh_menu(&app, state.inner());
            log::debug!("手势配置已更新（{} 条绑定）", {
                let c = state.settings.read().gestures.clone();
                c.bindings.len()
            });
        }
    }

    // 开机自启不是"存个 bool 就行"：得真去写 HKCU 的 Run 项。
    // 以前只改了配置，开关是纯装饰的，重启电脑什么都不会发生。
    if let Some(on) = patch
        .get("general")
        .and_then(|g| g.get("launchAtLogin"))
        .and_then(|v| v.as_bool())
    {
        crate::autostart::apply(on)?;
    }

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

/// 注册表里现在是否真的登记了开机自启。
///
/// 设置页拿它和开关值对比：配置说开着、注册表却没有，就说明写失败了
/// （或者被安全软件清掉了），不该让界面假装一切正常。
#[tauri::command]
pub fn autostart_status() -> bool {
    crate::autostart::is_enabled()
}

/// 识别一笔轨迹是不是形状手势（页面里"划一个"录制框用）。
///
/// 刻意走后端：形状识别只有一份实现，录制框认出来的和真手势必须完全一致 ——
/// 前端再写一套，早晚会出现"这里认得出、实际划就没反应"。
#[tauri::command]
pub fn gesture_recognize(points: Vec<[i32; 2]>) -> Option<String> {
    let pts: Vec<(i32, i32)> = points.iter().map(|p| (p[0], p[1])).collect();
    crate::win32::shape::recognize(&pts).map(|c| c.to_string())
}

/// 形状识别的诊断行：每个内置形状的"距离比"（1.0 = 正好卡在阈值上）。
///
/// 给 `tests/shape_probe.py` 和排查"为什么画 H 没反应"用 —— 只有数字
/// 才能说明是尺寸不够、还是画得不像，靠"再试一次"永远猜不出来。
#[tauri::command]
pub fn gesture_diagnose(points: Vec<[i32; 2]>) -> String {
    let pts: Vec<(i32, i32)> = points.iter().map(|p| (p[0], p[1])).collect();
    crate::win32::shape::diagnose(&pts)
}

/// 把用户画的一笔规范化成手势模板（32 点，等比归一化）。
///
/// 和真手势共用同一份实现：前端自己再算一遍，迟早出现
/// "录的时候认得出、划的时候不像"。
#[tauri::command]
pub fn gesture_normalize(points: Vec<[i32; 2]>) -> Vec<[f32; 2]> {
    let pts: Vec<(i32, i32)> = points.iter().map(|p| (p[0], p[1])).collect();
    crate::win32::shape::canonical_stroke(&pts)
}

/// 页面"试划"用：这一划会命中哪一条绑定（返回下标）。
///
/// 走的是和真手势完全相同的匹配顺序（自定义笔画优先，再方向串），
/// 所以这里试出来是什么，真划就是什么。
#[tauri::command]
pub fn gesture_match(
    points: Vec<[i32; 2]>,
    pattern: String,
    profile: Option<String>,
) -> Option<usize> {
    let pts: Vec<(i32, i32)> = points.iter().map(|p| (p[0], p[1])).collect();
    crate::gesture::match_scoped(profile.as_deref(), &pts, &pattern)
}
