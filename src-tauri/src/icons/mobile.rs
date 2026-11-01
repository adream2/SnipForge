//! Android APK / iOS IPA 的图标提取。
//!
//! 两者都是 zip 包，差别只在"图标放在哪、叫什么"：
//!
//! **APK**：`res/mipmap-<density>/ic_launcher.png`，按屏幕密度分档
//! （mdpi / hdpi / xhdpi / xxhdpi / xxxhdpi）。Android 8+ 之后主图标变成
//! `adaptive-icon`：`mipmap-anydpi-v26/ic_launcher.xml` 里用
//! `<foreground android:drawable="@drawable/xxx"/>` 指向真正的图案 ——
//! 只扫 png 会拿到背景色块，这是很多提取工具"取出来一片纯色"的原因。
//!
//! **IPA**：`Payload/<名字>.app/AppIcon60x60@2x.png` 之类，文件名里带尺寸和倍率。
//!
//! 为什么不去解析 `AndroidManifest.xml` 的图标引用：
//!   它是二进制 AXML，`android:icon` 在编译后被换成 `resources.arsc` 里的
//!   资源 ID（形如 0x7f0e0000），要反查路径还得再实现一遍 arsc 解析。
//!   而 99% 的 APK 图标就叫 `ic_launcher` —— 按名字匹配 + 按密度排序，
//!   效果一样，代码量差一个数量级。找不到时再放宽到"整个 mipmap 目录"。

use super::RawIcon;
use crate::error::{AppError, AppResult};
use std::io::Read;
use std::path::Path;

use zip::ZipArchive;

/// 图标文件名里出现这些词，就认为它是应用图标
const ICON_HINTS: &[&str] = &["ic_launcher", "app_icon", "ic_app", "icon", "logo"];

fn is_image(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.ends_with(".png") || lower.ends_with(".webp") || lower.ends_with(".jpg")
}

/// 密度档 → 相对权重。数值越大越清晰，用来在**不解码**的前提下排序。
fn density_weight(path: &str) -> u32 {
    let p = path.to_ascii_lowercase();
    for (tag, w) in [
        ("xxxhdpi", 640),
        ("xxhdpi", 480),
        ("xhdpi", 320),
        ("hdpi", 240),
        ("mdpi", 160),
        ("ldpi", 120),
        ("anydpi", 700), // adaptive-icon 的 xml，单独处理
        ("nodpi", 100),
    ] {
        if p.contains(tag) {
            return w;
        }
    }
    200
}

/// 给 APK 里的路径打分：分越高越可能是应用图标
fn apk_score(path: &str) -> u32 {
    let lower = path.to_ascii_lowercase();
    if !lower.starts_with("res/") || !is_image(path) {
        return 0;
    }
    let fname = lower.rsplit('/').next().unwrap_or("");
    // 文件名必须命中提示词，否则大概率是插图 / 按钮背景
    let name_hit = ICON_HINTS.iter().any(|h| fname.contains(h));
    let dir_hit = lower.contains("/mipmap");

    let mut score: u32 = 0;
    if name_hit {
        score += 1000;
    }
    if dir_hit {
        score += 400;
    }
    if lower.contains("/drawable") {
        score += 100;
    }
    // 系统自适应图标的前景层往往就是"真正的那张画"
    if fname.contains("foreground") {
        score += 60;
    }
    // 纯色背景层不是图标
    if fname.contains("background") {
        score = score.saturating_sub(500);
    }
    score += density_weight(path) / 10;
    if score < 400 {
        0
    } else {
        score
    }
}

pub fn from_apk(path: &Path, notes: &mut Vec<String>) -> AppResult<Vec<RawIcon>> {
    let icons = scan_apk(path, notes)?;
    if icons.is_empty() {
        notes.push("没在 res/ 下找到图标（可能被加固 / 资源加密）".into());
    }
    Ok(icons)
}

fn open_zip(path: &Path) -> AppResult<ZipArchive<std::fs::File>> {
    let file = std::fs::File::open(path)?;
    ZipArchive::new(file).map_err(|e| {
        AppError::Msg(format!(
            "不是有效的 zip 容器（APK/IPA 都是 zip，文件可能损坏或加密）: {e}"
        ))
    })
}

fn scan_apk(path: &Path, notes: &mut Vec<String>) -> AppResult<Vec<RawIcon>> {
    let mut zip = open_zip(path)?;

    // ---- 1. 扫一遍目录：收集候选路径 + adaptive-icon 的 xml ----
    // 只读条目名不解压，几百个条目也就几微秒
    let mut cands: Vec<(u32, String)> = Vec::new();
    let mut adaptive: Vec<String> = Vec::new();
    for i in 0..zip.len() {
        let name = {
            let Ok(entry) = zip.by_index_raw(i) else {
                continue;
            };
            entry.name().to_string()
        };
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".xml") && lower.contains("anydpi") {
            adaptive.push(name.clone());
        }
        let score = apk_score(&name);
        if score > 0 {
            cands.push((score, name));
        }
    }

    // ---- 2. adaptive-icon 优先 ----
    //
    // 只把 foreground 拿出来是不够的：Android 8+ 的图标是「背景 + 前景」两层叠出来的，
    // 只给前景会得到一张透明底的图案（比如微信只剩白色气泡、没有绿底）。
    // 所以这里合成两层，并按 72/108 居中裁剪 —— 108dp 画布里只有中间 72dp 是可见区，
    // 不裁的话四周那圈安全边距会一起存下来，贴到别处就显得特别小。
    let mut special: Vec<RawIcon> = Vec::new();
    for xml in &adaptive {
        let Some((fg_name, bg_name)) = adaptive_layers(&mut zip, xml) else {
            continue;
        };
        let Some((fw, fh, fg_px)) = find_best_image(&mut zip, &fg_name) else {
            continue;
        };
        let bg = bg_name
            .as_deref()
            .and_then(|n| find_best_image(&mut zip, n));
        let has_bg = bg.is_some();
        let (cw, ch, composed) = compose_adaptive(fw, fh, &fg_px, bg.as_ref());
        notes.push(format!(
            "自适应图标已合成{}并裁掉安全边距（{cw}×{ch}）",
            if has_bg {
                "背景+前景"
            } else {
                "前景（未找到背景层）"
            }
        ));
        // 合成图 + 原始前景层都给：有人就是想要透明底的素材
        special.push(RawIcon {
            width: cw,
            height: ch,
            rgba: composed,
            source: format!("adaptive-icon 合成（{xml}）"),
        });
        special.push(RawIcon {
            width: fw,
            height: fh,
            rgba: fg_px,
            source: format!("adaptive-icon 前景层（{fg_name}）"),
        });
        break;
    }

    // ---- 3. 没有命中标准命名时，退到"整个 mipmap 目录" ----
    if cands.is_empty() && special.is_empty() {
        for i in 0..zip.len() {
            let name = {
                let Ok(e) = zip.by_index_raw(i) else { continue };
                e.name().to_string()
            };
            let lower = name.to_ascii_lowercase();
            if lower.starts_with("res/") && lower.contains("/mipmap") && is_image(&name) {
                cands.push((density_weight(&name), name));
            }
        }
        if !cands.is_empty() {
            notes.push("未见标准命名，已取 mipmap 目录下最清晰的几张".into());
        }
    }

    // 高分优先，同分按密度从高到低
    cands.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
    cands.dedup_by(|a, b| a.1 == b.1);

    // ---- 4. 解码候选（前 24 个足够了，同尺寸的会被去重）----
    let mut out = special;
    for (_, name) in cands.iter().take(24) {
        match read_zip_image(&mut zip, name) {
            Ok(Some((w, h, rgba))) => out.push(RawIcon {
                width: w,
                height: h,
                rgba,
                source: name.clone(),
            }),
            Ok(None) => log::debug!("跳过无法解码的图: {name}"),
            Err(e) => log::debug!("读取 {name} 失败: {e}"),
        }
    }
    Ok(out)
}

/// 按名字片段（不含扩展名）在 zip 里找最清晰的那张图。
///
/// 同一个 drawable 会按密度有 4~5 份，`find_best_image` 只解最高密度那份 ——
/// 解所有密度纯属浪费（最后也会被同尺寸去重掉）。
fn find_best_image(
    zip: &mut ZipArchive<std::fs::File>,
    fragment: &str,
) -> Option<(u32, u32, Vec<u8>)> {
    let want = fragment.to_ascii_lowercase();
    let mut best: Option<(u32, String)> = None;
    for i in 0..zip.len() {
        let name = {
            let Ok(e) = zip.by_index_raw(i) else { continue };
            e.name().to_string()
        };
        if !is_image(&name) {
            continue;
        }
        let lower = name.to_ascii_lowercase();
        // 精确到"目录/名字."，避免 ic_launcher 匹配到 ic_launcher_background
        let stem = want.trim_end_matches(".png").trim_end_matches(".webp");
        let hit = lower.contains(&format!("/{stem}."))
            || lower.contains(&format!("/{stem}@"))
            || lower.ends_with(&format!("/{stem}.png"));
        if !hit {
            continue;
        }
        let weight = density_weight(&name);
        if best.as_ref().map(|(w, _)| weight > *w).unwrap_or(true) {
            best = Some((weight, name));
        }
    }
    let (_, name) = best?;
    read_zip_image(zip, &name).ok().flatten()
}

/// 背景拉伸铺满 + 前景 alpha 叠加 + 居中裁掉安全边距
fn compose_adaptive(
    fw: u32,
    fh: u32,
    fg: &[u8],
    bg: Option<&(u32, u32, Vec<u8>)>,
) -> (u32, u32, Vec<u8>) {
    let mut canvas = vec![0u8; (fw * fh * 4) as usize];

    if let Some((bw, bh, bg_px)) = bg {
        if *bw > 0 && *bh > 0 {
            for y in 0..fh {
                for x in 0..fw {
                    let sx = (x * bw / fw).min(bw - 1);
                    let sy = (y * bh / fh).min(bh - 1);
                    let so = ((sy * bw + sx) * 4) as usize;
                    let d = ((y * fw + x) * 4) as usize;
                    if so + 4 <= bg_px.len() {
                        canvas[d..d + 4].copy_from_slice(&bg_px[so..so + 4]);
                    }
                }
            }
        }
    }

    // 前景按 alpha 叠上去
    for i in 0..(fw * fh) as usize {
        let o = i * 4;
        let a = fg[o + 3] as u32;
        if a == 0 {
            continue;
        }
        for c in 0..3 {
            canvas[o + c] =
                ((fg[o + c] as u32 * a + canvas[o + c] as u32 * (255 - a)) / 255) as u8;
        }
        canvas[o + 3] = 255;
    }

    // 108dp 画布里只有中间 72dp 是可见区 → 裁到 2/3
    let cw = (fw * 2 / 3).max(1);
    let ch = (fh * 2 / 3).max(1);
    let ox = (fw - cw) / 2;
    let oy = (fh - ch) / 2;
    let mut out = vec![0u8; (cw * ch * 4) as usize];
    for y in 0..ch {
        for x in 0..cw {
            let so = (((y + oy) * fw + (x + ox)) * 4) as usize;
            let d = ((y * cw + x) * 4) as usize;
            out[d..d + 4].copy_from_slice(&canvas[so..so + 4]);
        }
    }
    (cw, ch, out)
}

/// 读 `mipmap-anydpi-v26/ic_launcher.xml`，取 `<foreground android:drawable="@drawable/xxx"/>`
/// 里那个 drawable 的名字。
///
/// 这个 XML **是纯文本**（不是 AXML）—— 它在 APK 里就是原样存放的，
/// 所以直接字符串处理就够，不需要 XML 解析器。
fn adaptive_layers(
    zip: &mut ZipArchive<std::fs::File>,
    name: &str,
) -> Option<(String, Option<String>)> {
    let mut text = String::new();
    zip.by_name(name).ok()?.read_to_string(&mut text).ok()?;

    let fg = find_drawable_after(&text, "<foreground")?;
    // 背景可能是 @drawable/xxx（图片）也可能是 @color/xxx（纯色）——
    // 后者在我们这儿找不到对应图片，返回值然后自然降级成透明底
    let bg = find_drawable_after(&text, "<background");
    Some((fg, bg))
}

/// 在 `key` 之后找第一个 `@drawable/xxx` / `@mipmap/xxx` 里的 `xxx`
fn find_drawable_after(text: &str, key: &str) -> Option<String> {
    let lower = text.to_ascii_lowercase();
    let pos = lower.find(&key.to_ascii_lowercase())?;
    let rest = &text[pos..];
    let at = rest.find('@')?;
    let after = &rest[at + 1..];
    let slash = after.find('/')?;
    let value: String = after[slash + 1..]
        .chars()
        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '.')
        .collect();
    (!value.is_empty()).then_some(value)
}

/// 从 zip 里读一张图并解码成 RGBA
fn read_zip_image(
    zip: &mut ZipArchive<std::fs::File>,
    name: &str,
) -> AppResult<Option<(u32, u32, Vec<u8>)>> {
    let mut data = Vec::new();
    zip.by_name(name)
        .map_err(|e| AppError::Msg(format!("zip 条目打不开: {e}")))?
        .read_to_end(&mut data)?;
    let Ok(img) = image::load_from_memory(&data) else {
        return Ok(None);
    };
    let rgba = img.to_rgba8();
    Ok(Some((rgba.width(), rgba.height(), rgba.into_raw())))
}

/* ---------------- IPA ---------------- */

pub fn from_ipa(path: &Path, notes: &mut Vec<String>) -> AppResult<Vec<RawIcon>> {
    let mut zip = open_zip(path)?;

    // IPA 的结构固定是 Payload/<App>.app/...
    let mut app_dir = String::new();
    let mut cands: Vec<(u32, String)> = Vec::new();
    let mut app_name = String::new();

    for i in 0..zip.len() {
        let name = {
            let Ok(e) = zip.by_index_raw(i) else { continue };
            e.name().to_string()
        };
        if app_dir.is_empty() {
            if let Some(rest) = name.strip_prefix("Payload/") {
                if let Some(idx) = rest.find(".app/") {
                    app_dir = format!("Payload/{}/", &rest[..idx + 4]);
                    app_name = rest[..idx].to_string();
                }
            }
        }
        if !is_image(&name) {
            continue;
        }
        let fname = name.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
        // iOS 图标的名字里一定带 AppIcon 或 Icon
        if !(fname.contains("appicon") || fname.contains("icon")) {
            continue;
        }
        // 排在 Bundle 之外的多半是分享用的图，不优先
        let in_app = name.starts_with("Payload/");
        let score = ios_name_score(&fname) + if in_app { 500 } else { 0 };
        cands.push((score, name));
    }

    if !app_dir.is_empty() {
        notes.push(format!("应用：{app_name}"));
    } else {
        notes.push("没有 Payload/*.app 目录，可能不是标准 IPA".into());
    }

    cands.sort_by(|a, b| b.0.cmp(&a.0));
    cands.dedup_by(|a, b| a.1 == b.1);

    let mut out = Vec::new();
    for (_, name) in cands.iter().take(24) {
        match read_zip_image(&mut zip, name) {
            Ok(Some((w, h, rgba))) => out.push(RawIcon {
                width: w,
                height: h,
                rgba,
                source: name.clone(),
            }),
            Ok(None) => log::debug!("跳过无法解码的图: {name}"),
            Err(e) => log::debug!("读取 {name} 失败: {e}"),
        }
    }
    Ok(out)
}

/// 从 `AppIcon60x60@2x.png` 这种名字里估出分辨率，用来排序。
///
/// 不解码就能排，省掉"解 30 张 PNG 只为挑一张"的开销。
fn ios_name_score(fname: &str) -> u32 {
    // 先找 "<数字>x<数字>"
    let mut base = 0u32;
    let bytes = fname.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i].is_ascii_digit() {
            let start = i;
            while i < bytes.len() && bytes[i].is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && (bytes[i] == b'x' || bytes[i] == b'X') {
                if let Ok(n) = fname[start..i].parse::<u32>() {
                    base = base.max(n);
                }
            }
        } else {
            i += 1;
        }
    }
    // "@2x" / "@3x"
    let mut scale = 1u32;
    if let Some(at) = fname.find('@') {
        let rest = &fname[at + 1..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if let Ok(s) = digits.parse::<u32>() {
            scale = s.max(1);
        }
    }
    // 名字里带 1024 的多半是 App Store 用的大图
    if fname.contains("1024") {
        base = base.max(512);
    }
    if base == 0 {
        // 老式 `Icon.png` / `Icon@2x.png`：只有倍率信息（57 是 iPhone 基准）
        base = 57;
    }
    base * scale
}

/// 魔数未知的 zip 容器：先按 APK 试，没结果再按 IPA 试。
pub fn from_zip_auto(path: &Path, notes: &mut Vec<String>) -> AppResult<Vec<RawIcon>> {
    let apk = scan_apk(path, notes)?;
    if !apk.is_empty() {
        return Ok(apk);
    }
    let mut ipa_notes = Vec::new();
    let ipa = from_ipa(path, &mut ipa_notes)?;
    if !ipa.is_empty() {
        notes.extend(ipa_notes);
        return Ok(ipa);
    }
    Ok(Vec::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn density_ordering() {
        assert!(density_weight("res/mipmap-xxxhdpi/ic_launcher.png") > density_weight("res/mipmap-hdpi/ic_launcher.png"));
        assert!(density_weight("res/mipmap-hdpi/ic_launcher.png") > density_weight("res/mipmap-mdpi/ic_launcher.png"));
    }

    #[test]
    fn apk_scoring_prefers_launcher() {
        let launcher = apk_score("res/mipmap-xxhdpi/ic_launcher.png");
        let random = apk_score("res/drawable-hdpi/btn_bg.png");
        assert!(launcher > random, "launcher {launcher} 应高于随机图 {random}");
        assert_eq!(random, 0, "不含提示词的图不该进候选");
    }

    #[test]
    fn apk_scoring_penalises_background() {
        let fg = apk_score("res/drawable-xxhdpi/ic_launcher_foreground.png");
        let bg = apk_score("res/drawable-xxhdpi/ic_launcher_background.png");
        assert!(fg > bg, "前景层应优先于背景层: {fg} vs {bg}");
    }

    #[test]
    fn ios_score_reads_size_and_scale() {
        assert_eq!(ios_name_score("appicon60x60@2x.png"), 120);
        assert_eq!(ios_name_score("appicon60x60@3x.png"), 180);
        assert!(ios_name_score("appicon-1024.png") >= 512);
        // 只有倍率的老式命名
        assert_eq!(ios_name_score("icon@2x.png"), 114);
        assert_eq!(ios_name_score("icon.png"), 57);
    }

    #[test]
    fn adaptive_layers_reads_both_layers() {
        // 这个 XML 在 APK 里就是纯文本，直接字符串处理
        let xml = r#"<adaptive-icon xmlns:android="http://schemas.android.com/apk/res/android">
            <background android:drawable="@drawable/ic_launcher_background"/>
            <foreground android:drawable="@mipmap/ic_launcher_foreground"/>
        </adaptive-icon>"#;
        assert_eq!(
            find_drawable_after(xml, "<foreground").as_deref(),
            Some("ic_launcher_foreground")
        );
        assert_eq!(
            find_drawable_after(xml, "<background").as_deref(),
            Some("ic_launcher_background")
        );
        // 纯色背景不是图片，找得到名字但找不到文件 —— 调用方会降级成透明底
        let color_bg = r#"<background android:drawable="@color/brand"/>"#;
        assert_eq!(
            find_drawable_after(color_bg, "<background").as_deref(),
            Some("brand")
        );
    }
}
