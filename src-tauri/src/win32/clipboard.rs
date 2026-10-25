//! 往系统剪贴板写文件列表（CF_HDROP）。
//!
//! arboard 只支持文本和图片，不提供文件列表 —— 但剪贴板的三大件里就数文件最麻烦。
//! 少了这个，从面板里点一条「3 个文件」的记录，粘到资源管理器里会变成三行路径文本，
//! 而不是三个文件。
//!
//! DROPFILES 结构（20 字节）+ 双 \0 结尾的 UTF-16 路径表，格式是固定的。

use windows::Win32::Foundation::HANDLE;
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};

/// CF_HDROP。不用 windows crate 的常量是因为它在 Win32_System_Ole 特性里，
/// 为了一个数字多编一个模块不划算。
const CF_HDROP: u32 = 15;

/// DROPFILES 头部大小：pFiles(4) + pt(8) + fNC(4) + fWide(4)
const HEADER_LEN: usize = 20;

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

    unsafe {
        OpenClipboard(None).map_err(|e| format!("打开剪贴板失败: {e}"))?;

        let result = (|| -> Result<(), String> {
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
        })();

        let _ = CloseClipboard();
        result
    }
}
