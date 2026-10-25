use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::types::{ProviderMeta, UploadOutcome};
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

    tauri::async_runtime::spawn_blocking(move || {
        registry.upload(
            &target,
            UploadFile {
                file_name: name,
                mime_type: mime,
                buffer: bytes,
            },
            &provider_cfg,
        )
    })
    .await
    .map_err(|e| AppError::Msg(format!("上传任务失败: {e}")))?
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
            },
            &provider_cfg,
        )
    })
    .await
    .map_err(|e| AppError::Msg(format!("测试任务失败: {e}")))?
}
