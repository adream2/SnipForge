use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::types::{DirEntry, ProviderMeta, UploadOutcome};
use crate::uploader::{data_url_to_bytes, mime_to_ext, render_file_name, UploadFile, UploaderRegistry};
use std::sync::Arc;
use tauri::Manager;

fn registry(app: &tauri::AppHandle) -> AppResult<Arc<UploaderRegistry>> {
    app.try_state::<Arc<UploaderRegistry>>()
        .map(|s| s.inner().clone())
        .ok_or_else(|| AppError::Msg("图床注册表未初始化".into()))
}

#[tauri::command]
pub fn upload_providers(app: tauri::AppHandle) -> AppResult<Vec<ProviderMeta>> {
    Ok(registry(&app)?.list())
}

/// 保存某个 provider 的配置
#[tauri::command]
pub fn upload_provider_save(
    state: tauri::State<'_, Arc<AppState>>,
    provider_id: String,
    values: serde_json::Value,
) -> AppResult<()> {
    // 锁着的时候内存里没有凭据，写进去也只会被落盘逻辑剥掉 ——
    // 静默丢失比报错更糟：用户以为保存了，下次解锁发现 token 没了。
    if state.vault.is_enabled() && !state.vault.is_unlocked() {
        return Err(AppError::Msg("图床凭据已加密锁定，请先在「设置 → 安全」里解锁".into()));
    }
    {
        let mut cfg = state.settings.write();
        let map = cfg
            .uploader
            .providers
            .entry(provider_id)
            .or_default();
        if let Some(obj) = values.as_object() {
            for (k, v) in obj {
                map.insert(k.clone(), v.clone());
            }
        }
    }
    state.persist_settings()
}

#[tauri::command]
pub fn upload_set_active(
    state: tauri::State<'_, Arc<AppState>>,
    provider_id: String,
) -> AppResult<()> {
    state.settings.write().uploader.active_provider = provider_id;
    state.persist_settings()
}

/// 上传图片
#[tauri::command]
pub async fn upload_image(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    data_url: String,
    provider_id: Option<String>,
    file_name: Option<String>,
    // 单次上传的目录（可含 {yyyy}/{MM} 之类的日期占位符）。空 = 用配置里的路径
    dir: Option<String>,
    // 压缩前的体积，只为在历史里显示"省了多少"，拿不到就不传
    orig_bytes: Option<u64>,
) -> AppResult<UploadOutcome> {
    let registry = registry(&app)?;
    let state = state.inner().clone();

    let cfg = state.settings.read().clone();
    if !cfg.uploader.enabled {
        return Err(AppError::Msg("图床功能已在设置中关闭".into()));
    }

    let target = provider_id.unwrap_or_else(|| cfg.uploader.active_provider.clone());
    let (bytes, mime) = data_url_to_bytes(&data_url)?;
    let ext = mime_to_ext(&mime);
    let name = file_name.unwrap_or_else(|| {
        format!("{}.{}", render_file_name(&cfg.save.name_template, ext), ext)
    });

    let provider_cfg = cfg
        .uploader
        .providers
        .get(&target)
        .cloned()
        .unwrap_or_default();

    // 目录模板在这里就展开，图床层拿到的永远是最终路径
    let dir = dir
        .map(|d| crate::uploader::render_dir(&d))
        .filter(|d| !d.is_empty());

    // 插件扩展点：上传前可拦截（"这张图不该外传"这类策略）
    let (cancelled, _) = state.hooks.run(
        crate::hooks::names::IMAGE_BEFORE_UPLOAD,
        serde_json::json!({
            "providerId": target,
            "name": &name,
            "bytes": bytes.len(),
        }),
    );
    if cancelled {
        return Err(AppError::Msg("上传已被插件取消".into()));
    }

    // 下面这些东西会被 move 进上传线程，历史记录需要留一份
    let hist_name = name.clone();
    let hist_bytes = bytes.len() as u64;
    let hist_mime = mime.clone();
    let hist_dir = dir.clone();

    let outcome = tauri::async_runtime::spawn_blocking(move || {
        registry.upload(
            &target,
            UploadFile {
                file_name: name,
                mime_type: mime,
                buffer: bytes,
                dir,
            },
            &provider_cfg,
        )
    })
    .await
    .map_err(|e| AppError::Msg(format!("上传任务失败: {e}")))??;

    // 插件扩展点：拿到链接之后（写历史记录、发通知之类）
    state.hooks.run(
        crate::hooks::names::IMAGE_AFTER_UPLOAD,
        serde_json::json!({
            "providerId": outcome.provider_id,
            "url": outcome.result.url,
        }),
    );

    // 记历史。写盘失败只记日志 —— 图已经传上去了，不能因为本地落盘出错
    // 就让用户看到"上传失败"，那样他会重传一遍。
    let rec = crate::types::UploadRecord {
        id: new_record_id(),
        url: outcome.result.url.clone(),
        delete_url: outcome.result.delete_url.clone(),
        name: hist_name,
        bytes: hist_bytes,
        orig_bytes: orig_bytes.unwrap_or(0),
        format: hist_mime,
        provider_id: outcome.provider_id.clone(),
        provider_name: outcome.provider_name.clone(),
        dir: hist_dir,
        created_at: now_ms(),
        local_path: local_path_of(&outcome.result.url),
    };
    if let Err(e) = state.uploads.lock().push(rec) {
        log::warn!("写入上传历史失败: {e}");
    }

    Ok(outcome)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 同一毫秒内连传两张也要区分开，所以后面缀一个微秒余数
fn new_record_id() -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}-{:04}", now.as_millis(), (now.subsec_micros() / 100) % 10_000)
}

/// 本地目录图床返回的是 `file:///E:/...`，这种链接在 webview 里既显示不了缩略图、
/// 也没法右键"打开文件位置"。转回真实路径备用。
fn local_path_of(url: &str) -> Option<String> {
    let rest = url.strip_prefix("file:///")?;
    let decoded = urlencoding::decode(rest)
        .map(|c| c.into_owned())
        .unwrap_or_else(|_| rest.to_string());
    Some(if cfg!(windows) {
        decoded.replace('/', "\\")
    } else {
        format!("/{decoded}")
    })
}

/* ---------------- 上传历史 ---------------- */
#[tauri::command]
pub fn upload_history_list(
    state: tauri::State<'_, Arc<AppState>>,
    keyword: Option<String>,
    limit: Option<usize>,
) -> AppResult<Vec<crate::types::UploadRecord>> {
    Ok(state
        .uploads
        .lock()
        .list(keyword.as_deref(), limit.unwrap_or(200)))
}

#[tauri::command]
pub fn upload_history_remove(
    state: tauri::State<'_, Arc<AppState>>,
    id: String,
) -> AppResult<bool> {
    state.uploads.lock().remove(&id)
}

#[tauri::command]
pub fn upload_history_clear(state: tauri::State<'_, Arc<AppState>>) -> AppResult<()> {
    state.uploads.lock().clear()
}

/// 测试图床配置：上传一张 1x1 的探针图，确认凭据和权限都对
#[tauri::command]
pub async fn upload_test(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    provider_id: String,
) -> AppResult<UploadOutcome> {
    let registry = registry(&app)?;
    let cfg = state.settings.read().clone();
    let provider_cfg = cfg.uploader.providers.get(&provider_id).cloned().unwrap_or_default();

    let probe = image::RgbaImage::from_pixel(8, 8, image::Rgba([120, 90, 230, 255]));
    let mut bytes = Vec::new();
    probe
        .write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png)
        .map_err(|e| AppError::Msg(format!("探针图生成失败: {e}")))?;

    tauri::async_runtime::spawn_blocking(move || {
        registry.upload(
            &provider_id,
            UploadFile {
                file_name: format!("snipforge-probe-{}.png", chrono::Utc::now().format("%Y%m%d%H%M%S")),
                mime_type: "image/png".into(),
                buffer: bytes,
                dir: None,
            },
            &provider_cfg,
        )
    })
    .await
    .map_err(|e| AppError::Msg(format!("测试任务失败: {e}")))?
}

/// 列某个图床的目录（供「图床」页选路径用）。
///
/// `path` 为空 = 该图床的根。
#[tauri::command]
pub async fn upload_list_dirs(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    provider_id: String,
    path: Option<String>,
) -> AppResult<Vec<DirEntry>> {
    let registry = registry(&app)?;
    let cfg = state.settings.read().clone();
    let provider_cfg = cfg
        .uploader
        .providers
        .get(&provider_id)
        .cloned()
        .unwrap_or_default();
    let path = path.unwrap_or_default();

    tauri::async_runtime::spawn_blocking(move || {
        let p = registry
            .get(&provider_id)
            .ok_or_else(|| AppError::Msg(format!("未找到图床: {provider_id}")))?;
        p.list_dirs(&provider_cfg, &path)
    })
    .await
    .map_err(|e| AppError::Msg(format!("浏览任务失败: {e}")))?
}

/// 列图床里某个目录下的图片 —— 图床页的「列表」用的就是它。
///
/// 和上传一样丢进 spawn_blocking：GitHub 列目录是网络请求，卡在命令线程上
/// 会把整个界面拖住。
#[tauri::command]
pub async fn upload_list_images(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    provider_id: String,
    path: Option<String>,
    limit: Option<usize>,
) -> AppResult<Vec<crate::types::RemoteImage>> {
    let registry = registry(&app)?;
    let cfg = state.settings.read().clone();
    let provider_cfg = cfg
        .uploader
        .providers
        .get(&provider_id)
        .cloned()
        .unwrap_or_default();
    let path = path.unwrap_or_default();
    let limit = limit.unwrap_or(300);

    tauri::async_runtime::spawn_blocking(move || {
        let p = registry
            .get(&provider_id)
            .ok_or_else(|| AppError::Msg(format!("未找到图床: {provider_id}")))?;
        p.list_images(&provider_cfg, &path, limit)
    })
    .await
    .map_err(|e| AppError::Msg(format!("浏览任务失败: {e}")))?
}

/// 给本地图片生成一张缩略图的 data URL。
///
/// 本地目录图床返回的是 `file:///...`，webview 里既不能当 `<img src>`
/// （CSP 没放行 `file:`），也不该为了这个把 asset 协议整个打开 ——
/// 那等于给页面一个任意文件读取口子。所以由后端读图、缩小、转 base64。
///
/// 只接受图片扩展名 + 体积上限，别让它变成通用的任意文件读取接口。
#[tauri::command]
pub async fn upload_thumb(path: String, max: Option<u32>) -> AppResult<String> {
    let max = max.unwrap_or(240).clamp(48, 512);
    tauri::async_runtime::spawn_blocking(move || {
        use base64::Engine;

        let p = std::path::PathBuf::from(&path);
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default();
        let ext = name
            .rsplit_once('.')
            .map(|(_, e)| e.to_ascii_lowercase())
            .unwrap_or_default();
        if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "webp" | "bmp") {
            return Err(AppError::Msg("这个格式不做缩略图".into()));
        }
        let meta = std::fs::metadata(&p).map_err(|e| AppError::Msg(format!("读不到文件: {e}")))?;
        if meta.len() > 64 * 1024 * 1024 {
            return Err(AppError::Msg("图片太大，跳过缩略图".into()));
        }

        let img = image::open(&p).map_err(|e| AppError::Msg(format!("解码失败: {e}")))?;
        let thumb = img.thumbnail(max, max).to_rgba8();

        // JPEG 没有透明通道，直接丢 alpha 会让图标类图片的透明底变黑，
        // 所以合成到白底上
        let (w, h) = thumb.dimensions();
        let mut rgb = image::RgbImage::new(w, h);
        for (x, y, px) in thumb.enumerate_pixels() {
            let a = px[3] as f32 / 255.0;
            let blend = |c: u8| (c as f32 * a + 255.0 * (1.0 - a)).round() as u8;
            rgb.put_pixel(x, y, image::Rgb([blend(px[0]), blend(px[1]), blend(px[2])]));
        }

        let mut buf = Vec::new();
        rgb.write_to(
            &mut std::io::Cursor::new(&mut buf),
            image::ImageFormat::Jpeg,
        )
        .map_err(|e| AppError::Msg(format!("编码缩略图失败: {e}")))?;

        Ok(format!(
            "data:image/jpeg;base64,{}",
            base64::engine::general_purpose::STANDARD.encode(&buf)
        ))
    })
    .await
    .map_err(|e| AppError::Msg(format!("缩略图任务失败: {e}")))?
}
