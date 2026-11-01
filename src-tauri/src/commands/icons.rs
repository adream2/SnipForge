//! 图标提取 IPC。
//!
//! 提取本身是纯 CPU 活（解 zip、解 PE 资源、解码 PNG），放在
//! `spawn_blocking` 里跑 —— APK 有几百个条目，在主线程上做会卡住窗口。

use crate::error::{AppError, AppResult};
use crate::icons::{IconBundle, IconAsset};
use std::path::Path;

/// 从 exe / dll / lnk / apk / ipa / ico / 图片里提取图标。
#[tauri::command]
pub async fn icon_extract(path: String) -> AppResult<IconBundle> {
    tauri::async_runtime::spawn_blocking(move || crate::icons::extract(Path::new(&path)))
        .await
        .map_err(|e| AppError::Msg(format!("图标提取任务失败: {e}")))?
}

/// 提取结果里的图标直接用 id 索引？不需要 —— 前端本来就有 data URL，
/// 保存时复用 `image_save`（它已经做了文件名消毒和目录校验）。
///
/// 这里只提供一个"把多个尺寸一次性存成 ico"的便利命令：
/// 用户做软件图标时往往需要 .ico，而浏览器没法生成它。
#[tauri::command]
pub async fn icon_build_ico(
    icons: Vec<IconAsset>,
    out_path: String,
) -> AppResult<String> {
    tauri::async_runtime::spawn_blocking(move || build_ico(&icons, Path::new(&out_path)))
        .await
        .map_err(|e| AppError::Msg(format!("生成 ICO 失败: {e}")))?
}

/// 把若干 PNG data URL 打包成一个多尺寸 .ico。
///
/// ICO 容器本身很简单：6 字节头 + 每帧 16 字节目录项 + 各帧数据。
/// 而且 **ICO 允许直接内嵌 PNG 帧**（Vista 起），所以完全不用重新编码 ——
/// 把 PNG 原样塞进去即可，体积和画质都是最优。
fn build_ico(icons: &[IconAsset], out: &Path) -> AppResult<String> {
    if icons.is_empty() {
        return Err(AppError::Msg("没有可保存的图标".into()));
    }

    // 解 data URL → PNG 字节
    let mut frames: Vec<(u32, u32, Vec<u8>)> = Vec::new();
    for icon in icons {
        let Some((_, payload)) = icon.data_url.split_once(',') else {
            continue;
        };
        let bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            payload,
        )
        .map_err(|e| AppError::Msg(format!("图标数据损坏: {e}")))?;
        frames.push((icon.width, icon.height, bytes));
    }
    if frames.is_empty() {
        return Err(AppError::Msg("图标数据为空".into()));
    }
    // ICO 目录项里宽高各占 1 字节，0 表示 256；超过 256 的帧必须跳过
    frames.retain(|(w, h, _)| *w <= 256 && *h <= 256);
    if frames.is_empty() {
        return Err(AppError::Msg(
            "所有图标都大于 256×256。ICO 格式单帧最大 256，请先缩小再保存".into(),
        ));
    }

    let count = frames.len() as u16;
    let mut head = Vec::with_capacity(6 + frames.len() * 16);
    head.extend_from_slice(&0u16.to_le_bytes()); // reserved
    head.extend_from_slice(&1u16.to_le_bytes()); // 1 = icon
    head.extend_from_slice(&count.to_le_bytes());

    let mut offset = 6 + frames.len() * 16;
    let mut body = Vec::new();
    for (w, h, png) in &frames {
        head.push(if *w >= 256 { 0 } else { *w as u8 });
        head.push(if *h >= 256 { 0 } else { *h as u8 });
        head.push(0); // 调色板颜色数，真彩色写 0
        head.push(0); // reserved
        head.extend_from_slice(&1u16.to_le_bytes()); // planes
        head.extend_from_slice(&32u16.to_le_bytes()); // bitCount
        head.extend_from_slice(&(png.len() as u32).to_le_bytes());
        head.extend_from_slice(&(offset as u32).to_le_bytes());
        body.extend_from_slice(png);
        offset += png.len();
    }

    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut data = head;
    data.extend_from_slice(&body);
    std::fs::write(out, &data)?;
    log::info!("已生成 ICO：{}（{} 帧，{} 字节）", out.display(), frames.len(), data.len());
    Ok(out.to_string_lossy().to_string())
}
