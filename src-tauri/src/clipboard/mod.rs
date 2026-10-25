//! 剪贴板监听与服务。
//!
//! 变更检测用 Win32 的 `GetClipboardSequenceNumber` —— 每次剪贴板内容变化时系统会递增这个计数。
//! 轮询它几乎零开销（一个原子读），只有真的变了才去读内容。
//! 这比"每 500ms 读一次剪贴板内容再比对"省一个数量级，也比注册
//! AddClipboardFormatListener 需要的隐藏窗口 + 消息循环简单得多。

pub mod store;

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::types::{ClipboardItem, ClipboardQuery, ClipboardStats};
use base64::Engine;
use parking_lot::Mutex;
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use store::ClipboardStore;
use tauri::{AppHandle, Emitter, Manager};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

pub struct ClipboardService {
    pub store: ClipboardStore,
    running: AtomicBool,
    /// 自己往剪贴板写内容时的静默期，避免把刚复制出去的又记一遍
    suppress_until: Mutex<i64>,
    last_sequence: Mutex<u32>,
}

impl ClipboardService {
    pub fn new(paths: &crate::paths::Paths) -> Self {
        Self {
            store: ClipboardStore::new(paths),
            running: AtomicBool::new(false),
            suppress_until: Mutex::new(0),
            last_sequence: Mutex::new(0),
        }
    }

    pub fn suppress(&self, ms: i64) {
        *self.suppress_until.lock() = chrono::Utc::now().timestamp_millis() + ms;
    }

    fn is_suppressed(&self) -> bool {
        chrono::Utc::now().timestamp_millis() < *self.suppress_until.lock()
    }
}

pub fn sequence_number() -> u32 {
    #[cfg(windows)]
    unsafe {
        windows::Win32::System::DataExchange::GetClipboardSequenceNumber()
    }
    #[cfg(not(windows))]
    0
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{:x}", hasher.finalize())
}

/// 启动监听线程
pub fn start_monitor(app: AppHandle, state: Arc<AppState>, interval_ms: u64, capture_images: bool, skip_sensitive: bool) {
    let service = match app.try_state::<Arc<ClipboardService>>() {
        Some(s) => s.inner().clone(),
        None => {
            log::error!("剪贴板服务未初始化");
            return;
        }
    };

    if service.running.swap(true, Ordering::SeqCst) {
        return;
    }

    std::thread::spawn(move || {
        // 先对齐一次，避免启动瞬间把当前剪贴板当成"新内容"
        *service.last_sequence.lock() = sequence_number();
        log::info!("剪贴板监听已启动，间隔 {interval_ms}ms");

        loop {
            if !service.running.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(interval_ms));

            let seq = sequence_number();
            if seq == *service.last_sequence.lock() {
                continue;
            }
            *service.last_sequence.lock() = seq;

            // 注意：这里**不能**因为"是自己写的"就跳过。
            // 截图后点复制、OCR 复制文字，都是往剪贴板写内容 —— 用户期望它们出现在历史里。
            // 重复内容由 hash 去重兜住（命中就只更新时间并提到最前），不会产生垃圾条目。
            match scan_clipboard(&service, capture_images, skip_sensitive) {
                Ok(Some(item)) => {
                    log::info!("剪贴板收录: {} ({} 字节)", item.kind, item.size);
                    let _ = app.emit("clipboard://new", &item);
                    let _ = app.emit("clipboard://stats", service.store.stats());
                    let _ = service.store.save();
                }
                Ok(None) => log::debug!("剪贴板变化无有效内容（seq={seq}）"),
                Err(e) => log::warn!("剪贴板扫描失败: {e}"),
            }
        }
        log::info!("剪贴板监听已停止");
    });
}

pub fn stop_monitor(app: &AppHandle) {
    if let Some(service) = app.try_state::<Arc<ClipboardService>>() {
        service.running.store(false, Ordering::SeqCst);
        let _ = service.store.save();
    }
}

/// 强特征密钥/凭据。默认不启用（用户要求全部记录），保留给需要的人
#[allow(dead_code)]
fn looks_sensitive(text: &str) -> bool {
    const MARKERS: &[&str] = &[
        "-----BEGIN",
        "ghp_",
        "github_pat_",
        "AKIA",
        "sk-",
        "xoxb-",
        "xoxp-",
    ];
    if MARKERS.iter().any(|m| text.contains(m)) {
        return true;
    }
    // JWT: 三段 base64 用点连接
    let dots = text.matches('.').count();
    dots == 2 && text.len() > 40 && text.starts_with("eyJ")
}

enum RawClip {
    Text(String),
    Html(String, String),
    Image(image::RgbaImage),
    Files(Vec<String>),
}

#[cfg(windows)]
fn read_files() -> Vec<String> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard};
    use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

    const CF_HDROP: u32 = 15;
    let mut out = Vec::new();
    unsafe {
        if OpenClipboard(None).is_err() {
            return out;
        }
        if let Ok(handle) = GetClipboardData(CF_HDROP) {
            let hdrop = HDROP(handle.0 as *mut _);
            let count = DragQueryFileW(hdrop, 0xFFFF_FFFF, None);
            for i in 0..count {
                let len = DragQueryFileW(hdrop, i, None);
                if len == 0 {
                    continue;
                }
                let mut buf = vec![0u16; len as usize + 1];
                let written = DragQueryFileW(hdrop, i, Some(&mut buf));
                if written > 0 {
                    out.push(String::from_utf16_lossy(&buf[..written as usize]));
                }
            }
            let _ = HGLOBAL::default();
        }
        let _ = CloseClipboard();
    }
    out
}

#[cfg(not(windows))]
fn read_files() -> Vec<String> {
    Vec::new()
}

#[cfg(windows)]
fn has_format(format: u32) -> bool {
    unsafe { windows::Win32::System::DataExchange::IsClipboardFormatAvailable(format).is_ok() }
}

fn read_raw(capture_images: bool) -> Option<RawClip> {
    let mut cb = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            // 剪贴板被别的程序独占时会发生，下一轮会重试
            log::debug!("打开剪贴板失败: {e}");
            return None;
        }
    };

    // 图片优先：截图工具最高频的动作就是截完直接 Ctrl+C
    if capture_images {
        match cb.get_image() {
            Ok(img) if img.width >= 2 && img.height >= 2 => {
                if let Some(buf) = image::RgbaImage::from_raw(
                    img.width as u32,
                    img.height as u32,
                    img.bytes.into_owned(),
                ) {
                    return Some(RawClip::Image(buf));
                }
            }
            Ok(_) => {}
            Err(e) => log::debug!("剪贴板里没有图片: {e}"),
        }
    }

    // 文件列表要单独走 Win32（arboard 不提供 CF_HDROP）
    let files = read_files();
    if !files.is_empty() {
        return Some(RawClip::Files(files));
    }

    match cb.get_text() {
        Ok(text) if !text.trim().is_empty() => Some(RawClip::Text(text)),
        _ => None,
    }
}

fn scan_clipboard(
    service: &ClipboardService,
    capture_images: bool,
    _skip_sensitive: bool,
) -> AppResult<Option<ClipboardItem>> {
    let Some(raw) = read_raw(capture_images) else {
        log::debug!("read_raw 返回空，剪贴板里可能是不支持的类型");
        return Ok(None);
    };

    let now = chrono::Utc::now().timestamp_millis();

    let (hash, kind) = match &raw {
        RawClip::Image(img) => (sha256_hex(img.as_raw()), "image"),
        RawClip::Text(t) => (sha256_hex(t.as_bytes()), detect_kind(t)),
        RawClip::Files(f) => (sha256_hex(f.join("|").as_bytes()), "files"),
        RawClip::Html(h, _) => (sha256_hex(h.as_bytes()), "html"),
    };

    // 相同内容：只更新时间，不重复入库
    if service.store.find_by_hash(&hash).is_some() {
        service.store.touch(&hash);
        return Ok(None);
    }

    let mut item = ClipboardItem {
        id: format!("c_{}_{}", now, &hash[..6.min(hash.len())]),
        kind: kind.to_string(),
        text: None,
        html: None,
        blob: None,
        width: None,
        height: None,
        files: None,
        color: None,
        hash: hash.clone(),
        size: 0,
        source_app: {
            let app = crate::win32::window::clipboard_source_app();
            (!app.is_empty()).then_some(app)
        },
        pinned: false,
        tags: Vec::new(),
        created_at: now,
        last_used_at: now,
        hits: 1,
    };

    match raw {
        RawClip::Text(t) => {
            item.size = t.len() as u64;
            item.color = detect_color(&t);
            item.text = Some(t);
        }
        RawClip::Html(h, plain) => {
            item.size = h.len() as u64;
            item.text = Some(plain);
            item.html = Some(h);
        }
        RawClip::Files(files) => {
            item.size = files.iter().map(|f| f.len() as u64).sum();
            item.text = Some(files.join("\n"));
            item.files = Some(files);
        }
        RawClip::Image(img) => {
            let blob = &hash[..24.min(hash.len())];
            item.blob = Some(blob.to_string());
            item.width = Some(img.width());
            item.height = Some(img.height());
            item.size = img.as_raw().len() as u64;

            let png_path = service.store.blobs_dir().join(format!("{blob}.png"));
            img.save_with_format(&png_path, image::ImageFormat::Png)
                .map_err(|e| AppError::Msg(format!("图片落盘失败: {e}")))?;

            // 缩略图：列表滚动时不用每次都解大图
            let thumb_w = 320u32.min(img.width());
            let ratio = thumb_w as f64 / img.width() as f64;
            let thumb_h = ((img.height() as f64) * ratio).round().max(1.0) as u32;
            let thumb = image::imageops::resize(&img, thumb_w, thumb_h, image::imageops::FilterType::Triangle);
            let rgb = image::DynamicImage::ImageRgba8(thumb).to_rgb8();
            let thumb_path = service.store.blobs_dir().join(format!("{blob}.t.jpg"));
            rgb.save_with_format(&thumb_path, image::ImageFormat::Jpeg)
                .map_err(|e| AppError::Msg(format!("缩略图写入失败: {e}")))?;
        }
    }

    Ok(Some(service.store.add(item)))
}

fn detect_kind(text: &str) -> &'static str {
    if detect_color(text).is_some() {
        "color"
    } else {
        "text"
    }
}

fn detect_color(text: &str) -> Option<String> {
    let t = text.trim();
    if t.starts_with('#') && (t.len() == 4 || t.len() == 7) && t[1..].chars().all(|c| c.is_ascii_hexdigit()) {
        let hex = if t.len() == 4 {
            let cs: Vec<char> = t[1..].chars().collect();
            format!("#{0}{0}{1}{1}{2}{2}", cs[0], cs[1], cs[2])
        } else {
            t.to_string()
        };
        return Some(hex.to_uppercase());
    }
    if (t.starts_with("rgb(") || t.starts_with("rgba(")) && t.ends_with(')') {
        let inner = t.split('(').nth(1)?.trim_end_matches(')');
        let parts: Vec<&str> = inner.split(',').map(|s| s.trim()).collect();
        if parts.len() >= 3 {
            let to_hex = |s: &str| -> Option<String> {
                s.parse::<u16>().ok().map(|v| format!("{:02X}", v.min(255)))
            };
            return Some(format!(
                "#{}{}{}",
                to_hex(parts[0])?,
                to_hex(parts[1])?,
                to_hex(parts[2])?
            ));
        }
    }
    None
}

/* ------------------------- 对外 API ------------------------- */

pub fn list(service: &ClipboardService, query: ClipboardQuery) -> Vec<ClipboardItem> {
    service.store.query(&query)
}

pub fn stats(service: &ClipboardService) -> ClipboardStats {
    service.store.stats()
}

/// 把条目写回系统剪贴板
pub fn paste(service: &ClipboardService, id: &str) -> AppResult<String> {
    let item = service
        .store
        .find_by_id(id)
        .ok_or_else(|| AppError::Msg("条目不存在".into()))?;

    service.suppress(900);
    let mut cb = arboard::Clipboard::new().map_err(|e| AppError::Clipboard(e.to_string()))?;

    match item.kind.as_str() {
        "image" => {
            let path = service
                .store
                .blob_path(&item)
                .ok_or_else(|| AppError::Msg("图片文件已丢失".into()))?;
            let img = image::open(&path)
                .map_err(|e| AppError::Msg(format!("图片读取失败: {e}")))?
                .to_rgba8();
            cb.set_image(arboard::ImageData {
                width: img.width() as usize,
                height: img.height() as usize,
                bytes: std::borrow::Cow::Owned(img.into_raw()),
            })
            .map_err(|e| AppError::Clipboard(e.to_string()))?;
        }
        "files" => {
            // 文件必须写成 CF_HDROP，粘到资源管理器/聊天窗口才是「文件」本身。
            // 退回多行路径文本的话，用户粘出来是一串字符串，还得自己再去找文件。
            let paths = item.files.clone().unwrap_or_default();
            if crate::win32::clipboard::set_files(&paths).is_err() {
                let text = paths.join("\r\n");
                cb.set_text(text).map_err(|e| AppError::Clipboard(e.to_string()))?;
            }
        }
        _ => {
            let text = item.text.clone().unwrap_or_default();
            cb.set_text(text).map_err(|e| AppError::Clipboard(e.to_string()))?;
        }
    }

    service.store.touch(&item.hash);
    let _ = service.store.save();
    Ok(item.kind)
}

pub fn copy_text(service: &ClipboardService, text: &str) -> AppResult<()> {
    service.suppress(700);
    let mut cb = arboard::Clipboard::new().map_err(|e| AppError::Clipboard(e.to_string()))?;
    cb.set_text(text).map_err(|e| AppError::Clipboard(e.to_string()))?;
    Ok(())
}

pub fn copy_image_bytes(service: &ClipboardService, png: &[u8]) -> AppResult<()> {
    let img = image::load_from_memory(png)
        .map_err(|e| AppError::Msg(format!("图片解码失败: {e}")))?
        .to_rgba8();
    service.suppress(900);
    let mut cb = arboard::Clipboard::new().map_err(|e| AppError::Clipboard(e.to_string()))?;
    cb.set_image(arboard::ImageData {
        width: img.width() as usize,
        height: img.height() as usize,
        bytes: std::borrow::Cow::Owned(img.into_raw()),
    })
    .map_err(|e| AppError::Clipboard(e.to_string()))?;
    Ok(())
}

pub fn thumbnail_data_url(service: &ClipboardService, id: &str) -> Option<String> {
    let item = service.store.find_by_id(id)?;
    let path = service.store.thumb_path(&item)?;
    let bytes = std::fs::read(path).ok()?;
    Some(format!("data:image/jpeg;base64,{}", B64.encode(bytes)))
}
