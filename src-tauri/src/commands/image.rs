use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::types::{ImageFileView, ImageInfoView, OcrResult, SaveOutcome};
use crate::uploader::{data_url_to_bytes, mime_to_ext, render_file_name};
use std::sync::Arc;
use tauri::Manager;

/// 保存图片（自动生成文件名）
#[tauri::command]
pub fn image_save(
    state: tauri::State<'_, Arc<AppState>>,
    data_url: String,
    file_name: Option<String>,
    open_folder: Option<bool>,
) -> AppResult<SaveOutcome> {
    let (bytes, mime) = data_url_to_bytes(&data_url)?;
    let ext = mime_to_ext(&mime);

    let cfg = state.settings.read().clone();
    let dir = if cfg.save.dir.trim().is_empty() {
        state.paths.pictures.clone()
    } else {
        std::path::PathBuf::from(&cfg.save.dir)
    };
    std::fs::create_dir_all(&dir)?;

    let name = match file_name {
        Some(n) if !n.trim().is_empty() => n,
        _ => format!("{}.{}", render_file_name(&cfg.save.name_template, ext), ext),
    };
    let path = dir.join(&name);
    std::fs::write(&path, &bytes)?;

    if open_folder.unwrap_or(cfg.save.after_save == "open-folder") {
        let _ = crate::commands::app_open_path_inner(&path);
    }

    Ok(SaveOutcome {
        path: path.to_string_lossy().to_string(),
        bytes: bytes.len() as u64,
        format: ext.to_string(),
    })
}

/// 另存为（弹系统对话框）
#[tauri::command]
pub async fn image_save_as(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    data_url: String,
    suggested: Option<String>,
) -> AppResult<Option<String>> {
    use tauri_plugin_dialog::DialogExt;

    let (bytes, mime) = data_url_to_bytes(&data_url)?;
    let ext = mime_to_ext(&mime);
    let cfg = state.settings.read().clone();
    let default_name = suggested.unwrap_or_else(|| {
        format!("{}.{}", render_file_name(&cfg.save.name_template, ext), ext)
    });

    let picked = app
        .dialog()
        .file()
        .set_title("保存图片")
        .set_file_name(&default_name)
        .add_filter(ext, &[ext])
        .blocking_save_file();

    match picked {
        Some(path) => {
            let p = path.to_string();
            std::fs::write(&p, &bytes)?;
            Ok(Some(p))
        }
        None => Ok(None),
    }
}

/// 复制到剪贴板
#[tauri::command]
pub fn image_copy(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    data_url: String,
) -> AppResult<()> {
    let (bytes, _) = data_url_to_bytes(&data_url)?;
    let service = app
        .try_state::<Arc<crate::clipboard::ClipboardService>>()
        .ok_or_else(|| AppError::Msg("剪贴板服务未初始化".into()))?;
    crate::clipboard::copy_image_bytes(&service, &bytes)
}

/// OCR 识别（按当前配置选择引擎）
#[tauri::command]
pub async fn image_ocr(
    state: tauri::State<'_, Arc<AppState>>,
    data_url: String,
) -> AppResult<OcrResult> {
    let cfg = state.settings.read().ocr.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (bytes, _) = data_url_to_bytes(&data_url)?;
        crate::ocr::recognize(&bytes, &cfg)
    })
    .await
    .map_err(|e| AppError::Msg(format!("OCR 任务失败: {e}")))?
}

#[tauri::command]
pub fn image_ocr_languages(state: tauri::State<'_, Arc<AppState>>) -> AppResult<Vec<String>> {
    Ok(crate::ocr::engines(&state.settings.read().ocr.paddle_path)
        .into_iter()
        .find(|e| e.builtin)
        .map(|e| e.languages)
        .unwrap_or_default())
}

/// 列出可用 OCR 引擎及实时状态
#[tauri::command]
pub fn ocr_engines(
    state: tauri::State<'_, Arc<AppState>>,
) -> AppResult<Vec<crate::ocr::EngineInfo>> {
    Ok(crate::ocr::engines(&state.settings.read().ocr.paddle_path))
}

/// 打开 OCR 引擎目录（用户把 PaddleOCR-json 解压到这里即可）
#[tauri::command]
pub fn ocr_open_engine_dir(app: tauri::AppHandle, state: tauri::State<'_, Arc<AppState>>) -> AppResult<String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = state
        .paths
        .data
        .join("ocr-engines");
    std::fs::create_dir_all(&dir)?;
    app.opener()
        .open_path(dir.to_string_lossy().to_string(), None::<&str>)
        .map_err(|e| AppError::Msg(e.to_string()))?;
    Ok(dir.to_string_lossy().to_string())
}

/// 图片基本信息
#[tauri::command]
pub fn image_info(data_url: String) -> AppResult<ImageInfoView> {
    let (bytes, mime) = data_url_to_bytes(&data_url)?;
    let (width, height) = decode_size(&bytes, &mime)?;
    Ok(ImageInfoView {
        width,
        height,
        format: mime,
        bytes: bytes.len() as u64,
    })
}

/// 按魔数认格式。前端拖进来的文件只有字节，没有可靠的 mime。
fn sniff_mime(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else if bytes.len() > 12 && &bytes[0..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "image/webp"
    } else if bytes.starts_with(b"GIF8") {
        "image/gif"
    } else if bytes.starts_with(b"BM") {
        "image/bmp"
    } else {
        "application/octet-stream"
    }
}

/// 取图片尺寸。GIF 交给 gif 解码器（image crate 没开 gif feature，解码不了，
/// 但预览要靠浏览器直接把原图显示出来，所以只补尺寸这一件事）。
fn decode_size(bytes: &[u8], mime: &str) -> AppResult<(u32, u32)> {
    if mime == "image/gif" || bytes.starts_with(b"GIF8") {
        let mut opts = gif::DecodeOptions::new();
        opts.set_color_output(gif::ColorOutput::RGBA);
        let dec = opts
            .read_info(std::io::Cursor::new(bytes))
            .map_err(|e| AppError::Msg(format!("GIF 解析失败: {e}")))?;
        return Ok((dec.width() as u32, dec.height() as u32));
    }
    let img = image::load_from_memory(bytes)
        .map_err(|e| AppError::Msg(format!("图片解码失败: {e}")))?;
    Ok((img.width(), img.height()))
}

/// 把磁盘上的图片读成 data URL 交给图片工具。
///
/// 走后端读文件而不是 webview 直接 fetch `asset://`：后者要么被 CSP 拦、
/// 要么得给 asset 协议开任意路径 —— 都不如一个明确的命令干净。
#[tauri::command]
pub fn image_load_file(path: String) -> AppResult<ImageFileView> {
    use base64::Engine;

    const MAX_BYTES: u64 = 128 * 1024 * 1024;
    let p = std::path::PathBuf::from(&path);
    let meta = std::fs::metadata(&p).map_err(|e| AppError::Msg(format!("读不到文件: {e}")))?;
    if !meta.is_file() {
        return Err(AppError::Msg("不是文件".into()));
    }
    if meta.len() > MAX_BYTES {
        return Err(AppError::Msg("图片超过 128MB，先压缩一下吧".into()));
    }

    let bytes = std::fs::read(&p)?;
    let mime = sniff_mime(&bytes);
    if mime == "application/octet-stream" {
        return Err(AppError::Msg("认不出这个格式（支持 PNG / JPEG / WebP / BMP / GIF）".into()));
    }
    let (width, height) = decode_size(&bytes, mime)?;
    let data_url = format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );
    Ok(ImageFileView {
        data_url,
        width,
        height,
        format: mime.to_string(),
        bytes: bytes.len() as u64,
    })
}

/// 打开图片所在目录
#[tauri::command]
pub async fn image_open_folder(app: tauri::AppHandle, path: String) -> AppResult<()> {
    use tauri_plugin_opener::OpenerExt;
    let parent = std::path::Path::new(&path)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or(path);
    app.opener()
        .open_path(parent, None::<&str>)
        .map_err(|e| AppError::Msg(e.to_string()))
}


/// 一键下载并安装 PaddleOCR。
///
/// 官方包是 88MB 的 7z，普通用户没法自己解 —— 所以托管掉整个流程：
/// 查最新版本 → 下载（带进度）→ 解压 → 自动设为当前引擎。
#[tauri::command]
pub async fn ocr_download_paddle(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
) -> AppResult<String> {
    use tauri::Emitter;

    let engine_dir = state.paths.data.join("ocr-engines");
    let handle = app.clone();

    let exe = tauri::async_runtime::spawn_blocking(move || {
        crate::ocr::paddle::download_and_install(&engine_dir, |p| {
            let _ = handle.emit("ocr://download", p);
        })
    })
    .await
    .map_err(|e| AppError::Msg(format!("下载任务异常: {e}")))??;

    let path_text = exe.to_string_lossy().to_string();
    {
        let mut cfg = state.settings.write();
        cfg.ocr.engine = crate::ocr::PADDLE.to_string();
        cfg.ocr.paddle_path = path_text.clone();
    }
    state.persist_settings()?;

    // 让设置页立刻看到状态变化
    let _ = app.emit("ocr://download", serde_json::json!({
        "stage": "done",
        "downloaded": 0,
        "total": 0,
        "message": "安装完成"
    }));

    Ok(path_text)
}

/// 钉图：把当前编辑结果钉在屏幕最上层当参考。
///
/// 先解码拿真实像素尺寸，才能算出"屏幕上该开多大"——
/// 尺寸交给前端算的话，多屏不同缩放一定错位。
#[tauri::command]
pub async fn image_pin(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    data_url: String,
) -> AppResult<()> {
    pin_data_url(&app, state.inner(), data_url).await
}

/// 把一张 dataURL 钉到屏幕上。
///
/// 抽出来是因为"框选完直接贴图"的手势也要用 —— 那条路不经过编辑器，
/// 手里只有一个 CaptureResult。
pub(crate) async fn pin_data_url(
    app: &tauri::AppHandle,
    state: &Arc<AppState>,
    data_url: String,
) -> AppResult<()> {
    let opacity = state.settings.read().editor.pin_opacity as f64;

    // 解码放阻塞线程池，别占着 IPC 线程
    let size = data_url.clone();
    let (w, h) = tauri::async_runtime::spawn_blocking(move || -> AppResult<(u32, u32)> {
        let (bytes, _mime) = data_url_to_bytes(&size)?;
        let img = image::load_from_memory(&bytes)
            .map_err(|e| AppError::Msg(format!("图片解码失败: {e}")))?;
        Ok((img.width(), img.height()))
    })
    .await
    .map_err(|e| AppError::Msg(format!("解码任务失败: {e}")))??;

    crate::windows::open_pin(app, data_url, w, h, opacity)
}

/* ------------------------- 取字窗口 ------------------------- */

/// 取走待识别的图。
///
/// 窗口新建时后端 emit 的事件会丢（webview 还没加载），所以前端加载完
/// 主动来取一次 —— 事件只作为"窗口已经开着"时的快速通道。
#[tauri::command]
pub fn ocr_take_image() -> Option<String> {
    crate::windows::take_pending_ocr()
}

/// 关掉取字窗口
#[tauri::command]
pub fn ocr_close(app: tauri::AppHandle) {
    if let Some(win) = tauri::Manager::get_webview_window(&app, "ocr") {
        let _ = win.close();
    }
}

/// 翻译一段文本。
///
/// 走 Google 的免密钥端点（`translate_a/single`）。够用、不用申请 key，
/// 但国内网络下可能连不上 —— 失败时把原因原样带回去，让窗口显示
/// "翻译服务不可达"，而不是默默转圈。
#[tauri::command]
pub async fn translate_text(text: String, to: Option<String>) -> AppResult<String> {
    let target = to.unwrap_or_else(|| "zh-CN".into());
    let text = text.trim();
    if text.is_empty() {
        return Err(AppError::Msg("没有可翻译的内容".into()));
    }
    // 端点限长，超了直接拒绝，别发出一个注定失败的请求
    if text.chars().count() > 4000 {
        return Err(AppError::Msg("文本太长，先选一段再翻译".into()));
    }

    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={target}&dt=t&q={}",
        percent_encode(text)
    );

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| AppError::Msg(format!("创建请求失败: {e}")))?;

    let resp = client
        .get(&url)
        .header("User-Agent", "Mozilla/5.0 SnipForge")
        .send()
        .await
        .map_err(|e| AppError::Msg(format!("翻译服务不可达: {e}")))?;

    if !resp.status().is_success() {
        return Err(AppError::Msg(format!("翻译服务返回 {}", resp.status())));
    }

    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Msg(format!("翻译结果解析失败: {e}")))?;

    // 结构是 [[["译文","原文",...], ...], ...]，把每一段译文连起来
    let mut out = String::new();
    if let Some(segments) = body.get(0).and_then(|v| v.as_array()) {
        for seg in segments {
            if let Some(t) = seg.get(0).and_then(|v| v.as_str()) {
                out.push_str(t);
            }
        }
    }
    if out.is_empty() {
        return Err(AppError::Msg("翻译服务没返回内容".into()));
    }
    Ok(out)
}

/// 最小可用的 URL 编码：只保留 unreserved 字符，其余按 UTF-8 字节转 %XX。
/// 不为这一处请求引入整个 urlencoding crate。
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
