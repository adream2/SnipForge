//! 录屏。
//!
//! 分层：
//!   - `mf`     —— Media Foundation H.264/MP4 写入器（纯编码，不含业务）
//!   - `cursor` —— 把真实鼠标光标合进画面（DXGI 帧不含光标）
//!   - 本文件   —— 会话：取帧、定拍、裁剪、暂停、停止
//!
//! 取帧走 xcap 的 DXGI 桌面复制（GPU 抓帧，只在画面变化时出帧），
//! 编码走系统自带的 H.264 编码器 —— 两者都不增加安装体积。
//!
//! 关键取舍：
//!   - **恒定帧率输出**。DXGI 只在画面变化时出帧，如果按"有帧就写"，
//!     静止画面会让时间轴出现空洞，播放器一卡一卡。所以按 1/fps 定拍，
//!     没新帧就重复上一帧 —— H.264 会把重复帧压到几乎不占码率。
//!   - **区域录制也抓整屏再裁**。DXGI 是按显示器复制的，没有"只复制一块区域"
//!     的接口；抓整屏再裁是唯一的做法，成本也多不了多少。

pub mod cursor;
pub mod mf;

use crate::error::{AppError, AppResult};
use crate::types::Rect;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use base64::Engine;
use cursor::CursorTracker;
use tauri::Emitter;

/// 放大镜取样的源尺寸（物理像素）。覆盖层按 CSS 放大 display 到 ~2 倍。
const LENS_SRC: u32 = 128;

const B64: base64::engine::GeneralPurpose = base64::engine::general_purpose::STANDARD;

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CursorEvt {
    /// 区域相对坐标（物理像素）
    pub x: i32,
    pub y: i32,
    /// 倒计时结束后才为 true，覆盖层据此决定画不画鼠标环
    pub recording: bool,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClickEvt {
    pub x: i32,
    pub y: i32,
    pub button: String,
}

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LensEvt {
    pub url: String,
    pub x: i32,
    pub y: i32,
}

/// 从帧里裁一块光标周围的小图，编成 PNG data URL 给覆盖层。
///
/// 取的是**已经画过光标**的帧，所以镜头里能看到放大后的光标本身 ——
/// 这正是"放大镜"该有的效果。
fn lens_data_url(frame: &[u8], fw: u32, fh: u32, cx: i32, cy: i32, size: u32) -> Option<String> {
    let half = (size / 2) as i32;
    let x0 = (cx - half).clamp(0, fw as i32 - 1);
    let y0 = (cy - half).clamp(0, fh as i32 - 1);
    let x1 = (x0 + size as i32).min(fw as i32);
    let y1 = (y0 + size as i32).min(fh as i32);
    let (w, h) = ((x1 - x0) as u32, (y1 - y0) as u32);
    if w < 8 || h < 8 {
        return None;
    }

    let stride = (fw * 4) as usize;
    let mut px = Vec::with_capacity((w * h * 4) as usize);
    for y in y0..y1 {
        let off = (y as usize) * stride + (x0 as usize) * 4;
        px.extend_from_slice(&frame[off..off + (w * 4) as usize]);
    }

    let img = image::RgbaImage::from_raw(w, h, px)?;
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(format!("data:image/png;base64,{}", B64.encode(png)))
}

/// 一次录制的参数
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordOptions {
    /// 物理像素，屏幕坐标
    pub rect: Rect,
    pub fps: u32,
    pub bitrate: u32,
    /// 是否把鼠标光标画进画面
    pub draw_cursor: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordStatus {
    pub running: bool,
    pub paused: bool,
    /// 倒计时结束后才为 true
    pub started: bool,
    pub lens: bool,
    /// 已录制时长（毫秒），不含暂停。等于视频时间轴的长度
    pub elapsed_ms: u64,
    pub frames: u64,
    pub fps: f64,
    pub path: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordResult {
    pub path: String,
    pub duration_ms: u64,
    pub frames: u64,
    pub bytes: u64,
    pub width: u32,
    pub height: u32,
}

/// 录制线程与外部共享的开关
pub struct Live {
    stop: AtomicBool,
    paused: AtomicBool,
    /// 是否已经开始正式写入帧。
    /// 开录前先跑一段"只采集不写入"的预热，把 DXGI 首帧的几百毫秒消耗掉，
    /// 否则用户点下录制后头半秒的画面是空的。
    started: AtomicBool,
    /// 放大镜是否开着（控制条上可以随时切）
    lens: AtomicBool,
    frames: AtomicU64,
    elapsed_ms: AtomicU64,
}

impl Live {
    fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            paused: AtomicBool::new(false),
            started: AtomicBool::new(false),
            lens: AtomicBool::new(false),
            frames: AtomicU64::new(0),
            elapsed_ms: AtomicU64::new(0),
        }
    }
    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::SeqCst)
    }
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }
    fn advance(&self, fps: u32) {
        let f = self.frames.fetch_add(1, Ordering::SeqCst) + 1;
        self.elapsed_ms
            .store(f * 1000 / fps.max(1) as u64, Ordering::SeqCst);
    }
}

/// 正在进行的录制
pub struct ActiveRecording {
    pub live: Arc<Live>,
    pub path: PathBuf,
    pub opts: RecordOptions,
    pub width: u32,
    pub height: u32,
    thread: Option<std::thread::JoinHandle<AppResult<RecordResult>>>,
}

impl ActiveRecording {
    /// 正式开始写入（倒计时结束后调）
    pub fn begin(&self) {
        self.live.started.store(true, Ordering::SeqCst);
    }

    pub fn set_lens(&self, on: bool) {
        self.live.lens.store(on, Ordering::SeqCst);
    }

    pub fn pause(&self) {
        self.live.paused.store(true, Ordering::SeqCst);
    }

    pub fn resume(&self) {
        self.live.paused.store(false, Ordering::SeqCst);
    }

    pub fn status(&self) -> RecordStatus {
        let frames = self.live.frames.load(Ordering::SeqCst);
        let elapsed = self.live.elapsed_ms.load(Ordering::SeqCst);
        RecordStatus {
            running: true,
            paused: self.live.is_paused(),
            started: self.live.is_started(),
            lens: self.live.lens.load(Ordering::SeqCst),
            elapsed_ms: elapsed,
            frames,
            fps: if elapsed > 0 {
                frames as f64 * 1000.0 / elapsed as f64
            } else {
                0.0
            },
            path: self.path.to_string_lossy().to_string(),
            width: self.width,
            height: self.height,
        }
    }

    /// 停止并等录制线程收尾，返回最终文件信息。
    pub fn stop(mut self) -> AppResult<RecordResult> {
        self.live.stop.store(true, Ordering::SeqCst);
        self.live.paused.store(false, Ordering::SeqCst);
        match self.thread.take() {
            Some(t) => match t.join() {
                Ok(r) => r,
                Err(_) => Err(AppError::Msg("录制线程异常退出".into())),
            },
            None => Err(AppError::Msg("录制线程不存在".into())),
        }
    }
}

/// 开录。立刻返回，真正的采集在后台线程里跑。
pub fn start(app: Option<tauri::AppHandle>, opts: RecordOptions, path: PathBuf) -> AppResult<ActiveRecording> {
    // 尺寸取偶数：H.264 的宏块要求，奇数宽高会被编码器拒
    let width = opts.rect.width & !1;
    let height = opts.rect.height & !1;
    if width < 16 || height < 16 {
        return Err(AppError::Msg("录制区域太小（至少 16×16）".into()));
    }

    // Monitor 内含裸指针、不是 Send，不能跨线程传；
    // 这里只做一次"能不能找到显示器"的前置检查，真正取句柄在录制线程里做。
    let _ = pick_monitor(&opts.rect)?;
    let live = Arc::new(Live::new());
    let live2 = live.clone();
    let opts2 = opts.clone();
    let path2 = path.clone();

    let thread = std::thread::Builder::new()
        .name("snipforge-record".into())
        .spawn(move || {
            let r = run(app.clone(), opts2, path2, width, height, live2);
            // 录制线程的失败以前只会躺在 JoinHandle 里，直到用户点停止才暴露。
            // 采集中途挂掉的话控制条还在计时，用户完全不知情 —— 必须立刻喊出来。
            if let Err(e) = &r {
                log::error!("录制线程失败: {e}");
                if let Some(app) = &app {
                    let _ = app.emit("record://error", e.to_string());
                }
            }
            r
        })
        .map_err(|e| AppError::Msg(format!("启动录制线程失败: {e}")))?;

    Ok(ActiveRecording {
        live,
        path,
        opts,
        width,
        height,
        thread: Some(thread),
    })
}

/// 找到与区域重叠面积最大的显示器
fn pick_monitor(rect: &Rect) -> AppResult<xcap::Monitor> {
    let monitors = xcap::Monitor::all().map_err(|e| AppError::Msg(format!("枚举显示器失败: {e}")))?;
    let mut best: Option<(i64, xcap::Monitor)> = None;
    for m in monitors {
        let (mx, my) = (m.x().unwrap_or(0), m.y().unwrap_or(0));
        let (mw, mh) = (m.width().unwrap_or(0) as i32, m.height().unwrap_or(0) as i32);
        let x0 = rect.x.max(mx);
        let y0 = rect.y.max(my);
        let x1 = (rect.x + rect.width as i32).min(mx + mw);
        let y1 = (rect.y + rect.height as i32).min(my + mh);
        let area = (x1 - x0).max(0) as i64 * (y1 - y0).max(0) as i64;
        if area > 0 && best.as_ref().map(|(a, _)| area > *a).unwrap_or(true) {
            best = Some((area, m));
        }
    }
    best.map(|(_, m)| m)
        .ok_or_else(|| AppError::Msg("录制区域不在任何显示器上".into()))
}

fn run(
    app: Option<tauri::AppHandle>,
    opts: RecordOptions,
    path: PathBuf,
    width: u32,
    height: u32,
    live: Arc<Live>,
) -> AppResult<RecordResult> {
    let monitor = pick_monitor(&opts.rect)?;
    let mx = monitor.x().unwrap_or(0);
    let my = monitor.y().unwrap_or(0);
    let mw = monitor.width().unwrap_or(width);
    let mh = monitor.height().unwrap_or(height);

    // 区域相对显示器左上角的偏移
    let ox = (opts.rect.x - mx).max(0);
    let oy = (opts.rect.y - my).max(0);
    if ox + width as i32 > mw as i32 || oy + height as i32 > mh as i32 {
        return Err(AppError::Msg(format!(
            "录制区域超出显示器范围（区域 {width}×{height}，偏移 {ox},{oy}，显示器 {mw}×{mh}）"
        )));
    }

    // 桌面复制是独占资源：上一次会话刚释放时，马上再抢会返回 E_INVALIDARG。
    // 用户"录完马上再录一段"是很常见的操作，所以这里重试几次再报错。
    let mut attempt = 0;
    let (recorder, rx) = loop {
        match monitor.video_recorder() {
            Ok(v) => break v,
            Err(e) => {
                attempt += 1;
                if attempt >= 4 {
                    return Err(AppError::Msg(format!(
                        "启动屏幕采集失败（桌面复制被占用，请稍后重试）: {e}"
                    )));
                }
                log::warn!("屏幕采集启动失败，{}ms 后重试: {e}", 150 * attempt);
                std::thread::sleep(Duration::from_millis(150 * attempt as u64));
            }
        }
    };
    recorder
        .start()
        .map_err(|e| AppError::Msg(format!("屏幕采集启动失败: {e}")))?;

    let mut writer = mf::Mp4Writer::new(
        &path,
        width,
        height,
        opts.fps,
        opts.bitrate.max(500_000),
    )?;

    let frame_dur = Duration::from_nanos(1_000_000_000u64 / opts.fps.max(1) as u64);
    let mut next_tick = Instant::now();
    let mut last_frame: Option<Vec<u8>> = None;
    let mut cropped = vec![0u8; (width * height * 4) as usize];
    let mut tracker = CursorTracker::default();
    let stride = (width * 4) as usize;
    let mut last_lens = Instant::now();
    let mut warmed = false;

    log::info!(
        "开始录制 {}×{} @{}fps 到 {}（显示器 {}×{} 偏移 {ox},{oy}）",
        width,
        height,
        opts.fps,
        path.display(),
        mw,
        mh
    );

    loop {
        if live.stop.load(Ordering::SeqCst) {
            break;
        }

        if live.paused.load(Ordering::SeqCst) {
            // 暂停期间不取帧也不写帧，时间轴自然停住；
            // 重新对齐节拍，避免恢复瞬间补写一大串旧帧
            std::thread::sleep(Duration::from_millis(20));
            next_tick = Instant::now();
            continue;
        }

        // 在下一个节拍到来之前，把队列里的帧全部收掉，只留最新的一个。
        // 不这么做的话，60Hz 的屏幕更新会把 30fps 的时间轴拖乱。
        while Instant::now() < next_tick {
            let left = next_tick.saturating_duration_since(Instant::now());
            match rx.recv_timeout(left) {
                Ok(f) => last_frame = Some(f.raw),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    log::warn!("屏幕采集通道断开");
                    last_frame = last_frame.take();
                    break;
                }
            }
        }

        if let Some(raw) = &last_frame {
            // 裁出区域。整屏帧是 RGBA、自上而下、无行填充
            for y in 0..height {
                let sy = (oy as u32 + y) as usize;
                let src_off = (sy * mw as usize + ox as usize) * 4;
                let dst_off = y as usize * stride;
                if src_off + stride <= raw.len() {
                    cropped[dst_off..dst_off + stride]
                        .copy_from_slice(&raw[src_off..src_off + stride]);
                }
            }

            // 光标采样：即使不画进画面也要采样，覆盖层的高亮环靠它定位
            let pos = tracker.poll();
            let rel = pos.map(|p| (p.x - opts.rect.x, p.y - opts.rect.y));

            if let Some(p) = pos {
                if opts.draw_cursor {
                    let lx = p.x - p.hot_x - opts.rect.x;
                    let ly = p.y - p.hot_y - opts.rect.y;
                    tracker.composite(&mut cropped, width, height, lx, ly);
                }
            }

            // 光标位置推给覆盖层（30Hz 足够画环，肉眼无延迟感）
            if let (Some(app), Some((x, y))) = (&app, rel) {
                let _ = app.emit(
                    "record://cursor",
                    CursorEvt {
                        x,
                        y,
                        recording: live.is_started(),
                    },
                );
            }

            // 点击波纹：低位边沿，短促点击也漏不掉
            let (l, r) = cursor::poll_clicks();
            if let (Some(app), Some((x, y))) = (&app, rel) {
                if l {
                    let _ = app.emit("record://click", ClickEvt { x, y, button: "left".into() });
                }
                if r {
                    let _ = app.emit("record://click", ClickEvt { x, y, button: "right".into() });
                }
            }

            if live.is_started() {
                if !warmed {
                    warmed = true;
                    next_tick = Instant::now();
                }
                if let Err(e) = writer.write_rgba(&cropped) {
                    let _ = recorder.stop();
                    return Err(e);
                }
                live.advance(opts.fps);

                // 放大镜：把光标周围裁一小块推给覆盖层，由它放大显示。
                // 12Hz 足够（这是预览，不是画面主体），PNG 无损避免二次压缩。
                if live.lens.load(Ordering::SeqCst) && last_lens.elapsed() >= Duration::from_millis(83) {
                    if let (Some(app), Some((x, y))) = (&app, rel) {
                        if let Some(url) = lens_data_url(&cropped, width, height, x, y, LENS_SRC) {
                            let _ = app.emit("record://lens", LensEvt { url, x, y });
                        }
                    }
                    last_lens = Instant::now();
                }
            }
        }

        next_tick += frame_dur;
        let now = Instant::now();
        if next_tick < now {
            // 落后超过一帧就重新对齐，宁可丢几帧也不要把旧帧挤在一起写
            next_tick = now;
        }
    }

    let frames = writer.frames_written();
    writer.finish()?;
    let _ = recorder.stop();

    let bytes = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    let duration_ms = (frames as u64) * 1000 / opts.fps.max(1) as u64;
    log::info!("录制结束：{frames} 帧，{duration_ms}ms，{} KB", bytes / 1024);

    Ok(RecordResult {
        path: path.to_string_lossy().to_string(),
        duration_ms,
        frames: frames as u64,
        bytes,
        width,
        height,
    })
}

/// 光标位图自检：把系统当前光标导成 PNG。
pub fn cursor_smoke(path: &std::path::Path) -> AppResult<(i32, i32)> {
    let mut t = CursorTracker::default();
    let pos = t.poll();
    println!("光标位置: {pos:?}");
    if let Some((n, bbox, amin, amax)) = t.stats() {
        println!("位图统计: 不透明 {n} 像素，包围盒 {bbox:?}，alpha {amin}..{amax}");
    } else {
        println!("位图统计: 空");
    }
    t.dump(path)
}

/// 编码器自检：写一段 2 秒的合成动画，再读回时长。
///
/// 单独拎出来是为了能把「编码器坏了」和「取帧坏了」分开定位 ——
/// 录出来是黑屏还是打不开，排查方向完全不同。
pub fn encoder_smoke(path: &std::path::Path, fps: u32, seconds: u32) -> AppResult<(u64, i64)> {
    const W: u32 = 640;
    const H: u32 = 360;
    let mut writer = mf::Mp4Writer::new(path, W, H, fps, 2_000_000)?;

    let total = fps * seconds;
    let mut frame = vec![0u8; (W * H * 4) as usize];
    for i in 0..total {
        // 一条从左往右扫的亮条，肉眼能确认帧在动
        let head = (i * W / total.max(1)) as i32;
        for y in 0..H {
            for x in 0..W {
                let o = ((y * W + x) * 4) as usize;
                let d = (x as i32 - head).abs();
                let lit = d < 40;
                frame[o] = if lit { 255 } else { 30 };
                frame[o + 1] = if lit { 220 } else { 32 };
                frame[o + 2] = if lit { 120 } else { 40 };
                frame[o + 3] = 255;
            }
        }
        writer.write_rgba(&frame)?;
    }
    let frames = writer.frames_written();
    writer.finish()?;

    let ms = mf::probe_duration_ms(path)?;
    let expect = (seconds * 1000) as i64;
    if (ms as i64 - expect).abs() > 400 {
        return Err(AppError::Msg(format!(
            "时长对不上：期望约 {expect}ms，实际 {ms}ms"
        )));
    }
    Ok((ms, frames))
}
