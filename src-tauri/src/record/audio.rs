//! 声音采集（WASAPI）：系统声音（loopback）+ 麦克风，以及两路混音。
//!
//! 为什么系统声用 loopback 而不是虚拟声卡：
//!   录屏八成是在讲一个东西 —— 视频里的声音、游戏音效、会议里的人声，
//!   这些都在**默认播放设备**的输出里。WASAPI 的 loopback 模式能把
//!   "正在播什么"原样抓下来，不需要装 VB-Cable 之类驱动。
//!
//! 几个必须记住的点：
//!   1. WASAPI 要用 COM。录制线程不是主线程，得自己 `CoInitializeEx`；
//!      返回 `RPC_E_CHANGED_MODE` 说明这个线程已经是别的套间模型，不是错误。
//!   2. **混音格式由设备决定**，我们不能指定。常见是 48kHz / 2ch / float32，
//!      但也见过 44.1kHz / 16bit。所以这里读出真实格式再转成 MF 要的
//!      「16bit 交错立体声」。
//!   3. `GetMixFormat` 给的是 CoTaskMem 分配的内存，必须 `CoTaskMemFree`，
//!      否则每开一次录制泄漏一点。
//!   4. loopback 在**设备没有声音输出时不会出包**（GetNextPacketSize 返回 0），
//!      这是正常的，不是采集失败 —— 别在这里报错。
//!   5. 麦克风和播放设备的采样率**可能不同**（44.1k vs 48k），
//!      混音前必须重采样到同一路，否则会有周期性的咔哒声。

use crate::error::{AppError, AppResult};
use std::ptr;

use windows::Win32::Media::Audio::{
    eCapture, eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
    MMDeviceEnumerator, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK,
    WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{
    CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
};

/// WASAPI 的静音标记：这一包数据是静音，指针内容不可信
const BUFFERFLAGS_SILENT: u32 = 0x2;
/// WAVE_FORMAT_IEEE_FLOAT
const WAVE_FORMAT_IEEE_FLOAT: u32 = 3;
/// WAVE_FORMAT_EXTENSIBLE
const WAVE_FORMAT_EXTENSIBLE: u32 = 0xFFFE;

/// 输出永远是 16bit 交错立体声 —— MF 的 AAC 编码器吃这个最省事
pub const OUT_CHANNELS: u16 = 2;

/// 采哪一路
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// 默认播放设备的 loopback（系统声音）
    Loopback,
    /// 默认录音设备（麦克风）
    Mic,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Loopback => "系统声音",
            Kind::Mic => "麦克风",
        }
    }
}

/// 采集到的源格式（决定怎么转成 s16）
#[derive(Debug, Clone, Copy)]
struct SrcFormat {
    channels: u16,
    /// 源样本是 32 位浮点
    is_float: bool,
}

/// 一路 WASAPI 采集
pub struct Capture {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    /// 设备混音采样率（一般是 48000）
    pub sample_rate: u32,
    src: SrcFormat,
    started: bool,
    kind: Kind,
}

impl Capture {
    /// 打开并启动一路采集。
    ///
    /// 失败时调用方应该**降级继续录画面**，不要因为没声音就整场录制失败。
    pub fn open(kind: Kind) -> AppResult<Self> {
        unsafe {
            // 录制线程自己的 COM 套间。RPC_E_CHANGED_MODE 表示已经初始化过，忽略。
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);

            let enumerator: IMMDeviceEnumerator =
                CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                    .map_err(|e| AppError::Msg(format!("获取音频设备枚举器失败: {e}")))?;

            let (flow, role) = match kind {
                Kind::Loopback => (eRender, eConsole),
                Kind::Mic => (eCapture, eConsole),
            };

            let device = enumerator
                .GetDefaultAudioEndpoint(flow, role)
                .map_err(|e| AppError::Msg(format!("没有可用的{}设备: {e}", kind.label())))?;

            let client: IAudioClient = device
                .Activate(CLSCTX_ALL, None)
                .map_err(|e| AppError::Msg(format!("打开{}客户端失败: {e}", kind.label())))?;

            let fmt_ptr = client
                .GetMixFormat()
                .map_err(|e| AppError::Msg(format!("读取{}格式失败: {e}", kind.label())))?;
            if fmt_ptr.is_null() {
                return Err(AppError::Msg("混音格式为空".into()));
            }

            // ---- 读格式 ----
            let fmt = &*fmt_ptr;
            let sample_rate = fmt.nSamplesPerSec;
            let channels = fmt.nChannels.max(1);
            let tag = fmt.wFormatTag as u32;
            let mut is_float = tag == WAVE_FORMAT_IEEE_FLOAT;

            if tag == WAVE_FORMAT_EXTENSIBLE && fmt.cbSize >= 22 {
                // EXTENSIBLE 的真正格式在 SubFormat 里。
                // 前 2 字节就是 WAVE_FORMAT_* 那个编号。
                let ext = &*(fmt_ptr as *const WAVEFORMATEXTENSIBLE);
                let tag = (ext.SubFormat.data1 & 0xFFFF) as u32;
                is_float = tag == WAVE_FORMAT_IEEE_FLOAT;
            }

            // 200ms 缓冲：够稳，又不至于让停止录制时等太久
            const BUFFER_HNS: i64 = 2_000_000;
            let flags = match kind {
                Kind::Loopback => AUDCLNT_STREAMFLAGS_LOOPBACK,
                // 麦克风绝不能带 LOOPBACK：带了会初始化失败
                Kind::Mic => 0,
            };
            let init = client.Initialize(
                AUDCLNT_SHAREMODE_SHARED,
                flags,
                BUFFER_HNS,
                0,
                fmt_ptr,
                None,
            );
            // 格式指针用完就要还回去（CoTaskMem 分配的）
            CoTaskMemFree(Some(fmt_ptr as *const core::ffi::c_void));

            init.map_err(|e| AppError::Msg(format!("初始化{}采集失败: {e}", kind.label())))?;

            let capture: IAudioCaptureClient = client
                .GetService()
                .map_err(|e| AppError::Msg(format!("获取{}采集接口失败: {e}", kind.label())))?;

            client
                .Start()
                .map_err(|e| AppError::Msg(format!("启动{}采集失败: {e}", kind.label())))?;

            log::info!(
                "{}采集中：{sample_rate}Hz {channels}ch {}",
                kind.label(),
                if is_float { "float32" } else { "int16" }
            );

            Ok(Self {
                client,
                capture,
                sample_rate,
                src: SrcFormat {
                    channels,
                    is_float,
                },
                started: true,
                kind,
            })
        }
    }

    /// 把当前可读的包全部追加到 `out`（16bit 交错立体声）。
    ///
    /// 没有新数据就什么都不做 —— loopback 在静音时确实不出包，不是错误。
    pub fn read(&mut self, out: &mut Vec<i16>) -> AppResult<()> {
        unsafe {
            loop {
                let frames = self
                    .capture
                    .GetNextPacketSize()
                    .map_err(|e| AppError::Msg(format!("读取音频包大小失败: {e}")))?;
                if frames == 0 {
                    break;
                }

                let mut data: *mut u8 = ptr::null_mut();
                let mut count: u32 = 0;
                let mut flags: u32 = 0;
                self.capture
                    .GetBuffer(&mut data, &mut count, &mut flags, None, None)
                    .map_err(|e| AppError::Msg(format!("读取音频数据失败: {e}")))?;

                if count > 0 {
                    if flags & BUFFERFLAGS_SILENT != 0 || data.is_null() {
                        // 静音包：指针内容未定义，只能自己补零
                        out.extend(std::iter::repeat_n(
                            0i16,
                            count as usize * OUT_CHANNELS as usize,
                        ));
                    } else {
                        self.convert(data, count as usize, out);
                    }
                }

                self.capture
                    .ReleaseBuffer(count)
                    .map_err(|e| AppError::Msg(format!("归还音频缓冲失败: {e}")))?;

                if count == 0 {
                    break;
                }
            }
        }
        Ok(())
    }

    /// 源格式 → 16bit 交错立体声
    fn convert(&self, data: *const u8, frames: usize, out: &mut Vec<i16>) {
        let ch = self.src.channels as usize;
        out.reserve(frames * OUT_CHANNELS as usize);

        for f in 0..frames {
            // 只取前两个声道：多声道（5.1/7.1）下前左/前右就是"人耳默认的立体声"
            let pick = |c: usize| -> i16 {
                let idx = c.min(ch - 1);
                unsafe {
                    if self.src.is_float {
                        let p = data.add((f * ch + idx) * 4) as *const f32;
                        let v = *p;
                        // 浮点越界要夹住，否则转 s16 会绕回成刺耳的爆音
                        (v.clamp(-1.0, 1.0) * 32767.0) as i16
                    } else {
                        let p = data.add((f * ch + idx) * 2) as *const i16;
                        *p
                    }
                }
            };
            out.push(pick(0));
            out.push(if ch >= 2 { pick(1) } else { pick(0) });
        }
    }

    /// 把积压的数据丢掉（暂停 / 倒计时期间用）。
    ///
    /// 不丢的话，暂停 10 秒再恢复会突然喷出 10 秒前的旧声音，
    /// 时间轴也会直接错位。
    pub fn drain(&mut self) {
        let mut scratch: Vec<i16> = Vec::new();
        let _ = self.read(&mut scratch);
    }
}

impl Drop for Capture {
    fn drop(&mut self) {
        if self.started {
            unsafe {
                let _ = self.client.Stop();
            }
            self.started = false;
            log::debug!("{}采集已停止", self.kind.label());
        }
    }
}

/* ---------------- 重采样与混音 ---------------- */

/// 线性插值重采样（立体声交错 i16）。
///
/// 为什么不用"每包独立插值"：那样每个包边界都会重新对齐，
/// 结果是每 20ms 一次的咔哒声。这里保留**上一包的最后一帧**和
/// **小数相位**，让插值跨包连续。
pub struct Resampler {
    from: u32,
    to: u32,
    /// 下一个输出样本在输入里的位置（相对当前包的起点，可为负）
    phase: f64,
    /// 上一包的最后一帧（L, R）
    tail: [i16; 2],
    primed: bool,
}

impl Resampler {
    pub fn new(from: u32, to: u32) -> Self {
        Self {
            from: from.max(1),
            to: to.max(1),
            phase: 0.0,
            tail: [0, 0],
            primed: false,
        }
    }

    /// 需要重采样吗（同采样率时走直通，省掉插值）
    pub fn needed(&self) -> bool {
        self.from != self.to
    }

    /// 把 `input` 重采样后追加到 `out`
    pub fn push(&mut self, input: &[i16], out: &mut Vec<i16>) {
        if !self.needed() {
            out.extend_from_slice(input);
            return;
        }
        let frames = input.len() / 2;
        if frames == 0 {
            return;
        }
        if !self.primed {
            self.tail = [input[0], input[1]];
            self.primed = true;
        }

        let step = self.from as f64 / self.to as f64;
        let mut pos = self.phase;
        out.reserve(((frames as f64 / step) as usize + 2) * 2);

        while pos < (frames - 1) as f64 {
            let i = pos.floor();
            let frac = pos - i;
            let i = i as isize;

            let a = if i < 0 {
                self.tail
            } else {
                let o = i as usize * 2;
                [input[o], input[o + 1]]
            };
            let j = i + 1;
            let b = if j < 0 {
                self.tail
            } else if j as usize >= frames {
                a
            } else {
                let o = j as usize * 2;
                [input[o], input[o + 1]]
            };

            for c in 0..2 {
                let v = a[c] as f64 * (1.0 - frac) + b[c] as f64 * frac;
                out.push(v.round().clamp(-32768.0, 32767.0) as i16);
            }
            pos += step;
        }

        // 剩下的相位相对下一包起点；裁到 [-1, 0] 防止长时间静音后漂出去
        self.phase = (pos - frames as f64).clamp(-1.0, 0.0);
        self.tail = [input[(frames - 1) * 2], input[(frames - 1) * 2 + 1]];
    }
}

/// 软限幅的拐点（约 -3dB）
const KNEE: f32 = 22937.0;

/// 两路相加并软限幅。
///
/// 直接 `clamp` 会削波，听起来是"破音"而不是"响"。
/// 这里用三次软膝：拐点以下完全线性（单路录音不会被改音色），
/// 拐点以上平滑压到满幅。
fn soft_add(a: i16, b: i16) -> i16 {
    let x = a as i32 + b as i32;
    let ax = x.abs() as f32;
    if ax <= KNEE {
        return x.clamp(-32768, 32767) as i16;
    }
    let range = 32767.0 - KNEE;
    let t = ((ax - KNEE) / range).min(1.0);
    // cubic soft knee：t=1 时正好映射到满幅
    let y = KNEE + range * ((t - t * t * t / 3.0) / (2.0 / 3.0)).min(1.0);
    (y * x.signum() as f32) as i16
}

/// 把 `b` 混进 `a`（长度取两者较大者，缺的部分当静音）
pub fn mix_into(a: &mut Vec<i16>, b: &[i16]) {
    if a.len() < b.len() {
        a.resize(b.len(), 0);
    }
    for (i, v) in b.iter().enumerate() {
        a[i] = soft_add(a[i], *v);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resampler_keeps_length_ratio() {
        let mut r = Resampler::new(44100, 48000);
        let input: Vec<i16> = (0..4410).flat_map(|i| [i as i16, i as i16]).collect();
        let mut out = Vec::new();
        // 分两包喂，验证跨包连续
        r.push(&input[..4410], &mut out);
        r.push(&input[4410..], &mut out);
        let frames = out.len() / 2;
        // 4410 帧 @44.1k ≈ 4800 帧 @48k，允许 1% 误差
        assert!(
            (frames as i64 - 4800).abs() < 48,
            "重采样后帧数 {frames} 偏离预期 4800"
        );
    }

    #[test]
    fn same_rate_is_passthrough() {
        let mut r = Resampler::new(48000, 48000);
        let input = vec![1i16, 2, 3, 4];
        let mut out = Vec::new();
        r.push(&input, &mut out);
        assert_eq!(out, input);
    }

    #[test]
    fn soft_clip_is_transparent_below_knee() {
        // 拐点以下必须逐样本相等，否则单路录音的音色会被悄悄改掉
        assert_eq!(soft_add(1000, -300), 700);
        assert_eq!(soft_add(11000, 11000), 22000);
    }

    #[test]
    fn soft_clip_never_overflows() {
        for (a, b) in [(32767, 32767), (-32768, -32768), (32000, -32000)] {
            let v = soft_add(a, b) as i32;
            assert!((-32768..=32767).contains(&v), "soft_add({a},{b}) = {v} 越界");
        }
        // 两路都满幅时要压到接近满幅，而不是变成一半
        assert!(soft_add(32767, 32767) > 30000);
    }

    #[test]
    fn mix_pads_shorter_track() {
        let mut a = vec![100i16, 100];
        mix_into(&mut a, &[200, 200, 300, 300]);
        assert_eq!(a.len(), 4);
        assert_eq!(a[0], 300);
        assert_eq!(a[2], 300);
    }
}
