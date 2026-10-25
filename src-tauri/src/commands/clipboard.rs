use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::types::{ClipboardItem, ClipboardQuery, ClipboardStats};
use std::sync::Arc;
use tauri::Manager;

fn service(app: &tauri::AppHandle) -> AppResult<Arc<crate::clipboard::ClipboardService>> {
    app.try_state::<Arc<crate::clipboard::ClipboardService>>()
        .map(|s| s.inner().clone())
        .ok_or_else(|| AppError::Msg("剪贴板服务未初始化".into()))
}

#[tauri::command]
pub fn clipboard_list(
    app: tauri::AppHandle,
    query: Option<ClipboardQuery>,
) -> AppResult<Vec<ClipboardItem>> {
    let svc = service(&app)?;
    let items = crate::clipboard::list(&svc, query.unwrap_or_default());
    // 面板打开时列表首项是谁，直接决定回车粘什么。
    // 这条日志是排查「粘错条目」唯一的抓手，留着。
    if let Some(first) = items.first() {
        log::debug!(
            "列表返回 {} 条，首项 id={} 内容={:?}",
            items.len(),
            first.id,
            first
                .text
                .as_deref()
                .unwrap_or("")
                .chars()
                .take(20)
                .collect::<String>()
        );
    }
    Ok(items)
}

#[tauri::command]
pub fn clipboard_thumb(app: tauri::AppHandle, id: String) -> AppResult<Option<String>> {
    let svc = service(&app)?;
    Ok(crate::clipboard::thumbnail_data_url(&svc, &id))
}

#[tauri::command]
pub fn clipboard_stats(app: tauri::AppHandle) -> AppResult<ClipboardStats> {
    let svc = service(&app)?;
    Ok(crate::clipboard::stats(&svc))
}

#[tauri::command]
pub fn clipboard_paste(app: tauri::AppHandle, id: String, auto_paste: Option<bool>) -> AppResult<String> {
    let svc = service(&app)?;
    let kind = crate::clipboard::paste(&svc, &id)?;

    // 可选：直接模拟 Ctrl+V 到前台窗口（需要在隐藏主窗口之后）
    if auto_paste.unwrap_or(false) {
        if let Some(win) = app.get_webview_window("main") {
            let _ = win.hide();
        }
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(160));
            if let Err(e) = crate::win32::input::paste_hotkey() {
                log::warn!("模拟粘贴失败: {e}");
            }
        });
    }
    Ok(kind)
}

#[tauri::command]
pub fn clipboard_toggle_pin(app: tauri::AppHandle, id: String) -> AppResult<Option<ClipboardItem>> {
    let svc = service(&app)?;
    let item = svc.store.toggle_pin(&id);
    let _ = svc.store.save();
    Ok(item)
}

#[tauri::command]
pub fn clipboard_remove(app: tauri::AppHandle, id: String) -> AppResult<bool> {
    let svc = service(&app)?;
    let ok = svc.store.remove(&id);
    let _ = svc.store.save();
    Ok(ok)
}

#[tauri::command]
pub fn clipboard_clear(app: tauri::AppHandle, keep_pinned: bool) -> AppResult<()> {
    let svc = service(&app)?;
    svc.store.clear(keep_pinned);
    let _ = svc.store.save();
    Ok(())
}

#[tauri::command]
pub fn clipboard_copy_text(app: tauri::AppHandle, text: String) -> AppResult<()> {
    let svc = service(&app)?;
    crate::clipboard::copy_text(&svc, &text)
}

#[tauri::command]
pub fn clipboard_open_file(app: tauri::AppHandle, id: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let svc = service(&app)?;
    let item = svc
        .store
        .find_by_id(&id)
        .ok_or_else(|| AppError::Msg("条目不存在".into()))?;

    match item.kind.as_str() {
        "image" => {
            let path = svc
                .store
                .blob_path(&item)
                .ok_or_else(|| AppError::Msg("图片文件已丢失".into()))?;
            app.opener()
                .open_path(path.to_string_lossy().to_string(), None::<&str>)
                .map_err(|e| AppError::Msg(e.to_string()))
        }
        "files" => {
            let files = item.files.unwrap_or_default();
            if files.is_empty() {
                return Err(AppError::Msg("没有可打开的文件".into()));
            }
            for f in files {
                let _ = app.opener().open_path(f, None::<&str>);
            }
            Ok(())
        }
        _ => Err(AppError::Msg("该类型没有可打开的文件".into())),
    }
}

/// 贴图：把剪贴板里的图片钉在屏幕上
#[tauri::command]
pub async fn clipboard_pin(app: tauri::AppHandle, id: String) -> AppResult<()> {
    let state = app
        .try_state::<Arc<AppState>>()
        .ok_or_else(|| AppError::Msg("状态未初始化".into()))?;
    let svc = service(&app)?;
    let item = svc
        .store
        .find_by_id(&id)
        .ok_or_else(|| AppError::Msg("条目不存在".into()))?;
    let path = svc
        .store
        .blob_path(&item)
        .ok_or_else(|| AppError::Msg("图片文件已丢失".into()))?;

    let bytes = std::fs::read(path)?;
    use base64::Engine;
    let data_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );

    let width = item.width.unwrap_or(800);
    let height = item.height.unwrap_or(600);
    let opacity = state.settings.read().editor.pin_opacity as f64;
    crate::windows::open_pin(&app, data_url, width, height, opacity)
}


/* ------------------ 接管系统剪贴板（Win+V） ------------------ */

/// Win+V 的系统剪贴板历史开关。
///
/// 为什么不是"直接注册 Win+V"？因为 Windows 把它当系统保留热键，
/// 第三方进程用 RegisterHotKey 注册一律失败。能做到的真正"接管"是：
/// 关掉系统自带的历史面板（Win+V 变成一个空弹窗/无反应），
/// 让用户改用 SnipForge 自己的面板快捷键。
const CLIP_KEY: &str = r"HKCU\Software\Microsoft\Clipboard";
const CLIP_VALUE: &str = "EnableClipboardHistory";

#[cfg(windows)]
fn reg_run(args: &[&str]) -> Result<(bool, String), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut cmd = std::process::Command::new("reg");
    cmd.args(args);
    cmd.creation_flags(CREATE_NO_WINDOW);
    match cmd.output() {
        Ok(out) => Ok((
            out.status.success(),
            String::from_utf8_lossy(&out.stdout).to_string(),
        )),
        Err(e) => Err(e.to_string()),
    }
}

/// 读取系统剪贴板历史是否开启（默认是开启的）
#[tauri::command]
pub fn clipboard_system_history_get() -> AppResult<bool> {
    #[cfg(windows)]
    {
        let (ok, stdout) = reg_run(&["query", CLIP_KEY, "/v", CLIP_VALUE])
            .map_err(|e| AppError::Msg(format!("读取注册表失败: {e}")))?;
        if !ok {
            // 键不存在 = 系统默认（开启）
            return Ok(true);
        }
        // 输出形如：    EnableClipboardHistory    REG_DWORD    0x1
        Ok(!stdout.contains("0x0"))
    }
    #[cfg(not(windows))]
    {
        Ok(false)
    }
}

/// 开/关系统剪贴板历史
#[tauri::command]
pub fn clipboard_system_history_set(enabled: bool) -> AppResult<bool> {
    #[cfg(windows)]
    {
        let value = if enabled { "1" } else { "0" };
        let (ok, _) =
            reg_run(&["add", CLIP_KEY, "/v", CLIP_VALUE, "/t", "REG_DWORD", "/d", value, "/f"])
                .map_err(|e| AppError::Msg(format!("写入注册表失败: {e}")))?;
        if !ok {
            return Err(AppError::Msg("写入注册表失败（可能被安全软件拦截）".into()));
        }
        Ok(enabled)
    }
    #[cfg(not(windows))]
    {
        let _ = enabled;
        Ok(false)
    }
}


/// 接管 Win+V：用键盘钩子截住 Win+V，弹本工具的面板。
///
/// 注意这跟上面的注册表开关是两件事：
///   - 这个靠 WH_KEYBOARD_LL，立即生效，不用重启 explorer；
///   - 注册表那个只是让系统别再存一份历史（省内存、避免重复）。
#[tauri::command]
pub fn clipboard_set_takeover_win_v(
    state: tauri::State<'_, Arc<AppState>>,
    enabled: bool,
) -> AppResult<bool> {
    crate::win32::keyhook::set_active(enabled);
    {
        let mut cfg = state.settings.write();
        cfg.clipboard.takeover_win_v = enabled;
    }
    state.persist_settings()?;
    Ok(enabled)
}

#[tauri::command]
pub fn clipboard_takeover_win_v_status() -> AppResult<bool> {
    Ok(crate::win32::keyhook::is_active())
}

/* ------------------ 悬浮面板（Win+V / Alt+V） ------------------ */

/// 面板里选中一条：写进系统剪贴板 → 收起面板 → 焦点还给原窗口 → 模拟 Ctrl+V。
///
/// `copy_only` 为 true 时只写剪贴板不粘贴（面板里的 Shift+Enter / 中键）。
#[tauri::command]
pub fn clipboard_panel_paste(
    app: tauri::AppHandle,
    id: String,
    copy_only: Option<bool>,
) -> AppResult<String> {
    crate::panel::paste_item(&app, &id, copy_only.unwrap_or(false))
}

#[tauri::command]
pub fn clipboard_panel_hide(app: tauri::AppHandle) -> AppResult<()> {
    crate::panel::hide(&app);
    Ok(())
}

/// 面板把列表加载完了。
///
/// 前端只在这时候调，作用是解除「预热」：预热期间键盘钩子接住的回车，
/// 会在这里补做成一次粘贴。`top_id` 传前端此刻列表的第一条，
/// 保证补做的那次粘贴和用户看到的条目一致。
#[tauri::command]
pub fn clipboard_panel_ready(app: tauri::AppHandle, top_id: Option<String>) -> AppResult<()> {
    crate::panel::mark_ready(&app, top_id);
    Ok(())
}

/// 记住的目标窗口进程名，面板显示「粘贴到 记事本」之类的提示
#[tauri::command]
pub fn clipboard_panel_target() -> AppResult<String> {
    Ok(crate::panel::target_app())
}

/// 把系统剪贴板里的图片直接钉在屏幕上（Alt+P）。
///
/// 这是"看图更快"的一条捷径：从任何地方复制一张图，按 Alt+P 就贴在屏幕上，
/// 不用先截图再截回来看。
#[tauri::command]
pub async fn clipboard_pin_current(app: tauri::AppHandle) -> AppResult<()> {
    use base64::Engine;

    let state = app
        .try_state::<Arc<AppState>>()
        .ok_or_else(|| AppError::Msg("状态未初始化".into()))?;

    let mut cb = arboard::Clipboard::new().map_err(|e| AppError::Clipboard(e.to_string()))?;
    let img = cb
        .get_image()
        .map_err(|e| AppError::Clipboard(format!("剪贴板里没有图片：{e}")))?;

    let rgba = image::RgbaImage::from_raw(
        img.width as u32,
        img.height as u32,
        img.bytes.into_owned(),
    )
    .ok_or_else(|| AppError::Msg("图片数据不完整".into()))?;

    let mut png: Vec<u8> = Vec::new();
    image::DynamicImage::ImageRgba8(rgba)
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .map_err(|e| AppError::Msg(format!("图片编码失败: {e}")))?;

    let data_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&png)
    );
    let opacity = state.settings.read().editor.pin_opacity as f64;
    crate::windows::open_pin(&app, data_url, img.width as u32, img.height as u32, opacity)
}
