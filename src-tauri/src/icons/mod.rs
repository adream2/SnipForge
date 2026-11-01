//! 应用图标提取。
//!
//! 支持四类来源，各自的技术门槛完全不同：
//!
//! | 来源 | 做法 | 坑 |
//! |---|---|---|
//! | `.exe` / `.dll` | 读 PE 里的 `RT_GROUP_ICON` / `RT_ICON` 资源 | 一个图标组含多档尺寸，必须逐档解出来 |
//! | `.lnk` | `IShellLinkW` 解目标 → 再走上面那条 | 目标可能是文档/文件夹，得退回系统图标 |
//! | `.apk` | 解 zip，读 `AndroidManifest.xml`（二进制 AXML） | 8.0+ 用 adaptive-icon，真图标在它引用的 drawable 里 |
//! | `.ipa` | 解 zip，扫 `Payload/*.app/` 里的 AppIcon | 二进制 plist 不好解，按文件名匹配更稳 |
//!
//! 统一产出「一组 PNG data URL + 每个的原始尺寸」，前端只管展示和保存。
//!
//! 设计上刻意**不依赖任何外部工具**（不调 ffmpeg / ImageMagick / aapt）：
//! 图标提取是个小功能，不该为了它让安装包多几十 MB。

pub mod mobile;
pub mod windows;

use crate::error::{AppError, AppResult};
use std::path::Path;

use base64::Engine;
use serde::Serialize;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// 一次最多返回多少个图标。
///
/// APK 里常有十几个密度分档，全塞给前端既慢又没用 ——
/// 用户要的是"最好的那个"和"几个备选"。
const MAX_ICONS: usize = 16;

#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IconAsset {
    /// `data:image/png;base64,...`
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    /// PNG 字节数（不含 base64 膨胀）
    pub bytes: usize,
    /// 这个图标在源文件里的位置（资源 ID / zip 路径），排查用
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IconBundle {
    /// exe / dll / lnk / apk / ipa / ico / image
    pub kind: String,
    /// 显示名：文件名去掉扩展名，快捷方式取 .lnk 自己的名字
    pub name: String,
    pub path: String,
    pub icons: Vec<IconAsset>,
    /// 给用户看的提示（"该文件有 3 个图标组，已选最完整的那个"）
    pub notes: Vec<String>,
}

/// 一组解出来的原始图标（还没编码成 PNG）
pub struct RawIcon {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub source: String,
}

impl RawIcon {
    fn into_asset(self) -> AppResult<IconAsset> {
        let png = encode_png(self.width, self.height, &self.rgba)?;
        Ok(IconAsset {
            data_url: format!("data:image/png;base64,{}", B64.encode(&png)),
            width: self.width,
            height: self.height,
            bytes: png.len(),
            source: self.source,
        })
    }
}

fn encode_png(w: u32, h: u32, rgba: &[u8]) -> AppResult<Vec<u8>> {
    let img = image::RgbaImage::from_raw(w, h, rgba.to_vec())
        .ok_or_else(|| AppError::Msg(format!("图标像素数量不匹配（{w}×{h}）")))?;
    let mut out = std::io::Cursor::new(Vec::new());
    img.write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| AppError::Msg(format!("PNG 编码失败: {e}")))?;
    Ok(out.into_inner())
}

/// 按尺寸降序、按内容去重，截到上限
pub fn finish(mut icons: Vec<RawIcon>) -> Vec<IconAsset> {
    // 先按面积降序：用户最想要的永远是最清晰的那个
    icons.sort_by(|a, b| (b.width * b.height).cmp(&(a.width * a.height)));

    let mut seen: Vec<u64> = Vec::new();
    let mut out = Vec::new();
    for icon in icons {
        if icon.width < 8 || icon.height < 8 {
            continue;
        }
        let h = cheap_hash(&icon.rgba);
        if seen.contains(&h) {
            continue;
        }
        seen.push(h);
        match icon.into_asset() {
            Ok(a) => out.push(a),
            Err(e) => log::warn!("图标编码失败，跳过: {e}"),
        }
        if out.len() >= MAX_ICONS {
            break;
        }
    }
    out
}

/// 内容指纹。只用来去重，不需要抗碰撞强度。
fn cheap_hash(data: &[u8]) -> u64 {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let d = hasher.finalize();
    u64::from_le_bytes(d[..8].try_into().unwrap_or([0; 8]))
}

/// 从任意受支持的文件里提取图标。
pub fn extract(path: &Path) -> AppResult<IconBundle> {
    if !path.exists() {
        return Err(AppError::Msg("文件不存在".into()));
    }
    if path.is_dir() {
        return Err(AppError::Msg(
            "这是一个文件夹。请选择 .exe / .dll / .lnk / .apk / .ipa / .ico 文件".into(),
        ));
    }

    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "icon".into());
    let path_text = path.to_string_lossy().to_string();

    let mut notes = Vec::new();
    let (kind, raw) = match ext.as_str() {
        "exe" | "dll" | "ocx" | "cpl" | "scr" | "sys" => {
            ("exe".to_string(), windows::from_pe(path, &mut notes)?)
        }
        "lnk" | "url" => ("lnk".to_string(), windows::from_lnk(path, &mut notes)?),
        "apk" | "aab" => ("apk".to_string(), mobile::from_apk(path, &mut notes)?),
        "ipa" => ("ipa".to_string(), mobile::from_ipa(path, &mut notes)?),
        "ico" => ("ico".to_string(), from_ico_file(path, &mut notes)?),
        "png" | "jpg" | "jpeg" | "webp" | "bmp" => {
            ("image".to_string(), from_image_file(path, &mut notes)?)
        }
        _ => {
            // 没扩展名或扩展名不认识：按魔数猜
            guess_by_magic(path, &mut notes)?
        }
    };

    if raw.is_empty() {
        return Err(AppError::Msg(format!(
            "这个文件里没有找到图标（{}）",
            if notes.is_empty() {
                "可能是纯代码文件 / 图标已被剥离 / 格式不受支持".to_string()
            } else {
                notes.join("；")
            }
        )));
    }

    let icons = finish(raw);
    if icons.is_empty() {
        return Err(AppError::Msg("找到了图标但都无法解码".into()));
    }

    let max = icons
        .first()
        .map(|i| format!("最大 {}×{}", i.width, i.height))
        .unwrap_or_default();
    notes.push(format!("{} 个图标，{max}", icons.len()));

    Ok(IconBundle {
        kind,
        name: stem,
        path: path_text,
        icons,
        notes,
    })
}

/// 按魔数判断类型（给没有正确扩展名的文件兜底）
fn guess_by_magic(path: &Path, notes: &mut Vec<String>) -> AppResult<(String, Vec<RawIcon>)> {
    let head = read_head(path, 8)?;
    if head.starts_with(b"MZ") {
        notes.push("按 PE 文件处理".into());
        return Ok(("exe".into(), windows::from_pe(path, notes)?));
    }
    if head.starts_with(b"PK\x03\x04") {
        notes.push("按 zip 容器处理（APK / IPA）".into());
        // 用 zip 里的内容判断是哪一种
        return Ok(("apk".into(), mobile::from_zip_auto(path, notes)?));
    }
    if head.starts_with(&[0x00, 0x00, 0x01, 0x00]) {
        notes.push("按 ICO 文件处理".into());
        return Ok(("ico".into(), from_ico_file(path, notes)?));
    }
    if head.starts_with(&[0x89, b'P', b'N', b'G']) {
        notes.push("按 PNG 处理".into());
        return Ok(("image".into(), from_image_file(path, notes)?));
    }
    Err(AppError::Msg(
        "不认识这个文件类型。支持 .exe .dll .lnk .apk .ipa .ico 和常见图片".into(),
    ))
}

fn read_head(path: &Path, n: usize) -> AppResult<Vec<u8>> {
    use std::io::Read;
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; n];
    let read = f.read(&mut buf)?;
    buf.truncate(read);
    Ok(buf)
}

/// 直接读图片文件（用户可能只是想把它转成 PNG 统一格式）
fn from_image_file(path: &Path, notes: &mut Vec<String>) -> AppResult<Vec<RawIcon>> {
    let data = std::fs::read(path)?;
    let img = image::load_from_memory(&data)
        .map_err(|e| AppError::Msg(format!("图片解析失败: {e}")))?;
    let rgba = img.to_rgba8();
    notes.push(format!("原图 {}×{}", rgba.width(), rgba.height()));
    Ok(vec![RawIcon {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
        source: "原图".into(),
    }])
}

/* ---------------- ICO 容器 ---------------- */

/// ICO 目录项
struct IcoEntry {
    width: u32,
    height: u32,
    bpp: u16,
    bytes: usize,
    offset: usize,
}

fn parse_ico_dir(data: &[u8]) -> Option<Vec<IcoEntry>> {
    if data.len() < 6 {
        return None;
    }
    let reserved = u16::from_le_bytes([data[0], data[1]]);
    let kind = u16::from_le_bytes([data[2], data[3]]);
    let count = u16::from_le_bytes([data[4], data[5]]) as usize;
    // 0 = ICO，1 = CUR。reserved 必须是 0
    if reserved != 0 || kind > 1 || count == 0 || count > 512 {
        return None;
    }
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        let base = 6 + i * 16;
        if base + 16 > data.len() {
            return None;
        }
        let w = data[base] as u32;
        let h = data[base + 1] as u32;
        let bpp = u16::from_le_bytes([data[base + 6], data[base + 7]]);
        let bytes = u32::from_le_bytes([
            data[base + 8],
            data[base + 9],
            data[base + 10],
            data[base + 11],
        ]) as usize;
        let offset = u32::from_le_bytes([
            data[base + 12],
            data[base + 13],
            data[base + 14],
            data[base + 15],
        ]) as usize;
        if offset + bytes > data.len() {
            continue;
        }
        out.push(IcoEntry {
            // 目录里 0 表示 256（一个字节存不下 256）
            width: if w == 0 { 256 } else { w },
            height: if h == 0 { 256 } else { h },
            bpp,
            bytes,
            offset,
        });
    }
    Some(out)
}

/// 把一帧 ICO 图像数据解成 RGBA。
///
/// ICO 里的单帧只有两种形态：
///   1. **PNG**（Vista 起用于 256×256）—— 直接交给 image 解；
///   2. **DIB**（`BITMAPINFOHEADER` + 调色板 + 像素 + AND 掩码）—— 自己解。
///
/// 为什么不把 DIB 也丢给 image 的 ICO 解码器：
///   它的实现要求数据长度完全对得上，而 PE 资源里的 DIB 常常是
///   4bpp/1bpp 的索引图，会报 `failed to fill whole buffer`。
///   自己解这几十行反而更稳，而且能正确处理 AND 掩码（透明通道）。
pub fn decode_ico_frame(raw: &[u8], w: u32, h: u32, _bpp: u16) -> AppResult<Vec<u8>> {
    if raw.starts_with(&[0x89, b'P', b'N', b'G']) {
        let img = image::load_from_memory(raw)
            .map_err(|e| AppError::Msg(format!("内嵌 PNG 解码失败: {e}")))?;
        return Ok(img.to_rgba8().into_raw());
    }
    decode_dib(raw).map(|(_, _, rgba)| rgba).ok_or_else(|| {
        AppError::Msg(format!("DIB 解析失败（声明尺寸 {w}×{h}）"))
    })
}

/// 解 ICO 内嵌的 DIB（设备无关位图）。
///
/// 几个必须处理对的点：
///   - `biHeight` 是**图像高度的两倍**（下面还跟着 AND 掩码），直接用会读越界；
///   - 像素是 **bottom-up**（第一行在缓冲区末尾）；
///   - 每行按 4 字节对齐；
///   - 32bpp 的 alpha 有时全是 0（旧式图标），这时要靠 AND 掩码补透明。
fn decode_dib(data: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    if data.len() < 40 {
        return None;
    }
    let bi_size = u32::from_le_bytes(data[0..4].try_into().ok()?) as usize;
    let bi_width = i32::from_le_bytes(data[4..8].try_into().ok()?);
    let bi_height = i32::from_le_bytes(data[8..12].try_into().ok()?);
    let bit_count = u16::from_le_bytes(data[14..16].try_into().ok()?);
    let compression = u32::from_le_bytes(data[16..20].try_into().ok()?);
    let clr_used = u32::from_le_bytes(data[32..36].try_into().ok()?);

    // 只支持未压缩的真彩/索引位图，RLE 压缩在图标里几乎见不到
    if compression != 0 || bi_width <= 0 {
        return None;
    }
    let w = bi_width as u32;
    let h = (bi_height.unsigned_abs() / 2).max(1);

    let palette_len = if bit_count <= 8 {
        let n = if clr_used > 0 {
            clr_used
        } else {
            1u32 << bit_count
        };
        n as usize * 4
    } else {
        0
    };

    let pixels_off = bi_size + palette_len;
    let row = |bits: u32| ((w * bits + 31) / 32 * 4) as usize;
    let stride = row(bit_count as u32);
    let mask_stride = row(1);
    let need = pixels_off + stride * h as usize;
    if data.len() < need {
        return None;
    }

    let mut out = vec![0u8; (w * h * 4) as usize];
    let palette = &data[bi_size..bi_size + palette_len];
    let has_alpha_byte = bit_count == 32;

    for y in 0..h as usize {
        // bottom-up：文件里第一行是图像的最后一行
        let src_y = h as usize - 1 - y;
        let line = &data[pixels_off + src_y * stride..pixels_off + (src_y + 1) * stride];
        for x in 0..w as usize {
            let (b, g, r, a) = match bit_count {
                32 => {
                    let p = x * 4;
                    (line[p], line[p + 1], line[p + 2], line[p + 3])
                }
                24 => {
                    let p = x * 3;
                    (line[p], line[p + 1], line[p + 2], 255)
                }
                8 => {
                    let idx = line[x] as usize * 4;
                    if idx + 2 >= palette.len() {
                        (0, 0, 0, 255)
                    } else {
                        (palette[idx], palette[idx + 1], palette[idx + 2], 255)
                    }
                }
                4 => {
                    let byte = line[x / 2];
                    let v = if x % 2 == 0 { byte >> 4 } else { byte & 0x0F };
                    let idx = v as usize * 4;
                    if idx + 2 >= palette.len() {
                        (0, 0, 0, 255)
                    } else {
                        (palette[idx], palette[idx + 1], palette[idx + 2], 255)
                    }
                }
                1 => {
                    let byte = line[x / 8];
                    let bit = (byte >> (7 - (x % 8))) & 1;
                    let idx = bit as usize * 4;
                    if idx + 2 >= palette.len() {
                        (0, 0, 0, 255)
                    } else {
                        (palette[idx], palette[idx + 1], palette[idx + 2], 255)
                    }
                }
                _ => return None,
            };
            let o = (y * w as usize + x) * 4;
            out[o] = r;
            out[o + 1] = g;
            out[o + 2] = b;
            out[o + 3] = a;
        }
    }

    // ---- AND 掩码：1 = 透明 ----
    let mask_off = pixels_off + stride * h as usize;
    let mask_available = data.len() >= mask_off + mask_stride * h as usize;
    let alpha_all_zero = has_alpha_byte
        && out
            .chunks_exact(4)
            .all(|p| p[3] == 0);

    if mask_available && (alpha_all_zero || !has_alpha_byte) {
        for y in 0..h as usize {
            // 掩码和像素一样是 bottom-up
            let src_y = h as usize - 1 - y;
            let line = &data[mask_off + src_y * mask_stride..mask_off + (src_y + 1) * mask_stride];
            for x in 0..w as usize {
                let bit = (line[x / 8] >> (7 - (x % 8))) & 1;
                let o = (y * w as usize + x) * 4 + 3;
                out[o] = if bit == 1 { 0 } else { 255 };
            }
        }
    } else if alpha_all_zero {
        // 既没有 alpha 也没有掩码：当成完全不透明，至少图是能看的
        for p in out.chunks_exact_mut(4) {
            p[3] = 255;
        }
    }

    Some((w, h, out))
}

fn from_ico_file(path: &Path, notes: &mut Vec<String>) -> AppResult<Vec<RawIcon>> {
    let data = std::fs::read(path)?;
    let entries = parse_ico_dir(&data)
        .ok_or_else(|| AppError::Msg("这不是有效的 ICO 文件".into()))?;
    notes.push(format!("ICO 内含 {} 帧", entries.len()));

    let mut out = Vec::new();
    for (i, e) in entries.iter().enumerate() {
        let raw = &data[e.offset..e.offset + e.bytes];
        match decode_ico_frame(raw, e.width, e.height, e.bpp) {
            Ok(rgba) => out.push(RawIcon {
                width: e.width,
                height: e.height,
                rgba,
                source: format!("第 {} 帧 {}×{}/{}bpp", i + 1, e.width, e.height, e.bpp),
            }),
            Err(e2) => log::debug!("ICO 第 {i} 帧解码失败: {e2}"),
        }
    }
    Ok(out)
}
