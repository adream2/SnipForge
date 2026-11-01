//! Windows 侧图标来源：PE 资源（exe / dll）与快捷方式（.lnk）。
//!
//! 为什么不用 `ExtractIconExW`：
//!   它只给系统设定的那一档（16 或 32 像素），而用户要的往往是 256×256。
//!   PE 里的 `RT_GROUP_ICON` 才是原始的多档图标组，一档不缺。
//!
//! 为什么用 `LOAD_LIBRARY_AS_DATAFILE`：
//!   这个标志**不执行 DllMain、不解析导入表**，只是把文件当资源容器映射进来，
//!   所以对来路不明的 DLL 也是安全的。普通 `LoadLibrary` 会跑别人的初始化代码。

use super::{decode_ico_frame, RawIcon};
use crate::error::{AppError, AppResult};
use std::path::Path;

use windows::core::{Interface, BOOL, PCWSTR};
use windows::Win32::Foundation::{FreeLibrary, HMODULE};
use windows::Win32::Graphics::Gdi::{
    DeleteObject, GetDC, GetDIBits, GetObjectW, ReleaseDC, BITMAP, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HGDIOBJ,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, STGM_READ,
};
use windows::Win32::System::LibraryLoader::{
    EnumResourceNamesW, FindResourceW, LoadLibraryExW, LoadResource, LockResource, SizeofResource,
    LOAD_LIBRARY_AS_DATAFILE,
};
use windows::Win32::UI::Shell::{
    IShellLinkW, SHGetFileInfoW, ShellLink, SHFILEINFOW, SHGFI_ICON, SHGFI_LARGEICON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyIcon, GetIconInfo, ICONINFO, RT_GROUP_ICON, RT_ICON,
};

fn wide(s: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

/// 资源名的高 16 位为 0 表示它是整数 ID 而不是字符串名
fn is_int_resource(name: PCWSTR) -> bool {
    (name.0 as usize) >> 16 == 0
}

fn int_resource(id: isize) -> PCWSTR {
    PCWSTR(id as usize as *const u16)
}

/* ---------------- PE 资源 ---------------- */

/// `GRPICONDIRENTRY`：14 字节，比 ICO 目录项少一个 4 字节偏移（资源里不需要偏移）
struct GroupEntry {
    width: u32,
    height: u32,
    bpp: u16,
    id: u16,
}

fn parse_group(data: &[u8]) -> Vec<GroupEntry> {
    if data.len() < 6 {
        return Vec::new();
    }
    let count = u16::from_le_bytes([data[4], data[5]]) as usize;
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let b = 6 + i * 14;
        if b + 14 > data.len() {
            break;
        }
        let w = data[b] as u32;
        let h = data[b + 1] as u32;
        // GRPICONDIRENTRY 布局（14 字节）：
        //   [0]bWidth [1]bHeight [2]bColorCount [3]bReserved
        //   [4..6]wPlanes [6..8]wBitCount [8..12]dwBytesInRes [12..14]nID
        // 位深在 +6，不是 +8（+8 是字节数，写成 +8 会得到 1640 这种"位深"）
        let bpp = u16::from_le_bytes([data[b + 6], data[b + 7]]);
        let id = u16::from_le_bytes([data[b + 12], data[b + 13]]);
        out.push(GroupEntry {
            // 目录里 0 表示 256
            width: if w == 0 { 256 } else { w },
            height: if h == 0 { 256 } else { h },
            bpp,
            id,
        });
    }
    out
}

unsafe extern "system" fn enum_group_cb(
    _module: HMODULE,
    _kind: PCWSTR,
    name: PCWSTR,
    lparam: isize,
) -> BOOL {
    let ids = &mut *(lparam as *mut Vec<isize>);
    if is_int_resource(name) {
        ids.push(name.0 as isize);
    }
    BOOL(1) // 继续枚举
}

unsafe fn read_resource(module: HMODULE, kind: PCWSTR, id: isize) -> Option<Vec<u8>> {
    let res = FindResourceW(Some(module), int_resource(id), kind);
    if res.is_invalid() {
        return None;
    }
    let handle = LoadResource(Some(module), res).ok()?;
    let ptr = LockResource(handle) as *const u8;
    let size = SizeofResource(Some(module), res) as usize;
    if ptr.is_null() || size == 0 {
        return None;
    }
    Some(std::slice::from_raw_parts(ptr, size).to_vec())
}

/// 从一个 exe / dll 里读出所有图标组的成员图。
///
/// 有些 DLL 里塞了十几个图标组（工具栏小图标、状态图标…），
/// 所以挑「最大尺寸最大」的那一组当应用图标 —— 主图标通常带 256×256，
/// 而工具栏图标最多到 32。
pub fn from_pe(path: &Path, notes: &mut Vec<String>) -> AppResult<Vec<RawIcon>> {
    let wpath = wide(&path.to_string_lossy());

    unsafe {
        let module = LoadLibraryExW(
            PCWSTR(wpath.as_ptr()),
            None,
            LOAD_LIBRARY_AS_DATAFILE,
        )
        .map_err(|e| {
            AppError::Msg(format!(
                "无法读取该文件（32 位 DLL 也可能失败，或被安全软件拦截）: {e}"
            ))
        })?;

        let result = (|| -> AppResult<Vec<RawIcon>> {
            let mut ids: Vec<isize> = Vec::new();
            let _ = EnumResourceNamesW(
                Some(module),
                RT_GROUP_ICON,
                Some(enum_group_cb),
                &mut ids as *mut Vec<isize> as isize,
            );

            if ids.is_empty() {
                // 也可能是纯资源 DLL 只放了 RT_ICON 没有组，这里直接放弃
                notes.push("这个文件里没有图标资源".into());
                return Ok(Vec::new());
            }
            if ids.len() > 1 {
                notes.push(format!("含 {} 个图标组，已选最完整的那个", ids.len()));
            }

            // 选「最大尺寸最大」的组
            let mut best: Option<(u32, Vec<GroupEntry>)> = None;
            for id in &ids {
                let Some(data) = read_resource(module, RT_GROUP_ICON, *id) else {
                    continue;
                };
                let entries = parse_group(&data);
                let max = entries
                    .iter()
                    .map(|e| e.width * e.height)
                    .max()
                    .unwrap_or(0);
                if best.as_ref().map(|(m, _)| max > *m).unwrap_or(true) {
                    best = Some((max, entries));
                }
            }

            let Some((_, entries)) = best else {
                return Ok(Vec::new());
            };

            let mut out = Vec::new();
            // 同一尺寸常常有 4bpp / 8bpp / 32bpp 三份，全给用户看只会让人困惑，
            // 而且 4bpp 那份没有抗锯齿、边缘是锯齿状的。只留位深最高的那一帧。
            let mut best_per_size: std::collections::BTreeMap<(u32, u32), (u16, u16)> =
                std::collections::BTreeMap::new();
            for e in &entries {
                let key = (e.width, e.height);
                let better = match best_per_size.get(&key) {
                    Some((bpp, _)) => e.bpp > *bpp,
                    None => true,
                };
                if better {
                    best_per_size.insert(key, (e.bpp, e.id));
                }
            }
            if best_per_size.len() < entries.len() {
                notes.push(format!(
                    "同尺寸保留最清晰的一档（{} → {} 个）",
                    entries.len(),
                    best_per_size.len()
                ));
            }

            for ((w, h), (bpp, id)) in &best_per_size {
                let Some(raw) = read_resource(module, RT_ICON, *id as isize) else {
                    notes.push(format!("RT_ICON #{id} 读不到"));
                    continue;
                };
                match decode_ico_frame(&raw, *w, *h, *bpp) {
                    Ok(rgba) => out.push(RawIcon {
                        width: *w,
                        height: *h,
                        rgba,
                        source: format!("RT_ICON #{id} {w}×{h}/{bpp}bpp"),
                    }),
                    Err(err) => {
                        log::debug!("图标 {w}×{h} 解码失败: {err}");
                        // 解码失败的原因要能被看见，否则用户只会得到一句"没找到图标"
                        let shown = notes.iter().filter(|n| n.contains("解码失败")).count();
                        if shown < 3 {
                            notes.push(format!("{w}×{h}/{bpp}bpp 解码失败: {err}"));
                        }
                    }
                }
            }
            Ok(out)
        })();

        let _ = FreeLibrary(module);
        result
    }
}

/* ---------------- 快捷方式 ---------------- */

pub fn from_lnk(path: &Path, notes: &mut Vec<String>) -> AppResult<Vec<RawIcon>> {
    unsafe {
        // .lnk 解析走 Shell 的 STA COM，忽略"已经初始化过"的错误
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);

        let link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|e| AppError::Msg(format!("无法解析快捷方式: {e}")))?;
        let file: windows::Win32::System::Com::IPersistFile = link
            .cast()
            .map_err(|e| AppError::Msg(format!("快捷方式接口不匹配: {e}")))?;

        let wpath = wide(&path.to_string_lossy());
        file.Load(PCWSTR(wpath.as_ptr()), STGM_READ)
            .map_err(|e| AppError::Msg(format!("读取快捷方式失败: {e}")))?;

        let mut buf = vec![0u16; 2048];
        let target = match link.GetPath(&mut buf, std::ptr::null_mut(), 0) {
            Ok(()) => {
                let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
                String::from_utf16_lossy(&buf[..end])
            }
            Err(_) => String::new(),
        };

        if !target.is_empty() {
            let tp = Path::new(&target);
            let ext = tp
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            // 目标是可执行文件 / 图标文件：走多尺寸提取，效果最好
            if matches!(ext.as_str(), "exe" | "dll" | "ocx" | "cpl" | "scr" | "ico")
                && tp.exists()
            {
                notes.push(format!("目标：{}", tp.file_name().unwrap_or_default().to_string_lossy()));
                let icons = if ext == "ico" {
                    super::from_ico_file(tp, notes)?
                } else {
                    from_pe(tp, notes)?
                };
                if !icons.is_empty() {
                    return Ok(icons);
                }
            } else if !tp.exists() {
                notes.push("快捷方式指向的目标已不存在".into());
            } else {
                notes.push("目标不是可执行文件，改用系统图标".into());
            }
        }

        // 兜底：让系统给一个 32×32 的关联图标（快捷方式指向文档/文件夹时走这里）
        let mut info = SHFILEINFOW::default();
        let ok = SHGetFileInfoW(
            PCWSTR(wpath.as_ptr()),
            Default::default(),
            Some(&mut info),
            std::mem::size_of::<SHFILEINFOW>() as u32,
            SHGFI_ICON | SHGFI_LARGEICON,
        );
        if ok == 0 || info.hIcon.is_invalid() {
            return Ok(Vec::new());
        }
        let icon = info.hIcon;
        let out = hicon_to_raw(icon).map(|r| vec![r]).unwrap_or_default();
        let _ = DestroyIcon(icon);
        Ok(out)
    }
}

/// HICON → RGBA。
///
/// 32 位图标自带 alpha；老式 24 位图标的 alpha 全是 0，得靠 AND 掩码补 ——
/// 不补的话整张图会变成全透明（用户看到一片空白，还以为是提取失败）。
unsafe fn hicon_to_raw(icon: windows::Win32::UI::WindowsAndMessaging::HICON) -> Option<RawIcon> {
    let mut ii = ICONINFO::default();
    GetIconInfo(icon, &mut ii).ok()?;

    let mut bm = BITMAP::default();
    let got = GetObjectW(
        HGDIOBJ(ii.hbmColor.0),
        std::mem::size_of::<BITMAP>() as i32,
        Some(&mut bm as *mut _ as *mut core::ffi::c_void),
    );
    if got == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
        let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
        let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
        return None;
    }
    let (w, h) = (bm.bmWidth as u32, bm.bmHeight as u32);

    let mut bi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: w as i32,
            // 负高度 = 请求自上而下的行序，省掉一次翻转
            biHeight: -(h as i32),
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut buf = vec![0u8; (w * h * 4) as usize];
    let dc = GetDC(None);
    let lines = GetDIBits(
        dc,
        ii.hbmColor,
        0,
        h,
        Some(buf.as_mut_ptr() as *mut core::ffi::c_void),
        &mut bi,
        DIB_RGB_COLORS,
    );
    ReleaseDC(None, dc);
    let _ = DeleteObject(HGDIOBJ(ii.hbmColor.0));
    let _ = DeleteObject(HGDIOBJ(ii.hbmMask.0));
    if lines == 0 {
        return None;
    }

    // BGRA → RGBA
    for px in buf.chunks_exact_mut(4) {
        px.swap(0, 2);
    }

    // alpha 全 0 = 旧式图标，补成不透明（能正常看不比精确透明重要）
    if buf.chunks_exact(4).all(|p| p[3] == 0) {
        for px in buf.chunks_exact_mut(4) {
            px[3] = 255;
        }
    }

    Some(RawIcon {
        width: w,
        height: h,
        rgba: buf,
        source: "系统关联图标".into(),
    })
}
