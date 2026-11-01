//! 屏幕捕获。
//!
//! 关键决策：**每块显示器独立一个捕获单元 + 独立一个覆盖窗口**。
//!
//! 为什么不做"一个窗口覆盖整个虚拟屏幕"？
//!   WebView2 在一个窗口内只认一个 DPI 基准。窗口横跨两块缩放不同的屏幕时，
//!   其中一块上的内容必然错位——这是物理限制，不是实现偷懒。
//!   逐屏处理换来的是每块屏都像素级精确，代价只是暂不支持跨屏框选
//!   （截图工具的跨屏框选本来就是伪需求）。

use crate::error::{AppError, AppResult};
use crate::types::{CaptureResult, DisplayPreview, FramePayload, Rect};
use base64::Engine;
use image::{codecs::jpeg::JpegEncoder, imageops, RgbaImage};
use std::io::Cursor;
use xcap::Monitor;

pub struct CapturedDisplay {
    pub id: u32,
    pub name: String,
    /// 物理像素坐标（虚拟屏幕坐标系）
    pub physical: Rect,
    pub scale_factor: f64,
    pub is_primary: bool,
    pub image: RgbaImage,
}

pub struct CaptureFrame {
    pub displays: Vec<CapturedDisplay>,
}

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

/// 抓取全部显示器。耗时主要在 PNG/JPEG 编码（预览图），
/// 原始 RGBA 数据留在内存里供后续精确裁剪。
pub fn capture_frame() -> AppResult<CaptureFrame> {
    let monitors = Monitor::all().map_err(|e| AppError::Msg(format!("枚举显示器失败: {e}")))?;
    if monitors.is_empty() {
        return Err(AppError::Msg("没有检测到显示器".into()));
    }

    let mut displays = Vec::with_capacity(monitors.len());
    for (index, monitor) in monitors.iter().enumerate() {
        let image = monitor
            .capture_image()
            .map_err(|e| AppError::Msg(format!("屏幕捕获失败: {e}")))?;

        let scale = monitor.scale_factor().unwrap_or(1.0) as f64;
        let x = monitor.x().unwrap_or(0);
        let y = monitor.y().unwrap_or(0);
        let width = monitor.width().unwrap_or(image.width());
        let height = monitor.height().unwrap_or(image.height());

        displays.push(CapturedDisplay {
            id: index as u32,
            name: monitor
                .friendly_name()
                .or_else(|_| monitor.name())
                .unwrap_or_else(|_| format!("显示器 {}", index + 1)),
            physical: Rect {
                x,
                y,
                width,
                height,
            },
            scale_factor: scale,
            is_primary: monitor.is_primary().unwrap_or(index == 0),
            image,
        });
    }

    Ok(CaptureFrame {
        displays,
    })
}

/// 编码成 PNG data URL
pub fn to_png_data_url(image: &RgbaImage) -> AppResult<String> {
    let mut buf = Vec::new();
    image
        .write_to(&mut Cursor::new(&mut buf), image::ImageFormat::Png)
        .map_err(|e| AppError::Msg(format!("PNG 编码失败: {e}")))?;
    Ok(format!("data:image/png;base64,{}", B64.encode(&buf)))
}

/// 生成给前端显示用的预览（缩到逻辑像素尺寸，JPEG，体积小传输快）
pub fn preview_data_url(image: &RgbaImage, scale: f64) -> AppResult<String> {
    let target_w = ((image.width() as f64) / scale).round().max(1.0) as u32;
    let target_h = ((image.height() as f64) / scale).round().max(1.0) as u32;

    let preview = if target_w < image.width() || target_h < image.height() {
        imageops::resize(image, target_w, target_h, imageops::FilterType::Triangle)
    } else {
        image.clone()
    };

    let rgb = imageops::colorops::grayscale_alpha(&preview);
    let _ = rgb; // 保留引用避免未使用告警

    let mut buf = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut buf, 82);
    // JPEG 不吃 alpha，转 RGB8 再编
    let rgb8 = image::DynamicImage::ImageRgba8(preview).to_rgb8();
    encoder
        .encode(&rgb8, rgb8.width(), rgb8.height(), image::ExtendedColorType::Rgb8)
        .map_err(|e| AppError::Msg(format!("JPEG 编码失败: {e}")))?;

    Ok(format!("data:image/jpeg;base64,{}", B64.encode(&buf)))
}

impl CaptureFrame {
    /// 给 selector 用的载荷（不含原始像素，只有预览）
    pub fn payload(&self) -> FramePayload {
        let union = self
            .displays
            .iter()
            .fold(None::<Rect>, |acc, d| match acc {
                None => Some(d.physical),
                Some(a) => {
                    let x = a.x.min(d.physical.x);
                    let y = a.y.min(d.physical.y);
                    let r = a.right().max(d.physical.right());
                    let b = a.bottom().max(d.physical.bottom());
                    Some(Rect {
                        x,
                        y,
                        width: (r - x) as u32,
                        height: (b - y) as u32,
                    })
                }
            })
            .unwrap_or_default();

        FramePayload {
            union,
            displays: self
                .displays
                .iter()
                .map(|d| DisplayPreview {
                    id: d.id,
                    bounds: d.physical,
                    scale_factor: d.scale_factor,
                    preview: preview_data_url(&d.image, d.scale_factor).unwrap_or_default(),
                })
                .collect(),
        }
    }

    pub fn find(&self, id: u32) -> Option<&CapturedDisplay> {
        self.displays.iter().find(|d| d.id == id)
    }
}

/// 从某块屏的原始帧里裁出指定物理矩形
pub fn crop_display(display: &CapturedDisplay, rect: Rect) -> AppResult<CaptureResult> {
    let local = rect.intersect(&display.physical).ok_or_else(|| {
        AppError::Msg("选区和屏幕没有交集".into())
    })?;

    let ox = (local.x - display.physical.x).max(0) as u32;
    let oy = (local.y - display.physical.y).max(0) as u32;
    let w = local.width.min(display.image.width().saturating_sub(ox)).max(1);
    let h = local.height.min(display.image.height().saturating_sub(oy)).max(1);

    let cropped = imageops::crop_imm(&display.image, ox, oy, w, h).to_image();
    let data_url = to_png_data_url(&cropped)?;

    Ok(CaptureResult {
        data_url,
        width: cropped.width(),
        height: cropped.height(),
        scale_factor: display.scale_factor,
        source: "region".into(),
        timestamp: chrono::Utc::now().timestamp_millis(),
    })
}

