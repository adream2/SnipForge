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
use std::time::{Duration, Instant};
use store::{AddResult, ClipboardStore};
use tauri::{AppHandle, Emitter, Manager};

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// 一次扫描的策略。
///
/// 集中成一个结构体而不是散着传参：以前每加一个开关就要改一遍
/// `start_monitor` / `sync_now` / `scan_clipboard` 三处的签名。
#[derive(Clone, Debug)]
pub struct ScanOpts {
    pub capture_images: bool,
    /// 跳过形似密钥的内容（默认关，见 ClipboardConfig 注释）
    pub skip_sensitive: bool,
    /// 不记录这些程序复制的内容（小写 exe 名，带不带 `.exe` 都认）
    pub ignore_apps: Vec<String>,
}

impl ScanOpts {
    pub fn from_cfg(cfg: &crate::config::ClipboardConfig) -> Self {
        Self {
            capture_images: cfg.capture_images,
            skip_sensitive: cfg.skip_sensitive,
            ignore_apps: cfg.ignore_apps.clone(),
        }
    }
}

pub struct ClipboardService {
    pub store: ClipboardStore,
    running: AtomicBool,
    last_sequence: Mutex<u32>,
}

impl ClipboardService {
    pub fn new(paths: &crate::paths::Paths) -> Self {
        Self {
            store: ClipboardStore::new(paths),
            running: AtomicBool::new(false),
            last_sequence: Mutex::new(0),
        }
    }

}

/*
 * 这里**没有**"自己写剪贴板就跳过"的抑制机制，是刻意的：
 * 截图后点复制、OCR 复制文字、面板里粘贴，都是我们自己往剪贴板写内容，
 * 用户同样期望它们出现在历史里。重复内容由 hash 去重兜住（命中只 touch 提到最前），
 * 不会产生垃圾条目。以前那套 suppress 曾经把用户刚复制的图永久丢掉。
 */

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

/// 启动监听线程。
///
/// 配置**每轮现读**，不做启动快照：这样"记录图片 / 过滤敏感内容 / 排除程序"
/// 在设置页改完立刻生效。以前是启动时把三个 bool 拷进闭包，
/// 用户关掉"记录图片"后后台照样在存图，只能重启才停 —— 典型的"开关是装饰"。
pub fn start_monitor(app: AppHandle, state: Arc<AppState>) {
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
        log::info!("剪贴板监听已启动");

        // 保留天数是"天级"策略，半小时查一次就够，不必每轮扫
        let mut last_purge = Instant::now();

        loop {
            if !service.running.load(Ordering::SeqCst) {
                break;
            }

            let (interval_ms, enabled, opts, retention_days) = {
                let s = state.settings.read();
                (
                    s.clipboard.poll_interval_ms.clamp(120, 5000),
                    s.clipboard.enabled,
                    ScanOpts::from_cfg(&s.clipboard),
                    s.clipboard.retention_days,
                )
            };

            std::thread::sleep(Duration::from_millis(interval_ms));

            // 上一轮被节流掉的写入在这里补上。一个轮询周期（默认 500ms）内
            // 一定会落到磁盘，同时不会因为连续复制而反复重写几 MB 的索引。
            let _ = service.store.flush_if_pending();

            if last_purge.elapsed() >= Duration::from_secs(1800) {
                last_purge = Instant::now();
                let removed = service.store.purge_older_than(retention_days);
                if removed > 0 {
                    log::info!("按保留天数（{retention_days} 天）清理 {removed} 条剪贴板历史");
                    if let Err(e) = service.store.save() {
                        log::warn!("清理后保存剪贴板历史失败: {e}");
                    }
                    let _ = app.emit("clipboard://stats", service.store.stats());
                }
            }

            // 关掉历史记录时只保留清理逻辑，不再采集
            if !enabled {
                continue;
            }

            let seq = sequence_number();
            if seq == *service.last_sequence.lock() {
                continue;
            }

            // 注意：序列号**只在真的读到内容之后**才推进。
            //
            // 以前是先推进再扫描，于是「剪贴板被占用」这一轮会被当成已处理，
            // 那次变化永远丢了 —— 而用户「Ctrl+C 完立刻按 Win+V」正好落在
            // 源程序还在渲染剪贴板的那个窗口里，表现就是「刚复制的不在列表里」。
            //
            // 另外这里**不能**因为"是自己写的"就跳过：截图后点复制、OCR 复制文字
            // 都是往剪贴板写内容，用户期望它们出现在历史里。重复内容由 hash 去重兜住。
            match scan_clipboard(&service, &opts) {
                Ok(ScanResult::New(item)) => {
                    *service.last_sequence.lock() = seq;
                    log::info!("剪贴板收录: {} ({} 字节)", item.kind, item.size);
                    // 插件扩展点：已经在历史里了，钩子只能做"通知"，不能撤销
                    state.hooks.run(
                        crate::hooks::names::CLIPBOARD_CAPTURED,
                        serde_json::json!({
                            "id": item.id,
                            "kind": item.kind,
                            "size": item.size,
                            "sourceApp": item.source_app,
                        }),
                    );
                    let _ = app.emit("clipboard://new", &item);
                    let _ = app.emit("clipboard://stats", service.store.stats());
                    // 节流写盘：连着复制时不必每条都重写整个索引，
                    // 被跳过的由下一轮 flush_if_pending 补上
                    let _ = service.store.save_throttled();
                }
                Ok(ScanResult::Duplicate) => {
                    *service.last_sequence.lock() = seq;
                    let _ = app.emit("clipboard://stats", service.store.stats());
                    let _ = service.store.save_throttled();
                }
                Ok(ScanResult::Skipped(reason)) => {
                    *service.last_sequence.lock() = seq;
                    log::debug!("剪贴板变化已跳过：{reason}");
                }
                Ok(ScanResult::Empty) => {
                    *service.last_sequence.lock() = seq;
                    log::debug!("剪贴板变化无有效内容（seq={seq}）");
                }
                Ok(ScanResult::Busy) => {
                    // 不推进序列号，下一轮继续试
                    log::debug!("剪贴板正被占用，{interval_ms}ms 后重试（seq={seq}）");
                }
                Err(e) => {
                    *service.last_sequence.lock() = seq;
                    log::warn!("剪贴板扫描失败: {e}");
                }
            }
        }
        log::info!("剪贴板监听已停止");
    });
}

/// 立刻扫一次剪贴板，不等轮询周期。
///
/// 面板打开前必须调这个：用户「Ctrl+C → 立刻 Win+V」的间隔常常不到 100ms，
/// 500ms 的轮询还没跑到，列表里就没有刚复制的那条 ——
/// 用户看到的是「按了没反应」，或者粘出来是上一次的内容。
///
/// 返回新收录的条目（没有新内容就是 None）。剪贴板被占用时**不推进序列号**，
/// 留一轮给下一次重试。
pub fn sync_now(app: &AppHandle) -> Option<ClipboardItem> {
    let service = app
        .try_state::<Arc<ClipboardService>>()?
        .inner()
        .clone();
    let state = app.try_state::<Arc<AppState>>()?;
    let (enabled, opts) = {
        let s = state.settings.read();
        (s.clipboard.enabled, ScanOpts::from_cfg(&s.clipboard))
    };
    // 历史记录关掉了就别再往里塞：面板打开时也不该偷偷收录
    if !enabled {
        return None;
    }

    let seq = sequence_number();
    if seq == *service.last_sequence.lock() {
        return None;
    }

    // 刚 Ctrl+C 的一瞬间，源程序还在渲染延迟格式，剪贴板是锁着的。
    // 这一次读不到就白等 500ms 轮询，而面板已经打开了 —— 用户看到的就是
    // 「列表里没有刚复制的东西」。所以这里最多重试 4 次（约 60ms），
    // 用 60ms 的延迟换「列表一定是最新的」，值。
    for attempt in 0..4 {
        match scan_clipboard(&service, &opts) {
            Ok(ScanResult::New(item)) => {
                *service.last_sequence.lock() = seq;
                log::info!("剪贴板即时收录: {} ({} 字节)", item.kind, item.size);
                state.hooks.run(
                    crate::hooks::names::CLIPBOARD_CAPTURED,
                    serde_json::json!({
                        "id": item.id,
                        "kind": item.kind,
                        "size": item.size,
                        "sourceApp": item.source_app,
                    }),
                );
                let _ = service.store.save();
                let _ = app.emit("clipboard://new", &item);
                return Some(item);
            }
            Ok(ScanResult::Duplicate) => {
                *service.last_sequence.lock() = seq;
                let _ = service.store.save();
                return None;
            }
            Ok(ScanResult::Skipped(_)) | Ok(ScanResult::Empty) => {
                *service.last_sequence.lock() = seq;
                return None;
            }
            Ok(ScanResult::Busy) => {
                if attempt == 3 {
                    // 留给轮询线程：序列号没推进，下一轮还会再试
                    log::debug!("剪贴板持续被占用，留给轮询重试");
                    return None;
                }
                std::thread::sleep(Duration::from_millis(15));
            }
            Err(e) => {
                *service.last_sequence.lock() = seq;
                log::warn!("剪贴板即时扫描失败: {e}");
                return None;
            }
        }
    }
    None
}

/// 强特征密钥/凭据。默认不启用（`clipboard.skip_sensitive`），
/// 打开后由 `scan_clipboard` 在入库前过滤。
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

/// 读剪贴板的结果。
///
/// **必须区分「空」和「占用」**：刚 Ctrl+C 的一瞬间，源程序还在渲染延迟格式，
/// 剪贴板是锁着的，这时候 `OpenClipboard` 会失败。如果把它当成「没有内容」
/// 一起跳过，那次变化就永远丢了 —— 用户「复制完立刻粘贴」正好落在这个窗口里，
/// 表现就是「刚复制的东西不在列表里」。
enum ReadOutcome {
    Got(RawClip),
    /// 剪贴板里没有我们支持的内容（空、或者只有不认识的格式）
    Empty,
    /// 剪贴板被别的进程占着，内容未知，必须重试
    Busy,
}

fn read_raw(capture_images: bool) -> ReadOutcome {
    let mut cb = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(e) => {
            // 被独占：这是"暂时读不到"，不是"没有内容"，交给调用方重试
            log::debug!("打开剪贴板失败（将重试）: {e}");
            return ReadOutcome::Busy;
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
                    return ReadOutcome::Got(RawClip::Image(buf));
                }
            }
            Ok(_) => {}
            Err(e) => log::debug!("剪贴板里没有图片: {e}"),
        }
    }

    // 文件列表要单独走 Win32（arboard 不提供 CF_HDROP）
    let files = read_files();
    if !files.is_empty() {
        return ReadOutcome::Got(RawClip::Files(files));
    }

    match cb.get_text() {
        Ok(text) if !text.trim().is_empty() => ReadOutcome::Got(RawClip::Text(text)),
        _ => ReadOutcome::Empty,
    }
}

/// 一次扫描的结论。调用方靠它决定要不要推进「已处理到的序列号」。
pub enum ScanResult {
    /// 新收录了一条
    New(ClipboardItem),
    /// 内容已在历史里，只把时间往前挪
    Duplicate,
    /// 有意跳过（来源程序被排除 / 命中敏感内容过滤），带一句原因便于排查
    Skipped(String),
    /// 没有可收录的内容
    Empty,
    /// 暂时读不到，下轮再来
    Busy,
}

fn scan_clipboard(service: &ClipboardService, opts: &ScanOpts) -> AppResult<ScanResult> {
    // 来源程序过滤放在读图之前：读一张 4K 图再丢掉是纯浪费
    let source_app = {
        let app = crate::win32::window::clipboard_source_app();
        (!app.is_empty()).then_some(app)
    };
    if let Some(app) = &source_app {
        if opts
            .ignore_apps
            .iter()
            .any(|e| crate::win32::window::exe_name_eq(e, app))
        {
            return Ok(ScanResult::Skipped(format!("来源程序 {app} 在排除列表里")));
        }
    }

    let raw = match read_raw(opts.capture_images) {
        ReadOutcome::Got(r) => r,
        ReadOutcome::Empty => return Ok(ScanResult::Empty),
        ReadOutcome::Busy => return Ok(ScanResult::Busy),
    };

    // 敏感内容过滤只对文本有意义（图片、文件名谈不上"形似密钥"）
    if opts.skip_sensitive {
        if let RawClip::Text(t) = &raw {
            if looks_sensitive(t) {
                return Ok(ScanResult::Skipped("内容形似密钥，按设置跳过".into()));
            }
        }
    }

    let now = chrono::Utc::now().timestamp_millis();

    let (hash, kind) = match &raw {
        RawClip::Image(img) => (sha256_hex(img.as_raw()), "image"),
        RawClip::Text(t) => (sha256_hex(t.as_bytes()), detect_kind(t)),
        RawClip::Files(f) => (sha256_hex(f.join("|").as_bytes()), "files"),
    };

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
        source_app,
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

    // 查重和插入在 store 内部同一把锁里完成 —— 两个扫描入口并发也只会进一条
    Ok(match service.store.add(item) {
        AddResult::Added(i) => ScanResult::New(i),
        AddResult::Duplicate => ScanResult::Duplicate,
    })
}

fn detect_kind(text: &str) -> &'static str {
    if detect_color(text).is_some() {
        "color"
    } else if is_link(text) {
        "link"
    } else {
        "text"
    }
}

/// 只认明确的单条链接：`http(s)://` 或 `www.` 开头，且整段没有空白。
///
/// 不用"含点就算链接"那种糊弄判断 —— 那样 `readme.txt`、`1.2.3` 全成了链接，
/// 分类反而没法用。宁可漏掉，也别误报。
fn is_link(text: &str) -> bool {
    let t = text.trim();
    if t.is_empty() || t.len() > 2048 || t.split_whitespace().count() != 1 {
        return false;
    }
    let lower = t.to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://") || lower.starts_with("www.")
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

pub fn copy_text(text: &str) -> AppResult<()> {
    let mut cb = arboard::Clipboard::new().map_err(|e| AppError::Clipboard(e.to_string()))?;
    cb.set_text(text).map_err(|e| AppError::Clipboard(e.to_string()))?;
    Ok(())
}

pub fn copy_image_bytes(png: &[u8]) -> AppResult<()> {
    let img = image::load_from_memory(png)
        .map_err(|e| AppError::Msg(format!("图片解码失败: {e}")))?
        .to_rgba8();
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
