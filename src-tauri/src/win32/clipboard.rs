//! 剪贴板里「arboard 办不到」的三件事：文件列表（CF_HDROP）、富文本（HTML Format），
//! 以及它们的读取侧。
//!
//! arboard 只支持纯文本和图片。少了这些，从面板里点一条「3 个文件」的记录，
//! 粘到资源管理器里会变成三行路径文本；点一条从网页复制的表格，粘到 Word 里
//! 会丢掉所有格式 —— 用户看到的都是「这软件粘贴不对劲」。
//!
//! 两个格式的细节：
//!   - **CF_HDROP**：DROPFILES 结构（20 字节）+ 双 `\0` 结尾的 UTF-16 路径表；
//!   - **HTML Format**：一段 UTF-8 文本，头部 4 个字节偏移 + 一段 HTML。
//!     偏移是**字节**偏移，从字符串起点算，必须用固定宽度写完再回填，
//!     否则数字位数一变，前面所有偏移全部作废。

use windows::core::PCWSTR;
use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RegisterClipboardFormatW,
    SetClipboardData,
};
use windows::Win32::System::Memory::{
    GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock, GMEM_MOVEABLE,
};

/// CF_HDROP。不用 windows crate 的常量是因为它在 Win32_System_Ole 特性里，
/// 为了一个数字多编一个模块不划算。
const CF_HDROP: u32 = 15;
/// CF_UNICODETEXT
const CF_UNICODETEXT: u32 = 13;

/// DROPFILES 头部大小：pFiles(4) + pt(8) + fNC(4) + fWide(4)
const HEADER_LEN: usize = 20;

/// 一段 HTML 超过这个长度就不往历史里存。
///
/// 复制一整个网页源码能轻松到几 MB，索引文件会被撑爆；而真正需要保留格式的
/// 场景（网页表格、邮件正文、富文本片段）大多在几 KB 到几十 KB。
const HTML_MAX: usize = 256 * 1024;

/* ---------------- 公共入口 ---------------- */

pub fn set_files(paths: &[String]) -> Result<(), String> {
    if paths.is_empty() {
        return Err("没有文件".into());
    }

    let mut wide: Vec<u16> = Vec::new();
    for p in paths {
        wide.extend(p.encode_utf16());
        wide.push(0);
    }
    wide.push(0); // 整表以双 \0 结束

    let total = HEADER_LEN + wide.len() * 2;

    with_clipboard(|_| unsafe {
        EmptyClipboard().map_err(|e| format!("清空剪贴板失败: {e}"))?;

        let hmem = GlobalAlloc(GMEM_MOVEABLE, total).map_err(|e| format!("分配内存失败: {e}"))?;
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            return Err("锁定内存失败".into());
        }

        let buf = std::slice::from_raw_parts_mut(ptr as *mut u8, total);
        buf[0..4].copy_from_slice(&(HEADER_LEN as u32).to_le_bytes());
        buf[4..12].fill(0); // pt.x / pt.y
        buf[12..16].fill(0); // fNC = FALSE
        buf[16..20].copy_from_slice(&1u32.to_le_bytes()); // fWide = TRUE
        for (i, unit) in wide.iter().enumerate() {
            let off = HEADER_LEN + i * 2;
            buf[off..off + 2].copy_from_slice(&unit.to_le_bytes());
        }
        let _ = GlobalUnlock(hmem);

        // 成功后内存归系统所有，不能再 free
        SetClipboardData(CF_HDROP, Some(HANDLE(hmem.0)))
            .map_err(|e| format!("写入剪贴板失败: {e}"))?;
        Ok(())
    })
}

/// 同时写入纯文本和 HTML。
///
/// **纯文本必须一起写**：记事本、终端、搜索框这类地方只认 CF_UNICODETEXT，
/// 只写 HTML 的话它们会认为剪贴板是空的。
pub fn set_text_html(text: &str, html: &str) -> Result<(), String> {
    let doc = build_html_clipboard(html);

    with_clipboard(|_| unsafe {
        EmptyClipboard().map_err(|e| format!("清空剪贴板失败: {e}"))?;

        put_unicode_text(text)?;
        put_bytes(html_format(), doc.as_bytes())?;
        Ok(())
    })
}

/// 读剪贴板里的 HTML 片段。
///
/// 返回的是 `<!--StartFragment-->` 之间那一段，也就是"用户真正复制的那部分"，
/// 不是一个完整网页。没有 HTML 时返回 `None` —— 从记事本复制的东西本来就没有。
pub fn read_html() -> Option<String> {
    let mut out = None;
    let _ = with_clipboard(|_| unsafe {
        let fmt = html_format();
        if fmt == 0 {
            return Ok(());
        }
        let handle = match GetClipboardData(fmt) {
            Ok(h) if !h.is_invalid() => h,
            _ => return Ok(()),
        };
        let hmem = windows::Win32::Foundation::HGLOBAL(handle.0);
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            return Ok(());
        }
        let size = GlobalSize(hmem);
        let bytes = std::slice::from_raw_parts(ptr as *const u8, size);
        // 尾部可能带 \0，按第一个 \0 截断
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(size);
        let raw = String::from_utf8_lossy(&bytes[..end]).to_string();
        let _ = GlobalUnlock(hmem);

        if let Some(frag) = extract_fragment(&raw) {
            if !frag.is_empty() && frag.len() <= HTML_MAX {
                out = Some(frag);
            }
        }
        Ok(())
    });
    out
}

/* ---------------- CF_HTML 编解码 ---------------- */

fn html_format() -> u32 {
    let name: Vec<u16> = "HTML Format\0".encode_utf16().collect();
    unsafe { RegisterClipboardFormatW(PCWSTR(name.as_ptr())) }
}

/// 头部四个偏移，每个固定 10 位十进制。
///
/// 固定宽度是必须的：偏移值写在头部，而头部的长度又参与偏移计算。
/// 位数一变（比如 99999 → 100000），前面所有偏移就全错了。
fn header(start_html: usize, end_html: usize, start_frag: usize, end_frag: usize) -> String {
    format!(
        "Version:0.9\r\n\
         StartHTML:{start_html:010}\r\n\
         EndHTML:{end_html:010}\r\n\
         StartFragment:{start_frag:010}\r\n\
         EndFragment:{end_frag:010}\r\n"
    )
}

const PRE: &str = "<html><body>\r\n<!--StartFragment-->";
const POST: &str = "<!--EndFragment-->\r\n</body></html>";

fn build_html_clipboard(fragment: &str) -> String {
    // 先用 0 生成一份，长度就是头部的真实字节数
    let hlen = header(0, 0, 0, 0).len();

    let start_frag = hlen + PRE.len();
    let end_frag = start_frag + fragment.len();
    let end_html = end_frag + POST.len();

    let mut out = header(hlen, end_html, start_frag, end_frag);
    out.push_str(PRE);
    out.push_str(fragment);
    out.push_str(POST);
    out
}

fn extract_fragment(raw: &str) -> Option<String> {
    let start = header_offset(raw, "StartFragment:")?;
    let end = header_offset(raw, "EndFragment:")?;
    if start >= end || end > raw.len() {
        return None;
    }
    // 用 str::get 而不是切片：偏移来自外部程序，不保证落在 UTF-8 边界上
    raw.get(start..end).map(|s| s.trim().to_string())
}

fn header_offset(raw: &str, key: &str) -> Option<usize> {
    let idx = raw.find(key)?;
    let rest = &raw[idx + key.len()..];
    rest.lines().next()?.trim().parse().ok()
}

/* ---------------- 底层小工具 ---------------- */

/// 打开剪贴板 → 做事 → 一定关闭。
///
/// 剪贴板是全局独占资源，别的程序（截图工具、输入法、Office）随时可能正在用，
/// 所以打开失败要重试几次 —— 只试一次的话，用户点了"粘贴"什么都没发生，
/// 而且没有任何提示。
fn with_clipboard(f: impl FnOnce(()) -> Result<(), String>) -> Result<(), String> {
    for attempt in 0..8 {
        let opened = unsafe { OpenClipboard(None) };
        if opened.is_ok() {
            let r = f(());
            let _ = unsafe { CloseClipboard() };
            return r;
        }
        if attempt < 7 {
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
    }
    Err("剪贴板正被其它程序占用".into())
}

fn put_unicode_text(text: &str) -> Result<(), String> {
    let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
    let mut bytes = Vec::with_capacity(wide.len() * 2);
    for u in wide {
        bytes.extend_from_slice(&u.to_le_bytes());
    }
    put_bytes(CF_UNICODETEXT, &bytes)
}

fn put_bytes(format: u32, bytes: &[u8]) -> Result<(), String> {
    if format == 0 {
        return Err("注册剪贴板格式失败".into());
    }
    let len = bytes.len() + 1; // 末尾补一个 \0，部分老程序会读到结尾
    unsafe {
        let hmem = GlobalAlloc(GMEM_MOVEABLE, len).map_err(|e| format!("分配内存失败: {e}"))?;
        let ptr = GlobalLock(hmem);
        if ptr.is_null() {
            return Err("锁定内存失败".into());
        }
        let buf = std::slice::from_raw_parts_mut(ptr as *mut u8, len);
        buf[..bytes.len()].copy_from_slice(bytes);
        buf[bytes.len()] = 0;
        let _ = GlobalUnlock(hmem);

        // 成功后内存归系统所有，不能再 free
        SetClipboardData(format, Some(HANDLE(hmem.0)))
            .map_err(|e| format!("写入剪贴板失败: {e}"))?;
    }
    Ok(())
}
