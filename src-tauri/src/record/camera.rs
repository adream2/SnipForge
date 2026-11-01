//! 摄像头采集 + 画中画合成。
//!
//! 为什么把画中画**合进画面**而不是让前端在覆盖层里画：
//!   覆盖层窗口是独立窗口，一旦被别的窗口盖住，录进去的就是"被盖住的摄像头"。
//!   合成进主帧才是所见即所得 —— 这也是 OBS 一类工具的做法。
//!
//! 三个关键取舍：
//!
//! 1. **采集跑在独立线程，主循环只取"最新一帧"。**
//!    摄像头是异步设备，如果主循环每次同步读一帧，编码一慢就会积压，
//!    延迟越录越大（典型的"口型越走越偏"）。独立线程持续把旧帧丢掉，
//!    主循环拿到的永远是最新的。
//!
//! 2. **缩放交给 Media Foundation 做（SourceReader 的 advanced video processing）。**
//!    每帧在 CPU 上从 1080p 缩到 320p，1440p 录制会直接掉帧。
//!    设输出类型时就把目标宽高写进去，MF 会用视频处理器 MFT 走优化路径。
//!    只有在 MF 拒绝缩放时才退回 CPU 双线性（`scale_bilinear`）。
//!
//! 3. **RGB32 的字节序是 BGRA，且 stride 可能是负数（bottom-up DIB）。**
//!    不按 stride 读就会得到一条条斜的彩带；不交换 R/B 人脸会变蓝。

use crate::error::{AppError, AppResult};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{CoInitializeEx, CoTaskMemFree, COINIT_MULTITHREADED};

/// SourceReader 的"第一个视频流"是个负数常量，转成 u32 传给接口
const FIRST_VIDEO: u32 = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;

fn startup() -> AppResult<()> {
    // 复用 mf.rs 里的 OnceLock：MFStartup 每进程只能成功一次
    super::mf::ensure_startup()
}

/// 一个摄像头设备
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CameraDevice {
    /// 用设备名当 id：序号会随插拔变化，名字不会
    pub id: String,
    pub name: String,
}

unsafe fn activate_name(a: &IMFActivate) -> String {
    let mut buf = vec![0u16; 256];
    match a.GetString(&MF_DEVSOURCE_ATTRIBUTE_FRIENDLY_NAME, &mut buf, None) {
        Ok(()) => {
            let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
            String::from_utf16_lossy(&buf[..end])
        }
        Err(_) => String::new(),
    }
}

/// 枚举所有视频采集设备。
///
/// 没有摄像头不是错误 —— 返回空列表，让界面显示"未检测到摄像头"。
pub fn list_devices() -> AppResult<Vec<CameraDevice>> {
    startup()?;
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 1)
            .map_err(|e| AppError::Msg(format!("创建设备枚举属性失败: {e}")))?;
        let attrs = attrs.ok_or_else(|| AppError::Msg("设备枚举属性为空".into()))?;
        attrs
            .SetGUID(
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
                &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
            )
            .map_err(|e| AppError::Msg(format!("设置设备类型失败: {e}")))?;

        let mut raw: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count: u32 = 0;
        if let Err(e) = MFEnumDeviceSources(&attrs, &mut raw, &mut count) {
            // 一台设备都没有时这里也会报错，按"没有设备"处理
            log::debug!("枚举摄像头失败（可能没有设备）: {e}");
            return Ok(Vec::new());
        }
        if raw.is_null() || count == 0 {
            return Ok(Vec::new());
        }

        // take() 把所有权从原始数组里搬出来，drop 时才会正确 Release；
        // 直接 clone 会漏掉原始数组那一份引用
        let mut list = Vec::with_capacity(count as usize);
        for i in 0..count as usize {
            if let Some(a) = (*raw.add(i)).take() {
                let name = activate_name(&a);
                if !name.is_empty() {
                    list.push(CameraDevice {
                        id: name.clone(),
                        name,
                    });
                }
            }
        }
        CoTaskMemFree(Some(raw as *const core::ffi::c_void));

        log::info!("检测到 {} 个摄像头", list.len());
        Ok(list)
    }
}

/// 一帧已经转成 RGBA、自上而下、无行填充的画面
#[derive(Debug)]
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    /// 采集时的序号，主循环靠它判断"有没有新帧"
    pub seq: u64,
}

/// 正在运行的摄像头采集
pub struct CameraCapture {
    latest: Arc<Mutex<Option<Arc<Frame>>>>,
    stop: Arc<AtomicBool>,
    /// 采集线程活着（掉线 / 被占用会自己退出）
    alive: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
    /// 实际输出尺寸（按摄像头宽高比算出来的，不一定等于请求值）
    pub width: u32,
    pub height: u32,
    pub device: String,
}

impl CameraCapture {
    /// 打开摄像头并按 `width × height` 输出。
    ///
    /// 失败时调用方应该**降级为不录人像**，不要因为摄像头打不开就整场录制失败。
    pub fn start(device: &str, width: u32, height: u32) -> AppResult<Self> {
        startup()?;
        let width = (width & !1).max(64);
        let height = (height & !1).max(48);

        let latest: Arc<Mutex<Option<Arc<Frame>>>> = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let alive = Arc::new(AtomicBool::new(true));

        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(String, u32, u32), String>>();

        let t_latest = latest.clone();
        let t_stop = stop.clone();
        let t_alive = alive.clone();
        let want = device.to_string();

        let thread = std::thread::Builder::new()
            .name("snipforge-camera".into())
            .spawn(move || {
                let result = capture_loop(
                    &want,
                    width,
                    height,
                    &t_latest,
                    &t_stop,
                    ready_tx,
                );
                if let Err(e) = result {
                    log::warn!("摄像头采集中断: {e}");
                }
                t_alive.store(false, Ordering::SeqCst);
                // 线程退出时把画面清掉：主循环下一帧就不贴了，
                // 而不是一直贴最后一张定格画面
                *t_latest.lock().unwrap_or_else(|e| e.into_inner()) = None;
            })
            .map_err(|e| AppError::Msg(format!("启动摄像头线程失败: {e}")))?;

        // 等第一帧，把"设备被占用 / 没有权限"这类错误同步报给用户，
        // 而不是等到录完才发现没人像
        match ready_rx.recv_timeout(std::time::Duration::from_secs(8)) {
            Ok(Ok((name, w, h))) => {
                log::info!("摄像头已就绪: {name}（{w}×{h}）");
                Ok(Self {
                    latest,
                        stop,
                    alive,
                    thread: Some(thread),
                    width: w,
                    height: h,
                    device: name,
                })
            }
            Ok(Err(e)) => {
                stop.store(true, Ordering::SeqCst);
                let _ = thread.join();
                Err(AppError::Msg(e))
            }
            Err(_) => {
                // 8 秒还没出第一帧：有些摄像头冷启动确实慢，先放行
                log::warn!("摄像头 8 秒内未出首帧，继续尝试");
                Ok(Self {
                    latest,
                        stop,
                    alive,
                    thread: Some(thread),
                    width,
                    height,
                    device: device.to_string(),
                })
            }
        }
    }

    /// 取最新一帧（同一帧重复调用会返回 None，省掉重复合成）
    pub fn take_latest(&self, last_seq: &mut u64) -> Option<Arc<Frame>> {
        let guard = self.latest.lock().unwrap_or_else(|e| e.into_inner());
        let frame = guard.as_ref()?;
        if frame.seq == *last_seq {
            return None;
        }
        *last_seq = frame.seq;
        Some(frame.clone())
    }

    /// 采集线程还活着吗（掉线后主循环据此停止合成）
    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::SeqCst)
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for CameraCapture {
    fn drop(&mut self) {
        self.stop();
    }
}

/// 采集线程主体。返回 Err 表示设备出错、线程应当结束。
fn capture_loop(
    want: &str,
    width: u32,
    height: u32,
    latest: &Arc<Mutex<Option<Arc<Frame>>>>,
    stop: &Arc<AtomicBool>,
    ready: std::sync::mpsc::Sender<Result<(String, u32, u32), String>>,
) -> AppResult<()> {
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

        // ---- 1. 找到设备 ----
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.ok_or_else(|| AppError::Msg("设备属性为空".into()))?;
        attrs.SetGUID(
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE,
            &MF_DEVSOURCE_ATTRIBUTE_SOURCE_TYPE_VIDCAP_GUID,
        )?;

        let mut raw: *mut Option<IMFActivate> = std::ptr::null_mut();
        let mut count: u32 = 0;
        MFEnumDeviceSources(&attrs, &mut raw, &mut count)
            .map_err(|e| AppError::Msg(format!("枚举摄像头失败: {e}")))?;

        let mut chosen: Option<IMFActivate> = None;
        let mut fallback: Option<IMFActivate> = None;
        for i in 0..count as usize {
            let Some(a) = (*raw.add(i)).take() else {
                continue;
            };
            let name = activate_name(&a);
            if fallback.is_none() {
                fallback = Some(a.clone());
            }
            // 用户指定的设备优先；名字对不上（换了设备）就用第一个
            if !want.is_empty() && name == want {
                chosen = Some(a);
                break;
            }
        }
        if !raw.is_null() {
            CoTaskMemFree(Some(raw as *const core::ffi::c_void));
        }
        let activate = chosen.or(fallback).ok_or_else(|| {
            let msg = "没有可用的摄像头".to_string();
            let _ = ready.send(Err(msg.clone()));
            AppError::Msg(msg)
        })?;
        let name = activate_name(&activate);

        // ---- 2. 建 SourceReader ----
        let source = MFCreateDeviceSource(&activate)
            .map_err(|e| AppError::Msg(format!("打开摄像头失败（可能被其他程序占用）: {e}")))?;

        let mut rattrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut rattrs, 2)?;
        let rattrs = rattrs.ok_or_else(|| AppError::Msg("采集属性为空".into()))?;
        // 让 MF 帮我们做「格式转换 + 缩放」，CPU 就不用每帧缩一遍
        let _ = rattrs.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1);
        let _ = rattrs.SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1);

        let reader = MFCreateSourceReaderFromMediaSource(&source, &rattrs)
            .map_err(|e| AppError::Msg(format!("创建采集器失败: {e}")))?;

        // 只要视频流，音频流不选（选了会白占带宽，还会让 ReadSample 返回音轨）
        let _ = reader.SetStreamSelection(u32::MAX, false);
        reader
            .SetStreamSelection(FIRST_VIDEO, true)
            .map_err(|e| AppError::Msg(format!("选择视频流失败: {e}")))?;

        // ---- 3. 设输出格式：RGB32 + 目标尺寸 ----
        //
        // 高度按**源宽高比**算，不写死 16:9：4:3 的摄像头硬拉成 16:9 人脸会变宽，
        // 而这是用户第一眼就看出来的失真。
        let aspect = native_aspect(&reader).unwrap_or(16.0 / 9.0);
        let out_w = width & !1;
        let out_h = if height > 0 {
            height & !1
        } else {
            (((out_w as f32 / aspect.max(0.2)) as u32).max(48)) & !1
        };
        let mut scaled = true;
        if let Err(e) = set_output(&reader, out_w, out_h) {
            log::warn!("摄像头不支持直接缩放到 {out_w}×{out_h}，改用原始尺寸: {e}");
            scaled = false;
            set_output(&reader, 0, 0)?;
        }

        let cur = reader.GetCurrentMediaType(FIRST_VIDEO)?;
        let packed = cur.GetUINT64(&MF_MT_FRAME_SIZE).unwrap_or(0);
        let src_w = (packed >> 32) as u32;
        let src_h = (packed & 0xFFFF_FFFF) as u32;
        if src_w == 0 || src_h == 0 {
            return Err(AppError::Msg("摄像头没有返回有效的画面尺寸".into()));
        }
        // stride 可能是负数（bottom-up），绝对值才是每行字节数
        let stride = cur
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .map(|v| v as i32)
            .unwrap_or(src_w as i32 * 4);

        log::info!(
            "摄像头格式：{src_w}×{src_h} stride={stride} 缩放={}",
            if scaled { "MF" } else { "CPU" }
        );

        let mut ready_sent = false;
        let mut n: u64 = 0;

        while !stop.load(Ordering::SeqCst) {
            let mut flags: u32 = 0;
            let mut sample: Option<IMFSample> = None;
            // 同步读：没有帧就阻塞在这里，摄像头掉线会返回错误
            reader
                .ReadSample(
                    FIRST_VIDEO,
                    0,
                    None,
                    Some(&mut flags),
                    None,
                    Some(&mut sample),
                )
                .map_err(|e| {
                    if !ready_sent {
                        let _ = ready.send(Err(format!("读取摄像头画面失败: {e}")));
                        ready_sent = true;
                    }
                    AppError::Msg(format!("读取摄像头画面失败: {e}"))
                })?;

            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Err(AppError::Msg("摄像头流已结束（设备被拔出？）".into()));
            }

            let Some(sample) = sample else {
                continue;
            };

            // ---- 4. 转成 RGBA ----
            let rgba = match read_rgba(&sample, src_w, src_h, stride) {
                Ok(v) => v,
                Err(e) => {
                    log::warn!("摄像头帧转换失败，跳过: {e}");
                    continue;
                }
            };

            let (fw, fh, pixels) = if scaled || (src_w == out_w && src_h == out_h) {
                (src_w, src_h, rgba)
            } else {
                // 兜底路径：MF 不肯缩放，只能自己缩
                let v = scale_bilinear(&rgba, src_w, src_h, out_w, out_h);
                (out_w, out_h, v)
            };

            n += 1;
            let frame = Arc::new(Frame {
                width: fw,
                height: fh,
                rgba: pixels,
                seq: n,
            });
            *latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame);

            if !ready_sent {
                ready_sent = true;
                let _ = ready.send(Ok((name.clone(), fw, fh)));
            }
        }
    }
    Ok(())
}

/// 读摄像头原生画面的宽高比（用来决定画中画高度，避免拉伸变形）
unsafe fn native_aspect(reader: &IMFSourceReader) -> Option<f32> {
    let t = reader.GetNativeMediaType(FIRST_VIDEO, 0).ok()?;
    let packed = t.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
    let w = (packed >> 32) as u32;
    let h = (packed & 0xFFFF_FFFF) as u32;
    if w == 0 || h == 0 {
        None
    } else {
        Some(w as f32 / h as f32)
    }
}

/// 设输出类型。`w`/`h` 为 0 表示不指定尺寸（只要格式转换）。
unsafe fn set_output(reader: &IMFSourceReader, w: u32, h: u32) -> AppResult<()> {
    let out = MFCreateMediaType()?;
    out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
    out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
    if w > 0 && h > 0 {
        out.SetUINT64(&MF_MT_FRAME_SIZE, ((w as u64) << 32) | h as u64)?;
        // 30fps 只是"期望值"，设备会挑最接近的
        out.SetUINT64(&MF_MT_FRAME_RATE, (30u64 << 32) | 1)?;
    }
    reader
        .SetCurrentMediaType(FIRST_VIDEO, None, &out)
        .map_err(|e| AppError::Msg(format!("设置摄像头输出格式失败: {e}")))
}

/// IMFMediaBuffer 里的一帧 BGRA → RGBA（自上而下、无行填充）
unsafe fn read_rgba(
    sample: &IMFSample,
    width: u32,
    height: u32,
    stride: i32,
) -> AppResult<Vec<u8>> {
    let buffer: IMFMediaBuffer = sample.ConvertToContiguousBuffer()?;
    let mut ptr: *mut u8 = std::ptr::null_mut();
    let mut len: u32 = 0;
    buffer.Lock(&mut ptr, None, Some(&mut len))?;

    let result = (|| -> AppResult<Vec<u8>> {
        if ptr.is_null() {
            return Err(AppError::Msg("摄像头缓冲为空".into()));
        }
        let row_bytes = (width * 4) as usize;
        let abs_stride = stride.unsigned_abs() as usize;
        if abs_stride < row_bytes {
            return Err(AppError::Msg(format!(
                "摄像头行跨度过小（stride={stride}，需要 ≥{row_bytes}）"
            )));
        }

        let mut out = vec![0u8; row_bytes * height as usize];
        for y in 0..height as usize {
            // 负 stride = 第一行在缓冲区末尾，要倒着数
            let src_y = if stride < 0 { height as usize - 1 - y } else { y };
            let off = src_y * abs_stride;
            if off + row_bytes > len as usize {
                return Err(AppError::Msg("摄像头帧数据长度不足".into()));
            }
            let src = std::slice::from_raw_parts(ptr.add(off), row_bytes);
            let dst = &mut out[y * row_bytes..(y + 1) * row_bytes];
            // BGRA → RGBA
            for x in 0..width as usize {
                let s = x * 4;
                dst[s] = src[s + 2];
                dst[s + 1] = src[s + 1];
                dst[s + 2] = src[s];
                dst[s + 3] = 255; // A 通道在 RGB32 里不可信，直接填满
            }
        }
        Ok(out)
    })();

    let _ = buffer.Unlock();
    result
}

/// CPU 双线性缩放（只在 MF 拒绝缩放时用）
pub fn scale_bilinear(src: &[u8], sw: u32, sh: u32, dw: u32, dh: u32) -> Vec<u8> {
    let mut out = vec![0u8; (dw * dh * 4) as usize];
    if sw == 0 || sh == 0 || dw == 0 || dh == 0 {
        return out;
    }
    let xr = sw as f32 / dw as f32;
    let yr = sh as f32 / dh as f32;
    for y in 0..dh as usize {
        let fy = ((y as f32 + 0.5) * yr - 0.5).max(0.0);
        let y0 = fy as usize;
        let y1 = (y0 + 1).min(sh as usize - 1);
        let wy = fy - y0 as f32;
        for x in 0..dw as usize {
            let fx = ((x as f32 + 0.5) * xr - 0.5).max(0.0);
            let x0 = fx as usize;
            let x1 = (x0 + 1).min(sw as usize - 1);
            let wx = fx - x0 as f32;

            let p = |px: usize, py: usize| -> &[u8] {
                let o = (py * sw as usize + px) * 4;
                &src[o..o + 4]
            };
            let a = p(x0, y0);
            let b = p(x1, y0);
            let c = p(x0, y1);
            let d = p(x1, y1);
            let o = (y * dw as usize + x) * 4;
            for ch in 0..4 {
                let top = a[ch] as f32 * (1.0 - wx) + b[ch] as f32 * wx;
                let bot = c[ch] as f32 * (1.0 - wx) + d[ch] as f32 * wx;
                out[o + ch] = (top * (1.0 - wy) + bot * wy).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

/* ---------------- 画中画布局与合成 ---------------- */

/// 画中画的位置与尺寸（相对录制画面左上角，物理像素）
#[derive(Debug, Clone, Copy)]
pub struct PipLayout {
    pub x: i32,
    pub y: i32,
    pub w: u32,
    pub h: u32,
}

/// 按「占画面宽度的百分比 + 九宫格位置 + 边距」算出布局。
///
/// 全部用比例而不是绝对像素：同一份配置在 1080p 和 4K 下看起来一样大，
/// 换显示器不用重新调。
pub fn layout_for(
    frame_w: u32,
    frame_h: u32,
    width_pct: u32,
    margin_pct: u32,
    corner: &str,
    aspect: f32,
) -> PipLayout {
    let pct = width_pct.clamp(8, 45) as f32 / 100.0;
    let w = ((frame_w as f32 * pct) as u32).max(64) & !1;
    let h = (((w as f32 / aspect.max(0.2)) as u32).max(48)) & !1;
    place(frame_w, frame_h, w, h, margin_pct, corner)
}

/// 已知画中画确切尺寸时用它。
///
/// **合成必须走这个入口**：`composite` 是 1:1 贴图（缩放由 MF 在采集端做完），
/// 所以 `layout.w/h` 必须严格等于帧的宽高。用 `layout_for` 再算一遍的话，
/// 一旦采集端返回的尺寸和请求值差几个像素（设备对齐、驱动取整），
/// 就会裁掉一部分画面或者拉伸。
pub fn place(
    frame_w: u32,
    frame_h: u32,
    pip_w: u32,
    pip_h: u32,
    margin_pct: u32,
    corner: &str,
) -> PipLayout {
    let w = pip_w.max(16) & !1;
    let h = pip_h.max(16) & !1;
    let m = ((frame_w.min(frame_h) as f32 * margin_pct.clamp(0, 12) as f32 / 100.0) as i32).max(0);

    let (x, y) = match corner {
        "tl" => (m, m),
        "tr" => (frame_w as i32 - w as i32 - m, m),
        "bl" => (m, frame_h as i32 - h as i32 - m),
        _ => (frame_w as i32 - w as i32 - m, frame_h as i32 - h as i32 - m),
    };
    PipLayout { x, y, w, h }
}

/// 预生成的圆角遮罩。尺寸固定，只算一次 —— 每帧算距离场太浪费。
pub struct Mask {
    w: u32,
    h: u32,
    /// 内部覆盖率 0-255（圆角处做 1px 抗锯齿）
    alpha: Vec<u8>,
    /// 描边强度 0-255
    border: Vec<u8>,
}

impl Mask {
    /// `radius_pct` 是短边的百分比，`border_px` 是描边宽度
    pub fn rounded(w: u32, h: u32, radius_pct: u32, border_px: f32) -> Self {
        let r = (w.min(h) as f32 * radius_pct.clamp(0, 50) as f32 / 100.0).max(0.0);
        let bw = border_px.max(0.0);
        let mut alpha = vec![0u8; (w * h) as usize];
        let mut border = vec![0u8; (w * h) as usize];

        if r <= 0.5 {
            alpha.fill(255);
            if bw > 0.0 {
                // 直角时描边就是四条边
                for y in 0..h as usize {
                    for x in 0..w as usize {
                        let dx = x.min(w as usize - 1 - x) as f32;
                        let dy = y.min(h as usize - 1 - y) as f32;
                        let d = dx.min(dy);
                        if d < bw {
                            border[y * w as usize + x] = 255;
                        }
                    }
                }
            }
            return Self { w, h, alpha, border };
        }

        let half_w = w as f32 / 2.0;
        let half_h = h as f32 / 2.0;
        for y in 0..h as usize {
            let py = y as f32 + 0.5 - half_h;
            for x in 0..w as usize {
                let px = x as f32 + 0.5 - half_w;
                // 圆角矩形的有符号距离：<0 在内部
                let qx = px.abs() - (half_w - r);
                let qy = py.abs() - (half_h - r);
                let d = if qx > 0.0 && qy > 0.0 {
                    (qx * qx + qy * qy).sqrt() - r
                } else {
                    qx.max(qy) - r
                };
                let i = y * w as usize + x;
                // 1px 抗锯齿：边缘按覆盖率过渡
                alpha[i] = ((-d).clamp(0.0, 1.0) * 255.0) as u8;
                if bw > 0.0 {
                    // `-d` 是"到最近边缘的距离"（内部为正）。
                    // 描边占据 (0, bw) 这一段，靠内边界 1px 渐隐，
                    // 否则小尺寸下描边会是一条生硬的锯齿线。
                    //
                    // 注意这里不能写 `bw - (-d) > 0`：矩形内部离边缘很远时
                    // 那个值恒为负，结果是**只有靠近上下边的地方有描边**，
                    // 左右两条边完全没有（这个 bug 被 --pip-test 的断言抓到过）。
                    let dist = -d;
                    if dist > 0.0 && dist < bw {
                        border[i] = ((bw - dist).clamp(0.0, 1.0) * 255.0) as u8;
                    }
                }
            }
        }
        Self { w, h, alpha, border }
    }

    pub fn matches(&self, w: u32, h: u32) -> bool {
        self.w == w && self.h == h
    }
}

/// 把摄像头画面贴到 `dst` 上（RGBA，无行填充）。
///
/// 超出画面的部分自动裁掉 —— 用户把画中画放到边缘时不该写越界。
#[allow(clippy::too_many_arguments)]
pub fn composite(
    dst: &mut [u8],
    dst_w: u32,
    dst_h: u32,
    src: &[u8],
    src_w: u32,
    src_h: u32,
    layout: PipLayout,
    mirror: bool,
    mask: &Mask,
    border_color: [u8; 4],
) {
    if src_w != mask.w || src_h != mask.h {
        return;
    }
    let dst_stride = dst_w as usize * 4;
    let src_stride = src_w as usize * 4;

    // 用 layout.h 而不是 src_h 收尾：两者理论上相等，但源尺寸变化时
    // layout 是唯一可信的目标区域
    let y_limit = (layout.h as usize).min(src_h as usize);
    for y in 0..y_limit {
        let dy = layout.y + y as i32;
        if dy < 0 || dy >= dst_h as i32 {
            continue;
        }
        let src_row = &src[y * src_stride..(y + 1) * src_stride];
        let dst_off = dy as usize * dst_stride;
        let x_limit = (layout.w as usize).min(src_w as usize);

        for x in 0..x_limit {
            let dx = layout.x + x as i32;
            if dx < 0 || dx >= dst_w as i32 {
                continue;
            }
            let i = y * src_w as usize + x;
            let a = mask.alpha[i];
            let bo = mask.border[i];
            let d = dst_off + dx as usize * 4;

            if bo > 0 {
                // 描边在圆角内侧，和图像区域互斥，直接按强度混合即可。
                // 再乘一次 alpha：外边缘那 1px 描边也要跟着抗锯齿，
                // 否则圆角处会露出一圈硬边。
                let t = (bo as u32 * a as u32) / 255;
                for c in 0..4 {
                    let base = dst[d + c] as u32;
                    dst[d + c] = ((border_color[c] as u32 * t + base * (255 - t)) / 255) as u8;
                }
                continue;
            }
            if a == 0 {
                continue;
            }
            let sx = if mirror { src_w as usize - 1 - x } else { x };
            let s = sx * 4;
            let sp = &src_row[s..s + 4];
            if a == 255 {
                dst[d..d + 4].copy_from_slice(sp);
            } else {
                let t = a as u32;
                for c in 0..3 {
                    let base = dst[d + c] as u32;
                    dst[d + c] = ((sp[c] as u32 * t + base * (255 - t)) / 255) as u8;
                }
                dst[d + 3] = 255;
            }
        }
    }
}

/// 供 `--camera-smoke` 之类的自检用：把一张 RGBA 存成 PNG 看效果
#[allow(dead_code)]
pub fn grab_one(device: &str, w: u32, h: u32) -> AppResult<Vec<u8>> {
    let mut cap = CameraCapture::start(device, w, h)?;
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    let mut seq = 0u64;
    while std::time::Instant::now() < deadline {
        if let Some(f) = cap.take_latest(&mut seq) {
            let out = f.rgba.clone();
            cap.stop();
            return Ok(out);
        }
        std::thread::sleep(std::time::Duration::from_millis(30));
    }
    cap.stop();
    Err(AppError::Msg("6 秒内没有取到摄像头画面".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_hugs_each_corner() {
        let (w, h) = (1280u32, 720u32);
        let tl = layout_for(w, h, 20, 2, "tl", 16.0 / 9.0);
        let tr = layout_for(w, h, 20, 2, "tr", 16.0 / 9.0);
        let bl = layout_for(w, h, 20, 2, "bl", 16.0 / 9.0);
        let br = layout_for(w, h, 20, 2, "br", 16.0 / 9.0);

        assert!(tl.x > 0 && tl.y > 0);
        assert_eq!(tl.x, bl.x, "左侧两角 x 应一致");
        assert_eq!(tl.y, tr.y, "上侧两角 y 应一致");
        assert_eq!(tr.x + tr.w as i32 + tl.x, w as i32, "左右对称");
        assert_eq!(br.y + br.h as i32 + tl.y, h as i32, "上下对称");
        // 16:9 的 20% 宽：1280*0.2=256，高 144
        assert_eq!(tl.w, 256);
        assert_eq!(tl.h, 144);
    }

    #[test]
    fn layout_keeps_aspect() {
        // 4:3 的摄像头不该被拉成 16:9
        let l = layout_for(1920, 1080, 25, 2, "br", 4.0 / 3.0);
        assert_eq!(l.w, 480);
        assert_eq!(l.h, 360);
    }

    #[test]
    fn place_uses_exact_size() {
        let l = place(1280, 720, 300, 200, 2, "br");
        assert_eq!((l.w, l.h), (300, 200));
        assert_eq!(l.x, 1280 - 300 - 14);
    }

    #[test]
    fn mask_border_wraps_all_four_sides() {
        // 这个断言对应一个真实 bug：描边判定写错时只有上下两侧有描边
        let m = Mask::rounded(200, 120, 12, 4.0);
        let at = |x: u32, y: u32| m.border[(y * 200 + x) as usize];
        let (mx, my) = (100u32, 60u32);
        assert!(at(mx, 0) > 0, "上边缺描边");
        assert!(at(mx, 119) > 0, "下边缺描边");
        assert!(at(0, my) > 0, "左边缺描边");
        assert!(at(199, my) > 0, "右边缺描边");
        // 内部不该是描边
        assert_eq!(at(mx, my), 0, "内部被误画成描边");
    }

    #[test]
    fn mask_corners_are_transparent() {
        let m = Mask::rounded(200, 120, 12, 0.0);
        let at = |x: u32, y: u32| m.alpha[(y * 200 + x) as usize];
        assert_eq!(at(0, 0), 0, "左上角应该透出背景");
        assert_eq!(at(199, 119), 0, "右下角应该透出背景");
        assert_eq!(at(100, 60), 255, "中心应该完全不透明");
    }

    #[test]
    fn composite_stays_inside_frame() {
        // 放到右下角外侧时不能 panic、也不能写到画面外
        let mut dst = vec![7u8; 40 * 40 * 4];
        let src = vec![255u8; 20 * 20 * 4];
        let mask = Mask::rounded(20, 20, 0, 0.0);
        let layout = PipLayout {
            x: 35,
            y: 35,
            w: 20,
            h: 20,
        };
        composite(&mut dst, 40, 40, &src, 20, 20, layout, false, &mask, [255; 4]);
        assert_eq!(dst.len(), 40 * 40 * 4);
    }

    #[test]
    fn composite_mirrors_horizontally() {
        // 2x2 源图：左列红、右列蓝
        let mut src = Vec::new();
        for _ in 0..2 {
            src.extend_from_slice(&[255, 0, 0, 255]); // 左上 红
            src.extend_from_slice(&[0, 0, 255, 255]); // 右上 蓝
        }
        let mut dst = vec![0u8; 4 * 4 * 4];
        let mask = Mask::rounded(2, 2, 0, 0.0);
        let layout = PipLayout { x: 1, y: 1, w: 2, h: 2 };

        composite(&mut dst, 4, 4, &src, 2, 2, layout, true, &mask, [255; 4]);
        let px = |x: usize, y: usize| {
            let o = (y * 4 + x) * 4;
            [dst[o], dst[o + 1], dst[o + 2]]
        };
        assert_eq!(px(1, 1)[2], 255, "镜像后左上应该是蓝色");
        assert_eq!(px(2, 1)[0], 255, "镜像后右上应该是红色");
    }

    #[test]
    fn bilinear_scale_keeps_size() {
        let src = vec![128u8; 10 * 10 * 4];
        let out = scale_bilinear(&src, 10, 10, 25, 25);
        assert_eq!(out.len(), 25 * 25 * 4);
        assert!(out.iter().all(|v| *v == 128), "纯色缩放后应仍是纯色");
    }
}
