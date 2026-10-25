//! H.264 / MP4 编码 —— 直接用系统自带的 Media Foundation。
//!
//! 为什么不用 ffmpeg：
//!   打包进一个 ffmpeg 可执行文件，安装包立刻从 2.4MB 涨到 40MB+。
//!   而 Windows 从 7 起就自带 H.264 编码器（MFT），Media Foundation 的
//!   SinkWriter 会把「RGB32 输入 → NV12 → H.264 → MP4 容器」这条链自动搭好，
//!   我们只需要喂帧。
//!
//! 两个必须记住的细节：
//!   1. `MFStartup` 每进程只能成功一次，重复调用要么幂等要么报错，用 OnceLock 兜住；
//!   2. `MFVideoFormat_RGB32` 是 DIB 约定 —— **第一行是图像底部**，
//!      所以拷贝时要倒着写。搞反了录出来就是上下颠倒。

use crate::error::{AppError, AppResult};
use std::path::Path;
use std::sync::OnceLock;

use windows::core::PCWSTR;
use windows::Win32::Media::MediaFoundation::*;

/// H.264 的帧尺寸必须是偶数，宽高各让一下
fn even(v: u32) -> u32 {
    (v & !1).max(2)
}

/// MF 的 FRAME_SIZE / FRAME_RATE 都是「高 32 位 / 低 32 位」打包成一个 u64
fn pack(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

fn wide(path: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

fn startup() -> AppResult<()> {
    static INIT: OnceLock<Result<(), String>> = OnceLock::new();
    let result = INIT.get_or_init(|| unsafe {
        MFStartup(MF_VERSION, MFSTARTUP_FULL).map_err(|e| format!("MFStartup 失败: {e}"))
    });
    match result {
        Ok(()) => Ok(()),
        Err(e) => Err(AppError::Msg(e.clone())),
    }
}

/// 把 RGBA 帧写进 MP4。
///
/// 只支持逐帧顺序写入（录制就是这形态），不支持随机访问和音轨。
pub struct Mp4Writer {
    writer: IMFSinkWriter,
    stream: u32,
    width: u32,
    height: u32,
    /// 每帧时长，单位 100ns（MF 的时间基准）
    frame_dur: i64,
    index: i64,
    finished: bool,
}

impl Mp4Writer {
    pub fn new(
        path: &Path,
        width: u32,
        height: u32,
        fps: u32,
        bitrate: u32,
    ) -> AppResult<Self> {
        startup()?;
        let w = even(width);
        let h = even(height);
        let fps = fps.clamp(5, 120);

        unsafe {
            // ---- 输出类型：H.264 ----
            let out = MFCreateMediaType().map_err(|e| AppError::Msg(format!("创建媒体类型失败: {e}")))?;
            out.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            out.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_H264)?;
            out.SetUINT32(&MF_MT_AVG_BITRATE, bitrate)?;
            out.SetUINT32(
                &MF_MT_INTERLACE_MODE,
                MFVideoInterlace_Progressive.0 as u32,
            )?;
            out.SetUINT64(&MF_MT_FRAME_SIZE, pack(w, h))?;
            out.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
            // Main profile：兼容性最好，绝大多数播放器和网页都吃得下
            out.SetUINT32(&MF_MT_MPEG2_PROFILE, eAVEncH264VProfile_Main.0 as u32)?;

            let url = wide(path);
            let writer = MFCreateSinkWriterFromURL(
                PCWSTR(url.as_ptr()),
                None::<&IMFByteStream>,
                None::<&IMFAttributes>,
            )
            .map_err(|e| AppError::Msg(format!("创建 MP4 写入器失败: {e}")))?;

            let stream = writer
                .AddStream(&out)
                .map_err(|e| AppError::Msg(format!("添加视频流失败: {e}")))?;

            // ---- 输入类型：RGB32（RGBA 每像素 4 字节，字节序 B,G,R,A）----
            let input = MFCreateMediaType()?;
            input.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
            input.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
            input.SetUINT32(
                &MF_MT_INTERLACE_MODE,
                MFVideoInterlace_Progressive.0 as u32,
            )?;
            input.SetUINT64(&MF_MT_FRAME_SIZE, pack(w, h))?;
            input.SetUINT64(&MF_MT_FRAME_RATE, pack(fps, 1))?;
            input.SetUINT32(&MF_MT_DEFAULT_STRIDE, w * 4)?;
            input.SetUINT32(&MF_MT_ALL_SAMPLES_INDEPENDENT, 1)?;

            writer.SetInputMediaType(stream, &input, None::<&IMFAttributes>)
                .map_err(|e| AppError::Msg(format!("设置输入格式失败（H.264 编码器不可用？）: {e}")))?;
            writer.BeginWriting()?;

            Ok(Self {
                writer,
                stream,
                width: w,
                height: h,
                frame_dur: 10_000_000 / fps as i64,
                index: 0,
                finished: false,
            })
        }
    }

    pub fn size(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// 写入一帧。`rgba` 必须是 `width * height * 4` 字节、自上而下。
    pub fn write_rgba(&mut self, rgba: &[u8]) -> AppResult<()> {
        let stride = (self.width * 4) as usize;
        let need = stride * self.height as usize;
        if rgba.len() < need {
            return Err(AppError::Msg(format!(
                "帧数据不足：需要 {need} 字节，实际 {} 字节",
                rgba.len()
            )));
        }

        unsafe {
            let buffer = MFCreateMemoryBuffer(need as u32)?;
            let mut ptr = std::ptr::null_mut();
            buffer.Lock(&mut ptr, None, None)?;
            {
                let dst = std::slice::from_raw_parts_mut(ptr, need);
                // 倒着写：RGB32 是 DIB 约定，第一行对应图像底部。
                // 同时把 RGBA 换成 BGRA。
                for y in 0..self.height as usize {
                    let src = &rgba[y * stride..y * stride + stride];
                    let dst_off = (self.height as usize - 1 - y) * stride;
                    let row = &mut dst[dst_off..dst_off + stride];
                    for i in (0..stride).step_by(4) {
                        row[i] = src[i + 2];
                        row[i + 1] = src[i + 1];
                        row[i + 2] = src[i];
                        row[i + 3] = 255;
                    }
                }
            }
            buffer.Unlock()?;
            buffer.SetCurrentLength(need as u32)?;

            let sample = MFCreateSample()?;
            sample.AddBuffer(&buffer)?;
            sample.SetSampleTime(self.index * self.frame_dur)?;
            sample.SetSampleDuration(self.frame_dur)?;
            self.writer.WriteSample(self.stream, &sample)?;
            self.index += 1;
        }
        Ok(())
    }

    pub fn frames_written(&self) -> i64 {
        self.index
    }

    /// 收尾：不调用的话 MP4 的索引（moov）写不进去，文件打不开。
    pub fn finish(&mut self) -> AppResult<()> {
        if self.finished {
            return Ok(());
        }
        self.finished = true;
        unsafe {
            self.writer
                .Finalize()
                .map_err(|e| AppError::Msg(format!("MP4 收尾失败: {e}")))?;
        }
        Ok(())
    }
}

impl Drop for Mp4Writer {
    fn drop(&mut self) {
        // 异常路径也要尽量把文件收干净，否则留下一个 0 字节的坏文件
        let _ = self.finish();
    }
}

/* ------------------------ MP4 自检 ------------------------ */

/// 读 MP4 的时长，用来验证录出来的文件真的能播。
///
/// 不引 Media Foundation 的 SourceReader（多一层 COM 和 PROPVARIANT 处理），
/// 直接按 ISO BMFF 的盒子结构取 `mvhd` 里的 timescale/duration ——
/// 只有 30 行，而且坏了能一眼看出是容器问题还是编码问题。
pub fn probe_duration_ms(path: &Path) -> AppResult<u64> {
    let data = std::fs::read(path)?;
    let find = |tag: &[u8; 4]| -> Option<usize> {
        data.windows(4).position(|w| w == tag)
    };

    let moov = find(b"moov").ok_or_else(|| AppError::Msg("MP4 缺少 moov 盒子".into()))?;
    let mvhd = find(b"mvhd").ok_or_else(|| AppError::Msg("MP4 缺少 mvhd 盒子".into()))?;
    if moov == 0 {
        return Err(AppError::Msg("MP4 结构异常".into()));
    }

    let base = mvhd + 4;
    if data.len() < base + 20 {
        return Err(AppError::Msg("MP4 头部被截断".into()));
    }
    let version = data[base];
    let (timescale, duration) = if version == 1 {
        let ts = u32::from_be_bytes(data[base + 20..base + 24].try_into().unwrap()) as u64;
        let du = u64::from_be_bytes(data[base + 24..base + 32].try_into().unwrap());
        (ts, du)
    } else {
        let ts = u32::from_be_bytes(data[base + 12..base + 16].try_into().unwrap()) as u64;
        let du = u32::from_be_bytes(data[base + 16..base + 20].try_into().unwrap()) as u64;
        (ts, du)
    };
    if timescale == 0 {
        return Err(AppError::Msg("MP4 的 timescale 为 0".into()));
    }
    Ok(duration * 1000 / timescale)
}

/* ------------------------ 抽帧自检 ------------------------ */

/// 解出第一帧存成 PNG。
///
/// 用途有两个：验证编码结果没画反/颜色没串（RGB32 是 DIB 约定，最容易踩），
/// 以及给录制结果做封面图。
pub fn extract_frame(path: &Path, out: &Path, skip: u32) -> AppResult<(u32, u32)> {
    startup()?;
    unsafe {
        let url = wide(path);
        // 必须开"高级视频处理"：解码器原生只吐 NV12，要 RGB32 得让 SourceReader
        // 在中间插一个颜色转换。不开的话 SetCurrentMediaType 会返回 MF_E_INVALIDMEDIATYPE。
        let mut attrs: Option<IMFAttributes> = None;
        MFCreateAttributes(&mut attrs, 1)?;
        let attrs = attrs.ok_or_else(|| AppError::Msg("创建属性失败".into()))?;
        attrs.SetUINT32(&MF_SOURCE_READER_ENABLE_ADVANCED_VIDEO_PROCESSING, 1)?;

        let reader = MFCreateSourceReaderFromURL(PCWSTR(url.as_ptr()), &attrs)
            .map_err(|e| AppError::Msg(format!("打开视频失败: {e}")))?;

        // 让解码器直接输出 RGB32，省得自己转 YUV
        let want = MFCreateMediaType()?;
        want.SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)?;
        want.SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)?;
        let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
        reader
            .SetCurrentMediaType(stream, None, &want)
            .map_err(|e| AppError::Msg(format!("设置解码输出格式失败: {e}")))?;

        // 跳过前 skip 帧：第 0 帧往往在覆盖层渲染之前，看不出叠加内容
        let mut sample: Option<IMFSample> = None;
        let mut flags: u32 = 0;
        for _ in 0..=skip {
            sample = None;
            reader
                .ReadSample(stream, 0, None, Some(&mut flags), None, Some(&mut sample))
                .map_err(|e| AppError::Msg(format!("读取帧失败: {e}")))?;
            if sample.is_none() || flags & 0x2 != 0 {
                // MF_SOURCE_READERF_ENDOFSTREAM
                break;
            }
        }
        let sample = sample.ok_or_else(|| AppError::Msg("视频里没有可解码的帧".into()))?;

        // 尺寸优先取自"当前类型"，个别解码器不给就退回原生类型
        let size_of = |t: &IMFMediaType| -> Option<(u32, u32)> {
            let p = t.GetUINT64(&MF_MT_FRAME_SIZE).ok()?;
            Some(((p >> 32) as u32, (p & 0xFFFF_FFFF) as u32))
        };
        let current = reader.GetCurrentMediaType(stream)?;
        let (w, h) = size_of(&current)
            .or_else(|| {
                reader
                    .GetNativeMediaType(stream, 0)
                    .ok()
                    .and_then(|t| size_of(&t))
            })
            .ok_or_else(|| AppError::Msg("读不到视频尺寸".into()))?;

        // 行跨度：解码器通常给负值（底朝上），拿不到就按紧凑排列算
        let stride = current
            .GetUINT32(&MF_MT_DEFAULT_STRIDE)
            .map(|v| v as i32)
            .unwrap_or((w * 4) as i32);
        let stride = if stride == 0 { (w * 4) as i32 } else { stride };

        let buffer = sample.ConvertToContiguousBuffer()?;
        let mut ptr = std::ptr::null_mut();
        buffer.Lock(&mut ptr, None, None)?;

        // 按 stride 的正负还原成自上而下
        let mut rgba = vec![0u8; (w * h * 4) as usize];
        for y in 0..h as i32 {
            let src_row = if stride < 0 { -stride * y } else { stride * (h as i32 - 1 - y) };
            let src = std::slice::from_raw_parts(
                (ptr as *const u8).offset(src_row as isize),
                (w * 4) as usize,
            );
            let dst_off = (y as u32 * w * 4) as usize;
            let row = &mut rgba[dst_off..dst_off + (w * 4) as usize];
            for x in 0..w as usize {
                let i = x * 4;
                row[i] = src[i + 2];
                row[i + 1] = src[i + 1];
                row[i + 2] = src[i];
                row[i + 3] = 255;
            }
        }
        buffer.Unlock()?;

        let img = image::RgbaImage::from_raw(w, h, rgba)
            .ok_or_else(|| AppError::Msg("帧尺寸不匹配".into()))?;
        img.save_with_format(out, image::ImageFormat::Png)?;
        Ok((w, h))
    }
}
