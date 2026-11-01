//! 本地 OCR —— 直接用 Windows 自带的 Windows.Media.Ocr。
//!
//! 不联网、不装模型、不带 tesseract：系统装了什么语言包就能识别什么。
//!
//! WinRT 的异步操作在 Rust 里不能直接 await（会牵出整个 async 运行时），
//! 这里用标准的"轮询 Status + GetResults"模式同步等待 —— OCR 是用户主动触发的
//! 一次性操作，卡几百毫秒不影响任何东西。

use crate::error::{AppError, AppResult};
use crate::types::{OcrLine, OcrResult, OcrWord};

#[cfg(windows)]
pub fn available_languages() -> Vec<String> {
    use windows::Media::Ocr::OcrEngine;

    let mut out = Vec::new();
    // WinRT 的这些调用在 windows crate 里是安全函数，不需要 unsafe
    if let Ok(langs) = OcrEngine::AvailableRecognizerLanguages() {
        for lang in langs {
            if let Ok(tag) = lang.LanguageTag() {
                out.push(tag.to_string());
            }
        }
    }
    out
}

#[cfg(not(windows))]
pub fn available_languages() -> Vec<String> {
    Vec::new()
}

/// 同步等待一个 WinRT 异步操作完成。
/// windows-future 的 Async trait 是私有的，所以这里直接轮询 Status + GetResults。
/// 8ms 的间隔对 OCR 这种一次性操作完全无感。
#[cfg(windows)]
macro_rules! wait {
    ($op:expr) => {{
        let op = ($op).map_err(|e: windows::core::Error| AppError::Win32(e.to_string()))?;
        loop {
            match op.Status() {
                Ok(windows_future::AsyncStatus::Started) => {
                    std::thread::sleep(std::time::Duration::from_millis(8));
                }
                Ok(windows_future::AsyncStatus::Completed) => {
                    break op
                        .GetResults()
                        .map_err(|e: windows::core::Error| AppError::Win32(e.to_string()))?
                }
                Ok(_) => return Err(AppError::Win32("WinRT 异步操作被取消".into())),
                Err(e) => return Err(AppError::Win32(e.to_string())),
            }
        }
    }};
}

#[cfg(windows)]
pub fn recognize(png_bytes: &[u8]) -> AppResult<OcrResult> {
    use windows::core::HSTRING;
    use windows::Globalization::Language;
    use windows::Graphics::Imaging::{
        BitmapAlphaMode, BitmapDecoder, BitmapPixelFormat, ColorManagementMode, ExifOrientationMode,
    };
    use windows::Media::Ocr::OcrEngine;
    use windows::Storage::Streams::{DataWriter, InMemoryRandomAccessStream};

    let started = std::time::Instant::now();

    // WinRT 调用本身都是安全函数（windows crate 已封装），这里用普通块分组，
    // 不再套 `unsafe` —— 保留 unsafe 会让"这块有危险操作"变成误报。
    {
        let stream =
            InMemoryRandomAccessStream::new().map_err(|e| AppError::Win32(e.to_string()))?;
        let writer =
            DataWriter::CreateDataWriter(&stream).map_err(|e| AppError::Win32(e.to_string()))?;
        writer
            .WriteBytes(png_bytes)
            .map_err(|e| AppError::Win32(e.to_string()))?;
        wait!(writer.StoreAsync());
        wait!(writer.FlushAsync());
        stream.Seek(0).map_err(|e| AppError::Win32(e.to_string()))?;

        let decoder = wait!(BitmapDecoder::CreateAsync(&stream));

        let max_dim = OcrEngine::MaxImageDimension().unwrap_or(10000) as f64;
        let px_w = decoder.PixelWidth().unwrap_or(0) as f64;
        let px_h = decoder.PixelHeight().unwrap_or(0) as f64;

        // 超过引擎尺寸上限必须缩放，否则 RecognizeAsync 直接报错
        let bitmap = if px_w > max_dim || px_h > max_dim {
            let ratio = max_dim / px_w.max(px_h);
            let tw = (px_w * ratio).floor().max(1.0) as u32;
            let th = (px_h * ratio).floor().max(1.0) as u32;
            let transform = windows::Graphics::Imaging::BitmapTransform::new()
                .map_err(|e| AppError::Win32(e.to_string()))?;
            transform
                .SetScaledWidth(tw)
                .map_err(|e| AppError::Win32(e.to_string()))?;
            transform
                .SetScaledHeight(th)
                .map_err(|e| AppError::Win32(e.to_string()))?;
            wait!(decoder.GetSoftwareBitmapTransformedAsync(
                BitmapPixelFormat::Bgra8,
                BitmapAlphaMode::Premultiplied,
                &transform,
                ExifOrientationMode::IgnoreExifOrientation,
                ColorManagementMode::DoNotColorManage
            ))
        } else {
            // 无参版本返回解码器原生格式；OcrEngine 接受 Bgra8/Gray8，
            // 绝大多数 PNG 解码出来就是 Bgra8，直接可用
            wait!(decoder.GetSoftwareBitmapAsync())
        };

        let engine = match OcrEngine::TryCreateFromUserProfileLanguages() {
            Ok(e) => e,
            Err(_) => {
                let lang = Language::CreateLanguage(&HSTRING::from("zh-Hans-CN"))
                    .map_err(|e| AppError::Win32(e.to_string()))?;
                OcrEngine::TryCreateFromLanguage(&lang).map_err(|_| {
                    AppError::Msg(
                        "系统未安装 OCR 语言包。请在「设置 → 时间和语言 → 语言和区域」添加语言并勾选「光学字符识别」".into(),
                    )
                })?
            }
        };

        let result = wait!(engine.RecognizeAsync(&bitmap));

        let language = engine
            .RecognizerLanguage()
            .and_then(|l| l.LanguageTag())
            .map(|t| t.to_string())
            .unwrap_or_default();

        let mut lines = Vec::new();
        if let Ok(ocr_lines) = result.Lines() {
            for line in ocr_lines {
                let text = line.Text().map(|t| t.to_string()).unwrap_or_default();
                let mut words = Vec::new();
                if let Ok(ocr_words) = line.Words() {
                    for w in ocr_words {
                        let Ok(rect) = w.BoundingRect() else { continue };
                        words.push(OcrWord {
                            text: w.Text().map(|t| t.to_string()).unwrap_or_default(),
                            x: rect.X,
                            y: rect.Y,
                            width: rect.Width,
                            height: rect.Height,
                        });
                    }
                }
                lines.push(OcrLine { text, words });
            }
        }

        let text = lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        Ok(OcrResult {
            text,
            lines,
            language,
            duration_ms: started.elapsed().as_millis() as u64,
        })
    }
}

#[cfg(not(windows))]
pub fn recognize(_png_bytes: &[u8]) -> AppResult<OcrResult> {
    Err(AppError::Msg("OCR 目前仅支持 Windows".into()))
}
