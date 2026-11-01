//! 图床系统。
//!
//! 三层扩展能力，从轻到重：
//!   1. **声明式 provider** —— 往 plugins/ 丢一个 .json 就能接入任意 HTTP 图床，零代码
//!   2. **内置 provider** —— GitHub / S3 / SM.MS / 自定义 HTTP / 本地目录
//!   3. **trait 扩展** —— 编译期实现 Provider trait，适合需要特殊签名的服务
//!
//! 三者共用同一套 ProviderMeta，UI 侧不需要区分来源。

pub mod history;
pub mod providers;

use crate::error::{AppError, AppResult};
use crate::types::{DirEntry, ProviderMeta, RemoteImage, UploadOutcome, UploadResult};
use parking_lot::RwLock;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

pub type Cfg = HashMap<String, serde_json::Value>;

#[derive(Debug, Clone)]
pub struct UploadFile {
    pub file_name: String,
    pub mime_type: String,
    pub buffer: Vec<u8>,
    /// 单次上传的目录覆盖。None = 用配置里的路径。
    /// 已经在命令层按模板渲染过（{yyyy}/{MM} 之类），到这儿就是最终值。
    pub dir: Option<String>,
}

pub trait Provider: Send + Sync {
    fn meta(&self) -> ProviderMeta;
    fn upload(&self, file: &UploadFile, cfg: &Cfg) -> AppResult<UploadResult>;

    /// 列某个目录下的子目录。只有 can_browse 的图床需要实现。
    fn list_dirs(&self, _cfg: &Cfg, _path: &str) -> AppResult<Vec<DirEntry>> {
        Err(AppError::Msg("这个图床不支持浏览目录".into()))
    }

    /// 列某个目录下的图片。只有 can_list_images 的图床需要实现。
    ///
    /// `dir` 为空 = 图床根目录。返回结果按时间/名称排序，前端不再排。
    fn list_images(&self, _cfg: &Cfg, _dir: &str, _limit: usize) -> AppResult<Vec<RemoteImage>> {
        Err(AppError::Msg("这个图床不支持浏览已有图片".into()))
    }
}

pub struct UploaderRegistry {
    providers: RwLock<HashMap<String, Arc<dyn Provider>>>,
}

impl UploaderRegistry {
    pub fn new() -> Self {
        Self {
            providers: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, provider: Arc<dyn Provider>) {
        let id = provider.meta().id;
        log::debug!("注册图床: {id}");
        self.providers.write().insert(id, provider);
    }

    pub fn unregister(&self, id: &str) {
        self.providers.write().remove(id);
    }

    pub fn get(&self, id: &str) -> Option<Arc<dyn Provider>> {
        self.providers.read().get(id).cloned()
    }

    pub fn list(&self) -> Vec<ProviderMeta> {
        let mut metas: Vec<ProviderMeta> = self.providers.read().values().map(|p| p.meta()).collect();
        // 内置的排前面，其余按名字排
        metas.sort_by(|a, b| match (a.origin.as_str(), b.origin.as_str()) {
            ("builtin", "builtin") => a.name.cmp(&b.name),
            ("builtin", _) => std::cmp::Ordering::Less,
            (_, "builtin") => std::cmp::Ordering::Greater,
            _ => a.name.cmp(&b.name),
        });
        metas
    }

    pub fn upload(
        &self,
        provider_id: &str,
        file: UploadFile,
        cfg: &Cfg,
    ) -> AppResult<UploadOutcome> {
        let started = Instant::now();
        let provider = self
            .get(provider_id)
            .ok_or_else(|| AppError::Msg(format!("未找到图床: {provider_id}")))?;

        let meta = provider.meta();
        log::info!("上传 {} → {}", file.file_name, meta.name);

        let result = provider.upload(&file, cfg)?;

        Ok(UploadOutcome {
            provider_id: meta.id.clone(),
            provider_name: meta.name,
            result,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }
}

impl Default for UploaderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/* --------------------- 配置取值小工具 --------------------- */

pub fn cfg_str(cfg: &Cfg, key: &str) -> String {
    cfg.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .trim()
        .to_string()
}

pub fn mime_to_ext(mime: &str) -> &'static str {
    match mime {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/bmp" => "bmp",
        "image/avif" => "avif",
        "image/gif" => "gif",
        _ => "png",
    }
}

/// data URL → 原始字节
pub fn data_url_to_bytes(data_url: &str) -> AppResult<(Vec<u8>, String)> {
    use base64::Engine;
    let (head, payload) =
        data_url.split_once(',').ok_or_else(|| AppError::Msg("无效的图片数据".into()))?;
    let mime = head
        .trim_start_matches("data:")
        .split(';')
        .next()
        .unwrap_or("image/png")
        .to_string();
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(payload)
        .map_err(|e| AppError::Msg(format!("Base64 解码失败: {e}")))?;
    Ok((bytes, mime))
}

pub fn build_registry() -> UploaderRegistry {
    let registry = UploaderRegistry::new();
    registry.register(Arc::new(providers::GithubProvider));
    registry.register(Arc::new(providers::S3Provider));
    registry.register(Arc::new(providers::SmmsProvider));
    registry.register(Arc::new(providers::CustomHttpProvider));
    registry.register(Arc::new(providers::LocalFolderProvider));
    log::info!("已注册内置图床 {} 个", registry.list().len());
    registry
}

/* --------------------- 路径安全 --------------------- */
// 纯逻辑在 core crate 里（那边有真正能跑的单测）。
// 这里保留同名入口，调用方不用关心它住在哪个 crate。
pub use snipforge_core::safe::{render_dir, render_file_name};

/// 见 `snipforge_core::safe::safe_join`。这里只把错误换成 `AppError`。
pub fn safe_join(root: &str, sub: &str) -> AppResult<std::path::PathBuf> {
    snipforge_core::safe::safe_join(root, sub).map_err(AppError::Msg)
}

/// 见 `snipforge_core::safe::safe_file_name`。
pub fn safe_file_name(name: &str) -> AppResult<String> {
    snipforge_core::safe::safe_file_name(name).map_err(AppError::Msg)
}
