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
use crate::types::{DirEntry, ProviderMeta, UploadOutcome, UploadResult};
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

pub fn cfg_bool(cfg: &Cfg, key: &str, fallback: bool) -> bool {
    cfg.get(key).and_then(|v| v.as_bool()).unwrap_or(fallback)
}

/// 拼装文件名：{yyyy}{MM}{dd}{HH}{mm}{ss}{rand}{type}
pub fn render_file_name(template: &str, ext: &str) -> String {
    let now = chrono::Local::now();
    let rand: String = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("{:04x}", (nanos ^ (std::process::id() << 8)) & 0xffff)
    };

    let mut out = template.to_string();
    for (key, value) in [
        ("{yyyy}", now.format("%Y").to_string()),
        ("{MM}", now.format("%m").to_string()),
        ("{dd}", now.format("%d").to_string()),
        ("{HH}", now.format("%H").to_string()),
        ("{mm}", now.format("%M").to_string()),
        ("{ss}", now.format("%S").to_string()),
        ("{rand}", rand),
        ("{type}", ext.to_string()),
    ] {
        out = out.replace(key, &value);
    }

    // Windows 文件名非法字符一律替换掉，别让保存莫名其妙失败
    let sanitized: String = out
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 32 => '_',
            c => c,
        })
        .collect();

    let trimmed = sanitized.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        format!("SnipForge_{}", chrono::Local::now().format("%Y%m%d_%H%M%S"))
    } else {
        trimmed.chars().take(180).collect()
    }
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

/// 展开目录模板：{yyyy} {MM} {dd} {HH} {mm} {ss}，并去掉首尾斜杠、合并多余斜杠。
///
/// 和文件名的模板是同一套占位符，方便用户写 `blog/{yyyy}/{MM}` 这种"按年月归档"。
pub fn render_dir(template: &str) -> String {
    let now = chrono::Local::now();
    let mut out = template.trim().to_string();
    for (key, value) in [
        ("{yyyy}", now.format("%Y").to_string()),
        ("{yy}", now.format("%y").to_string()),
        ("{MM}", now.format("%m").to_string()),
        ("{dd}", now.format("%d").to_string()),
        ("{HH}", now.format("%H").to_string()),
        ("{mm}", now.format("%M").to_string()),
        ("{ss}", now.format("%S").to_string()),
    ] {
        out = out.replace(key, &value);
    }
    // 干掉空段（a//b → a/b），别让远端出现奇怪的路径
    let parts: Vec<&str> = out
        .split('/')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    parts.join("/")
}
