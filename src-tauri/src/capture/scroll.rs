//! 长截图（滚动拼接）。
//!
//! 算法：逐帧捕获 → 用「行签名」在下一帧里定位上一帧尾部的落点 → 只追加新增行。
//!
//! 为什么不用特征点匹配（SIFT/ORB 那套）？
//!   1. 引入 opencv 级别的依赖，编译体积和复杂度都不值
//!   2. 我们控制着"一屏内恰好滚一次"，搜索范围极小，行级暴力扫描足够精确
//!   3. 纯文本页面占绝大多数，行匹配的准确率比特征点更高
//!
//! 三个坑都处理了：
//!   - 滚动条自己会动 → 匹配时排除右边缘
//!   - 页面惯性没停就截 → 帧间等待 + 连续两帧无新增即停止
//!   - 到顶/到底 → 同上

use crate::error::{AppError, AppResult};
use crate::types::{CaptureResult, Rect};
use image::{imageops, RgbaImage};
use serde::Serialize;

/// 采样列数：24 个点足以区分绝大多数文本行，再多就是浪费 CPU
const SAMPLE_POINTS: u32 = 24;
/// 右侧留白，避开滚动条
const SCROLLBAR_GUARD: u32 = 20;
/// 输出高度上限（物理像素）。再高就该分页了，不是截图工具该干的事
const MAX_OUTPUT_HEIGHT: u32 = 24000;
/// 匹配时取的条带高度
const BAND_HEIGHT: u32 = 24;
/// 平均色差阈值，超过认为匹配失败
const MATCH_THRESHOLD: f32 = 14.0;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrollProgress {
    pub frame: usize,
    pub max_frames: usize,
    pub stitched_height: u32,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

fn row_signature(img: &RgbaImage, y: u32) -> Vec<f32> {
    let usable = img.width().saturating_sub(SCROLLBAR_GUARD * 2).max(8);
    let step = (usable / SAMPLE_POINTS).max(1);
    let mut sig = Vec::with_capacity(SAMPLE_POINTS as usize);
    for i in 0..SAMPLE_POINTS {
        let x = (SCROLLBAR_GUARD + i * step).min(img.width().saturating_sub(1));
        let p = img.get_pixel(x, y).0;
        // 亮度加权，换通道顺序也不受影响
        sig.push(p[0] as f32 * 0.299 + p[1] as f32 * 0.587 + p[2] as f32 * 0.114);
    }
    sig
}

fn sig_diff(a: &[f32], b: &[f32]) -> f32 {
    a.iter()
        .zip(b.iter())
        .map(|(x, y)| (x - y).abs())
        .sum::<f32>()
        / a.len() as f32
}

/// 在 next 中寻找 prev 底部条带的落点，返回重叠行数
fn find_overlap(prev: &RgbaImage, next: &RgbaImage) -> u32 {
    if prev.height() < BAND_HEIGHT + 4 || next.height() < BAND_HEIGHT + 4 {
        return 0;
    }

    let band_start = prev.height() - BAND_HEIGHT;
    let prev_sigs: Vec<Vec<f32>> = (0..BAND_HEIGHT)
        .map(|i| row_signature(prev, band_start + i))
        .collect();

    let max_search = next.height().saturating_sub(BAND_HEIGHT);
    let mut best_score = f32::MAX;
    let mut best_pos: Option<u32> = None;

    // 粗扫步长 2，命中再邻域精调
    let mut y = 0u32;
    while y <= max_search {
        let mut score = 0.0;
        for i in 0..BAND_HEIGHT {
            score += sig_diff(&prev_sigs[i as usize], &row_signature(next, y + i));
            if score > best_score {
                break;
            }
        }
        score /= BAND_HEIGHT as f32;
        if score < best_score {
            best_score = score;
            best_pos = Some(y);
        }
        y += 2;
    }

    let Some(pos) = best_pos else { return 0 };

    let lo = pos.saturating_sub(3);
    let hi = (pos + 3).min(max_search);
    for yy in lo..=hi {
        let mut score = 0.0;
        for i in 0..BAND_HEIGHT {
            score += sig_diff(&prev_sigs[i as usize], &row_signature(next, yy + i));
        }
        score /= BAND_HEIGHT as f32;
        if score < best_score {
            best_score = score;
            best_pos = Some(yy);
        }
    }

    if best_score > MATCH_THRESHOLD {
        0
    } else {
        best_pos.unwrap_or(0)
    }
}

pub struct ScrollOptions {
    pub max_frames: usize,
    pub settle_ms: u64,
    /// 滚轮事件的物理像素落点
    pub wheel_x: i32,
    pub wheel_y: i32,
    /// 每次滚多少（WHEEL_STEP 的倍数）
    pub wheel_delta: i32,
}

impl Default for ScrollOptions {
    fn default() -> Self {
        Self {
            max_frames: 24,
            settle_ms: 420,
            wheel_x: 0,
            wheel_y: 0,
            wheel_delta: -360,
        }
    }
}

/// 执行长截图。progress 回调用于向前端推送实时进度。
///
/// `capture_once` 由调用方提供 —— 让它负责"隐藏窗口 + 抓取指定区域"，
/// 长截图算法本身不需要知道窗口怎么管。
pub fn run<F, P>(
    rect: Rect,
    options: ScrollOptions,
    mut capture_once: F,
    mut progress: P,
    aborted: &dyn Fn() -> bool,
) -> AppResult<CaptureResult>
where
    F: FnMut(Rect) -> AppResult<RgbaImage>,
    P: FnMut(ScrollProgress),
{
    let first = capture_once(rect)?;
    let width = first.width();
    let mut stitched: Vec<u8> = first.as_raw().clone();
    let mut stitched_height = first.height();
    let mut prev = first;
    let mut idle_rounds = 0usize;
    let mut frame_index = 0usize;

    progress(ScrollProgress {
        frame: 0,
        max_frames: options.max_frames,
        stitched_height,
        status: "capturing".into(),
        preview: None,
        message: None,
    });

    for _ in 0..options.max_frames {
        if aborted() {
            break;
        }

        crate::win32::input::scroll_at(options.wheel_x, options.wheel_y, options.wheel_delta)
            .map_err(AppError::Win32)?;
        std::thread::sleep(std::time::Duration::from_millis(options.settle_ms));
        if aborted() {
            break;
        }

        let next = match capture_once(rect) {
            Ok(img) => img,
            Err(e) => {
                log::warn!("长截图帧捕获失败: {e}");
                break;
            }
        };

        // 尺寸变了说明目标窗口被调整，继续拼只会糊掉
        if next.width() != width || next.height() != prev.height() {
            log::warn!("帧尺寸变化，提前结束");
            break;
        }

        let overlap = find_overlap(&prev, &next);
        let new_rows = next.height().saturating_sub(overlap);

        // 重叠 > 95% 视为没滚动（到底了，或者目标窗口不响应滚轮）
        if overlap == 0 || new_rows < 12 || new_rows > (next.height() as f32 * 0.95) as u32 {
            idle_rounds += 1;
            if idle_rounds >= 2 {
                log::info!("连续两帧无新增内容，结束长截图");
                break;
            }
            prev = next;
            continue;
        }
        idle_rounds = 0;

        if stitched_height + new_rows > MAX_OUTPUT_HEIGHT {
            log::warn!("触及高度上限 {MAX_OUTPUT_HEIGHT}px，提前结束");
            break;
        }

        // 只把新增部分追加进去
        let row_bytes = (width * 4) as usize;
        let start = overlap as usize * row_bytes;
        stitched.extend_from_slice(&next.as_raw()[start..]);

        stitched_height += new_rows;
        prev = next;
        frame_index += 1;

        progress(ScrollProgress {
            frame: frame_index,
            max_frames: options.max_frames,
            stitched_height,
            status: "capturing".into(),
            preview: Some(crate::capture::screen::preview_data_url(&prev, 1.0).unwrap_or_default()),
            message: None,
        });
    }

    progress(ScrollProgress {
        frame: frame_index,
        max_frames: options.max_frames,
        stitched_height,
        status: "stitching".into(),
        preview: None,
        message: None,
    });

    let image = RgbaImage::from_raw(width, stitched_height, stitched)
        .ok_or_else(|| AppError::Msg("拼接结果尺寸异常".into()))?;

    let data_url = crate::capture::screen::to_png_data_url(&image)?;

    progress(ScrollProgress {
        frame: frame_index,
        max_frames: options.max_frames,
        stitched_height,
        status: "done".into(),
        preview: Some(crate::capture::screen::preview_data_url(&image, 1.0).unwrap_or_default()),
        message: None,
    });

    Ok(CaptureResult {
        width: image.width(),
        height: image.height(),
        data_url,
        scale_factor: 1.0,
        source: "scroll".into(),
        timestamp: chrono::Utc::now().timestamp_millis(),
    })
}

/// 把捕获到的显示器帧按物理矩形裁剪成独立图片（长截图帧来源）
pub fn extract_region(display: &crate::capture::screen::CapturedDisplay, rect: Rect) -> Option<RgbaImage> {
    let local = rect.intersect(&display.physical)?;
    let ox = (local.x - display.physical.x).max(0) as u32;
    let oy = (local.y - display.physical.y).max(0) as u32;
    let w = local.width.min(display.image.width().saturating_sub(ox)).max(1);
    let h = local.height.min(display.image.height().saturating_sub(oy)).max(1);
    Some(imageops::crop_imm(&display.image, ox, oy, w, h).to_image())
}
