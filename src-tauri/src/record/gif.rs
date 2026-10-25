//! GIF 编码 —— 录制线程直接喂原始帧，边录边写。
//!
//! 为什么不"先录 MP4 再转 GIF"：
//!   转码要先解 H.264，等于把 ffmpeg 塞进安装包（体积从 3MB 涨到 40MB+）。
//!   而录制循环手里本来就有 RGB 原始帧，直接送进 GIF 编码器是零成本路径 ——
//!   这也是 ScreenToGif / LICEcap / ShareX 的做法。
//!
//! # 线程模型
//!
//! 量化 + LZW + 写盘是重活（256 色量化要逐像素查色表），放在取帧循环里会
//! 直接把采集拖垮 —— 实测 1080p 下一帧能让采集掉到 5fps，连 MP4 都跟着卡。
//! 所以这里分两段：
//!
//!   录制线程  →  采样 / 降采样 / 静态帧去重  →  有界队列  →  编码线程
//!
//! 录制线程只做"轻活"（面积平均缩图 + memcmp 比帧），重活全在编码线程。
//! 队列满就丢帧，并把这一帧的显示时长攒进 `carry` 补到下一帧上 ——
//! 掉帧只会让画面更新慢一点，时间轴永远是对的。
//!
//! # 三个让 GIF 真的能用的取舍
//!
//!   1. **降采样**。GIF 是 256 色的位图序列，1080p 原尺寸基本不可用
//!      （一分钟上百 MB）。默认压到 900px 宽，用面积平均（box）缩 ——
//!      屏幕上最怕糊的是文字，box 平均比最近邻干净得多。
//!
//!   2. **合并静态帧**。屏幕录屏大部分时间画面不动，而 GIF 没有"跳过这帧"
//!      的概念，只能靠 delay。所以完全相同的帧不重复写，把时长累加到上一帧。
//!      对着静态桌面录 10 秒，合并后通常只有几帧。这是体积最大的一块优化。
//!
//!   3. **逐帧调色板**。GIF 允许每帧带自己的色表，正好对付"录制中打开不同
//!      窗口"。256 色以内的画面（UI 截图基本都在这个区间）直接建精确色表，
//!      颜色一个不差；超过 256 色才退回 NeuQuant 量化。
//!
//! 已知取舍：没有做误差扩散（dither）。屏幕内容以纯色块为主，精确色表能覆盖
//! 绝大部分场景；对照片类画面做 dither 会让平色区域起噪点，反而更难看。

use crate::error::{AppError, AppResult};
use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::thread::JoinHandle;

/// GIF 单帧 delay 是 u16、单位 1/100 秒 → 单帧最多停 655.35 秒。
/// 留一点余量，超过就再写一帧同样的画面。
const MAX_DELAY_CS: u16 = 60_000;

/// NeuQuant 采样因子，1 = 最慢最好，30 = 最快最糙。
/// 10 在 900px 宽度下肉眼几乎看不出差别，但快一个数量级。
const QUANT_SPEED: i32 = 10;

/// 队列深度。2 帧就够吃下一次编码抖动，再多只是白占内存
/// （1080p 一帧 8MB，深了等于把内存当缓存用）。
const QUEUE_DEPTH: usize = 3;

/// 一次录制里 GIF 那一路的配置
#[derive(Debug, Clone)]
pub struct GifPlan {
    pub path: PathBuf,
    /// 采样帧率（GIF 实际播放帧率）
    pub fps: u32,
    /// 输出最大宽度，0 = 不缩放
    pub max_width: u32,
}

/// 交给编码线程的一帧（已经缩好、已经去重过）
struct Job {
    rgba: Vec<u8>,
    delay: u16,
}

/// 录制线程这一侧的句柄：管采样节奏、缩图、去重，把重活丢给编码线程。
pub struct GifWriter {
    tx: Option<SyncSender<Job>>,
    handle: Option<JoinHandle<AppResult<u64>>>,
    path: PathBuf,
    /// 源尺寸 / 输出尺寸
    sw: u32,
    sh: u32,
    dw: u32,
    dh: u32,
    /// 源帧累计值：每来一帧加 `ratio`，跨过 1 就采一帧。
    /// 用它把 30fps 的采集精确对齐到 12fps 的 GIF（每 2~3 帧取一帧，长期不漂）。
    acc: f64,
    /// 每来一个采集帧，相当于多少分之一"要采的帧"
    ratio: f64,
    /// delay 的四舍五入余数，单位 1/100 秒
    d_acc: f64,
    /// 每个 GIF 帧的目标时长，单位 1/100 秒
    delay_avg: f64,
    /// 上一帧的缩放结果，等下一帧到了才知道它该停多久
    last: Vec<u8>,
    /// `last` 该停的时长，静态帧会一直往上累
    pending: u16,
    /// 队列满被丢掉的那些帧的时长，补到下一帧上，保证时间轴不错位
    carry: u16,
    started: bool,
    sent: u64,
    dropped: u64,
}

impl GifWriter {
    pub fn new(plan: &GifPlan, src_w: u32, src_h: u32, capture_fps: u32) -> AppResult<Self> {
        let (dw, dh) = out_size(src_w, src_h, plan.max_width);
        if dw < 2 || dh < 2 {
            return Err(AppError::Msg(format!("GIF 输出尺寸太小：{dw}×{dh}")));
        }
        // GIF 的宽高字段是 u16
        if dw > u16::MAX as u32 || dh > u16::MAX as u32 {
            return Err(AppError::Msg(format!("GIF 尺寸超出上限：{dw}×{dh}")));
        }

        // 编码线程自己开文件：建不出来要在这里就报错，不能等录完才发现
        let path = plan.path.clone();
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)?;
            }
        }
        let enc = Encoder::new(&path, dw, dh)?;

        let (tx, rx) = sync_channel(QUEUE_DEPTH);
        let handle = std::thread::Builder::new()
            .name("snipforge-gif".into())
            .spawn(move || encode_loop(enc, rx))
            .map_err(|e| AppError::Msg(format!("启动 GIF 编码线程失败: {e}")))?;

        // GIF 帧率不可能高于采集帧率，高于也没帧可用
        let fps = plan.fps.clamp(2, capture_fps.clamp(2, 60));
        log::info!(
            "GIF 输出 {}×{} @{}fps（源 {}×{} @{}fps）→ {}",
            dw,
            dh,
            fps,
            src_w,
            src_h,
            capture_fps,
            path.display()
        );

        Ok(Self {
            tx: Some(tx),
            handle: Some(handle),
            path,
            sw: src_w,
            sh: src_h,
            dw,
            dh,
            acc: 0.0,
            ratio: (fps as f64 / capture_fps.max(1) as f64).min(1.0),
            d_acc: 0.0,
            delay_avg: 100.0 / fps as f64,
            last: Vec::new(),
            pending: 0,
            carry: 0,
            started: false,
            sent: 0,
            dropped: 0,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn out_size(&self) -> (u32, u32) {
        (self.dw, self.dh)
    }

    /// 录制循环每帧调一次，内部决定这一帧要不要进 GIF。
    ///
    /// 采样点用"累计跨过 1 就采"的方式，而不是 `acc_ms >= target` 那种比较 ——
    /// 后者在 30fps 采集配 12fps GIF 时会因为余数关系连着跳过两帧，
    /// 动作看起来一顿一顿。按 2/3 帧的节奏均匀取，观感才顺。
    pub fn sample(&mut self, rgba: &[u8]) -> AppResult<()> {
        self.acc += self.ratio;
        if self.acc + 1e-9 < 1.0 {
            return Ok(());
        }
        self.acc -= 1.0;

        // delay 只能是整数（1/100 秒），四舍五入的余数留在 d_acc 里抵下一次，
        // **不要 clamp 到 0** —— 余数有正有负，掐掉就会系统性地偏慢。
        self.d_acc += self.delay_avg;
        let delay = self.d_acc.round().max(1.0).min(MAX_DELAY_CS as f64) as u16;
        self.d_acc -= delay as f64;

        let cur = self.scale(rgba);

        if self.started {
            if cur == self.last {
                // 画面没动：不写新帧，把这帧的时长加到上一帧头上
                self.pending = self.pending.saturating_add(delay);
                if self.pending >= MAX_DELAY_CS {
                    self.flush_pending()?;
                }
                return Ok(());
            }
            // 画面变了，上一帧的停留时长到此确定
            self.flush_pending()?;
        }

        self.last = cur;
        self.pending = delay;
        self.started = true;
        Ok(())
    }

    /// 把 `last` 连同 `pending` 的时长丢给编码线程。
    /// 用 `mem::take` 把缓冲借出来，避免每帧再 clone 一份 4MB 的图。
    fn flush_pending(&mut self) -> AppResult<()> {
        let frame = std::mem::take(&mut self.last);
        let mut delay = self.pending.max(1);
        // 上次因为队列满被丢掉的帧，时长补在这里
        delay = delay.saturating_add(self.carry).min(MAX_DELAY_CS);
        self.carry = 0;

        let tx = self
            .tx
            .as_ref()
            .ok_or_else(|| AppError::Msg("GIF 编码线程已关闭".into()))?;

        match tx.try_send(Job { rgba: frame, delay }) {
            Ok(()) => {
                self.sent += 1;
                Ok(())
            }
            Err(TrySendError::Full(job)) => {
                // 编码跟不上：这一帧不要了，时长攒到下一帧，时间轴照样准
                self.carry = self.carry.saturating_add(job.delay);
                self.dropped += 1;
                if self.dropped == 1 {
                    log::warn!("GIF 编码跟不上采集，开始丢帧（时间轴会自动补偿）");
                }
                Ok(())
            }
            Err(TrySendError::Disconnected(_)) => {
                Err(AppError::Msg("GIF 编码线程已退出".into()))
            }
        }
    }

    /// 把源帧缩到输出尺寸。尺寸一致时直接拷贝。
    fn scale(&self, src: &[u8]) -> Vec<u8> {
        if self.dw == self.sw && self.dh == self.sh {
            return src.to_vec();
        }
        downscale(src, self.sw, self.sh, self.dw, self.dh)
    }

    /// 收尾：把最后一帧补上，等编码线程写完文件尾。返回实际写入的帧数。
    pub fn finish(&mut self) -> AppResult<u64> {
        let mut first_err: Option<AppError> = None;

        if self.started && self.pending > 0 {
            if let Err(e) = self.flush_pending() {
                first_err.get_or_insert(e);
            }
        }

        // 关掉发送端，编码线程读到通道空就会退出并落盘
        self.tx.take();

        let written = match self.handle.take() {
            Some(h) => match h.join() {
                Ok(Ok(n)) => n,
                Ok(Err(e)) => {
                    first_err.get_or_insert(e);
                    0
                }
                Err(_) => {
                    first_err.get_or_insert(AppError::Msg("GIF 编码线程崩溃".into()));
                    0
                }
            },
            None => 0,
        };

        log::info!(
            "GIF 收尾：写出 {written} 帧（投递 {}，丢 {}）→ {}",
            self.sent,
            self.dropped,
            self.path.display()
        );
        match first_err {
            Some(e) => Err(e),
            None => Ok(written),
        }
    }
}

impl Drop for GifWriter {
    fn drop(&mut self) {
        // 忘了调 finish（比如录到一半出错）也不能把编码线程晾在那儿
        self.tx.take();
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

/* ------------------------- 编码线程 ------------------------- */

/// 编码线程侧的写入器：只管量化 + 写帧，不管采样节奏。
struct Encoder {
    enc: Option<gif::Encoder<BufWriter<File>>>,
    dw: u32,
    dh: u32,
    frames: u64,
}

impl Encoder {
    fn new(path: &Path, dw: u32, dh: u32) -> AppResult<Self> {
        let file = File::create(path)?;
        let mut enc = gif::Encoder::new(BufWriter::new(file), dw as u16, dh as u16, &[])
            .map_err(|e| AppError::Msg(format!("创建 GIF 编码器失败: {e}")))?;
        enc.set_repeat(gif::Repeat::Infinite)
            .map_err(|e| AppError::Msg(format!("写入 GIF 循环标记失败: {e}")))?;
        Ok(Self {
            enc: Some(enc),
            dw,
            dh,
            frames: 0,
        })
    }

    fn write(&mut self, rgba: &mut [u8], delay: u16) -> AppResult<()> {
        // 全不透明：`from_rgba_speed` 会把 alpha==0 的像素当成透明色，
        // 录屏没有透明需求，统一拍平，省得画面上出现随机空洞。
        for p in rgba.chunks_exact_mut(4) {
            p[3] = 255;
        }

        let mut frame =
            gif::Frame::from_rgba_speed(self.dw as u16, self.dh as u16, rgba, QUANT_SPEED);
        frame.delay = delay.max(1);
        // 每帧都是整幅画面 + 自己的调色板，disposal 随便哪种都行
        frame.dispose = gif::DisposalMethod::Any;

        self.enc
            .as_mut()
            .ok_or_else(|| AppError::Msg("GIF 编码器已关闭".into()))?
            .write_frame(&frame)
            .map_err(|e| AppError::Msg(format!("写入 GIF 帧失败: {e}")))?;
        self.frames += 1;
        Ok(())
    }

    fn finish(&mut self) -> AppResult<()> {
        // `into_inner` 会写文件尾（GIF trailer，0x3B）。
        // 少了它，文件在浏览器里能看，在资源管理器里缩略图是坏的。
        let Some(enc) = self.enc.take() else {
            return Ok(());
        };
        let mut w = enc
            .into_inner()
            .map_err(|e| AppError::Msg(format!("GIF 收尾失败: {e}")))?;
        w.flush()?;
        Ok(())
    }
}

fn encode_loop(mut enc: Encoder, rx: Receiver<Job>) -> AppResult<u64> {
    for job in rx.iter() {
        let mut rgba = job.rgba;
        if let Err(e) = enc.write(&mut rgba, job.delay) {
            // 磁盘满之类：后面没法继续了，直接收尾把已有内容保住
            log::error!("GIF 编码中断: {e}");
            let _ = enc.finish();
            return Err(e);
        }
    }
    enc.finish()?;
    Ok(enc.frames)
}

/* ------------------------- 尺寸 / 缩放 ------------------------- */

/// 算输出尺寸：宽超了就等比缩，不超就原样。
fn out_size(sw: u32, sh: u32, max_w: u32) -> (u32, u32) {
    if max_w == 0 || sw <= max_w {
        return (sw.max(2), sh.max(2));
    }
    let dw = max_w;
    let dh = ((sh as u64 * dw as u64) / sw.max(1) as u64).max(2) as u32;
    (dw, dh)
}

/// 面积平均降采样。
///
/// 每个目标像素取它在源图上覆盖的那块矩形求平均 —— 缩小时这是最不容易
/// 出摩尔纹的做法，而且对屏幕上的细字比双线性插值更干净。
fn downscale(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = vec![0u8; (dw as usize) * (dh as usize) * 4];
    let sstride = (sw as usize) * 4;

    for dy in 0..dh {
        let y0 = (dy as u64 * sh as u64 / dh as u64) as usize;
        let y1 = ((((dy as u64 + 1) * sh as u64 / dh as u64) as usize).max(y0 + 1)).min(sh as usize);
        for dx in 0..dw {
            let x0 = (dx as u64 * sw as u64 / dw as u64) as usize;
            let x1 =
                ((((dx as u64 + 1) * sw as u64 / dw as u64) as usize).max(x0 + 1)).min(sw as usize);

            let (mut r, mut g, mut b, mut n) = (0u32, 0u32, 0u32, 0u32);
            for y in y0..y1 {
                let row = y * sstride;
                for x in x0..x1 {
                    let o = row + x * 4;
                    if o + 3 >= src.len() {
                        continue;
                    }
                    r += src[o] as u32;
                    g += src[o + 1] as u32;
                    b += src[o + 2] as u32;
                    n += 1;
                }
            }
            let o = (dy as usize * dw as usize + dx as usize) * 4;
            let n = n.max(1);
            out[o] = (r / n) as u8;
            out[o + 1] = (g / n) as u8;
            out[o + 2] = (b / n) as u8;
            // 录屏全不透明，alpha 直接拍平（见 Encoder::write 里的说明）
            out[o + 3] = 255;
        }
    }
    out
}

/* ---------------------------- 自检 ---------------------------- */

/// 编码器自检：写一段合成动画，再用 gif 自己的解码器读回来核对。
///
/// 单拎出来是为了能区分「GIF 编码坏了」和「取帧坏了」——
/// 录出来是黑屏、还是文件根本打不开，排查方向完全不同。
pub fn gif_smoke(path: &Path, fps: u32, seconds: u32) -> AppResult<(u64, u64, u64)> {
    const W: u32 = 640;
    const H: u32 = 360;

    let _ = std::fs::remove_file(path);
    let plan = GifPlan {
        path: path.to_path_buf(),
        fps,
        max_width: W,
    };
    let mut w = GifWriter::new(&plan, W, H, fps)?;

    let total = fps * seconds;
    let mut frame = vec![0u8; (W * H * 4) as usize];
    for i in 0..total {
        // 一条从左往右扫的亮条
        let head = (i * W / total.max(1)) as i32;
        for y in 0..H {
            for x in 0..W {
                let o = ((y * W + x) * 4) as usize;
                let lit = (x as i32 - head).abs() < 40;
                frame[o] = if lit { 255 } else { 30 };
                frame[o + 1] = if lit { 220 } else { 32 };
                frame[o + 2] = if lit { 120 } else { 40 };
                frame[o + 3] = 255;
            }
        }
        w.sample(&frame)?;
    }
    let written = w.finish()?;

    // 读回来：帧数、总时长、画布尺寸
    let (read_frames, total_cs, rw, rh) = decode_stats(path)?;
    let expect_cs = seconds as u64 * 100;
    if read_frames == 0 {
        return Err(AppError::Msg("读回的 GIF 一帧都没有".into()));
    }
    if rw != W as u16 || rh != H as u16 {
        return Err(AppError::Msg(format!(
            "画布尺寸不对：{rw}×{rh}，期望 {W}×{H}"
        )));
    }
    if written != read_frames {
        return Err(AppError::Msg(format!(
            "写入 {written} 帧但读回 {read_frames} 帧"
        )));
    }
    // 合并静态帧 + 四舍五入 delay，允许 5% 的偏差（末尾那半个采样周期会丢）
    if (total_cs as i64 - expect_cs as i64).abs() > (expect_cs / 20) as i64 {
        return Err(AppError::Msg(format!(
            "总时长对不上：期望约 {expect_cs}×0.01s，实际 {total_cs}×0.01s"
        )));
    }
    Ok((written, read_frames, total_cs))
}

/// 用 gif 解码器读一遍，返回（帧数, delay 总和[1/100 秒], 宽, 高）
pub fn decode_stats(path: &Path) -> AppResult<(u64, u64, u16, u16)> {
    let file = File::open(path)?;
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let mut dec = opts
        .read_info(std::io::BufReader::new(file))
        .map_err(|e| AppError::Msg(format!("读取 GIF 头失败: {e}")))?;
    let (w, h) = (dec.width(), dec.height());

    let mut frames = 0u64;
    let mut total = 0u64;
    while let Some(f) = dec
        .read_next_frame()
        .map_err(|e| AppError::Msg(format!("解码 GIF 帧失败: {e}")))?
    {
        frames += 1;
        total += f.delay as u64;
    }
    Ok((frames, total, w, h))
}

/// 把 GIF 的第 index 帧导成 PNG，用来肉眼确认颜色/方向。
pub fn extract_frame(path: &Path, out: &Path, index: u32) -> AppResult<(u16, u16)> {
    let file = File::open(path)?;
    let mut opts = gif::DecodeOptions::new();
    opts.set_color_output(gif::ColorOutput::RGBA);
    let mut dec = opts
        .read_info(std::io::BufReader::new(file))
        .map_err(|e| AppError::Msg(format!("读取 GIF 头失败: {e}")))?;
    let (w, h) = (dec.width(), dec.height());

    let mut i = 0;
    while let Some(f) = dec
        .read_next_frame()
        .map_err(|e| AppError::Msg(format!("解码 GIF 帧失败: {e}")))?
    {
        if i == index {
            let img = image::RgbaImage::from_raw(f.width as u32, f.height as u32, f.buffer.to_vec())
                .ok_or_else(|| AppError::Msg("GIF 帧数据长度不符".into()))?;
            img.save_with_format(out, image::ImageFormat::Png)?;
            return Ok((w, h));
        }
        i += 1;
    }
    Err(AppError::Msg(format!("GIF 只有 {i} 帧，没有第 {index} 帧")))
}
