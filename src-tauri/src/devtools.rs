//! 开发者自检工具：`snipforge.exe --smoke` / `--record-smoke` 这些入口的实现。
//!
//! 单独一个模块的理由：它们是**只在命令行用**的诊断代码，和应用的启动、
//! 命令分发没有任何关系。混在 `lib.rs` 里会让启动流程被三百行 printf 埋掉。

use crate::capture;
use crate::clipboard;
use crate::ocr;
use crate::record;
use crate::types;
use crate::win32;

/// 冒烟测试：不开窗口，把核心链路跑一遍，输出到 stdout。
///
/// 用途：CI / 打包后自检 / 用户报"截不了图"时让他跑一下看哪一环断了。
/// `snipforge.exe --smoke`
pub fn smoke_test() -> i32 {
    let mut failures = 0;
    println!("== SnipForge 冒烟测试 ==");

    match capture::screen::capture_frame() {
        Ok(frame) => {
            println!("[ok] 屏幕捕获：{} 块显示器", frame.displays.len());
            for d in &frame.displays {
                println!(
                    "     - #{} {} {}x{} @({},{}) scale={:.2}{}",
                    d.id,
                    d.name,
                    d.physical.width,
                    d.physical.height,
                    d.physical.x,
                    d.physical.y,
                    d.scale_factor,
                    if d.is_primary { " [主屏]" } else { "" }
                );
            }
            match frame.payload().displays.first() {
                Some(p) if p.preview.starts_with("data:image/jpeg") => {
                    println!("[ok] 预览编码：{} KB", p.preview.len() / 1024);
                }
                _ => {
                    println!("[fail] 预览编码异常");
                    failures += 1;
                }
            }
        }
        Err(e) => {
            println!("[fail] 屏幕捕获：{e}");
            failures += 1;
        }
    }

    let wins = win32::window::list_top_level_windows(std::process::id());
    if wins.is_empty() {
        println!("[warn] 窗口枚举：0 个（正常情况下至少应有几个）");
    } else {
        println!("[ok] 窗口枚举：{} 个", wins.len());
        for w in wins.iter().take(3) {
            println!("     - {} [{}]", w.title.chars().take(30).collect::<String>(), w.exe);
        }
    }

    let langs = ocr::available_languages();
    if langs.is_empty() {
        println!("[warn] OCR(Windows)：未安装语言包");
    } else {
        println!("[ok] OCR(Windows)：{}", langs.join(", "));
    }
    match ocr::paddle::find_exe("") {
        Some(p) => println!("[ok] OCR(PaddleOCR)：{}", p.display()),
        None => println!("[info] OCR(PaddleOCR)：未安装（可选，放引擎目录即可）"),
    }

    // 剪贴板端到端：写 → 序列号变化 → 读回 → 还原
    {
        let original = arboard::Clipboard::new().ok().and_then(|mut c| c.get_text().ok());
        let seq_before = clipboard::sequence_number();
        let marker = format!("snipforge-smoke-{}", chrono::Utc::now().timestamp_millis());

        let wrote = arboard::Clipboard::new()
            .and_then(|mut c| c.set_text(marker.clone()))
            .is_ok();

        if !wrote {
            println!("[fail] 剪贴板：写入失败");
            failures += 1;
        } else {
            std::thread::sleep(std::time::Duration::from_millis(150));
            let seq_after = clipboard::sequence_number();
            let readback = arboard::Clipboard::new()
                .ok()
                .and_then(|mut c| c.get_text().ok())
                .unwrap_or_default();

            if seq_after != seq_before {
                println!("[ok] 剪贴板序列号变化：{seq_before} → {seq_after}");
            } else {
                println!("[fail] 剪贴板序列号未变化，监听器将检测不到任何变化");
                failures += 1;
            }
            if readback == marker {
                println!("[ok] 剪贴板写读一致");
            } else {
                println!("[fail] 剪贴板写读不一致");
                failures += 1;
            }
        }

        // 还原用户原来的内容，别把测试残留留在剪贴板里
        if let Some(orig) = original {
            let _ = arboard::Clipboard::new().and_then(|mut c| c.set_text(orig));
        }
    }

    // 图片剪贴板：截图后点"复制"走的就是这条路径，必须单独验
    {
        let probe = image::RgbaImage::from_pixel(64, 32, image::Rgba([200, 100, 50, 255]));
        let written = arboard::Clipboard::new()
            .and_then(|mut c| {
                c.set_image(arboard::ImageData {
                    width: 64,
                    height: 32,
                    bytes: std::borrow::Cow::Owned(probe.into_raw()),
                })
            })
            .is_ok();

        if !written {
            println!("[fail] 图片剪贴板：写入失败");
            failures += 1;
        } else {
            std::thread::sleep(std::time::Duration::from_millis(150));
            match arboard::Clipboard::new().and_then(|mut c| c.get_image()) {
                Ok(img) if img.width == 64 && img.height == 32 => {
                    println!("[ok] 图片剪贴板：写入并读回 64×32");
                }
                Ok(img) => {
                    println!("[fail] 图片剪贴板：尺寸不符 {}×{}", img.width, img.height);
                    failures += 1;
                }
                Err(e) => {
                    println!("[fail] 图片剪贴板读取失败：{e}");
                    failures += 1;
                }
            }
        }
    }

    println!("== 完成，{} 项失败 ==", failures);
    failures
}

/// 录屏编码自检：写一段 2 秒 30fps 的合成动画到临时文件，再读回时长。
/// `snipforge.exe --record-smoke`
pub fn record_smoke() -> i32 {
    let path = std::env::temp_dir().join("snipforge-record-smoke.mp4");
    let _ = std::fs::remove_file(&path);
    print!("== 录屏编码自检 ==");
    match record::encoder_smoke(&path, 30, 2) {
        Ok((ms, frames)) => {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            println!("
[ok] 写出 {} 帧，时长 {}ms，文件 {} KB", frames, ms, size / 1024);
            println!("[ok] 文件：{}", path.display());
            0
        }
        Err(e) => {
            println!("
[fail] {e}");
            1
        }
    }
}

/// GIF 编码自检：合成动画 → 写 GIF → 用 gif 自己的解码器读回来核对。
/// `snipforge.exe --gif-smoke`
pub fn gif_smoke() -> i32 {
    let path = std::env::temp_dir().join("snipforge-gif-smoke.gif");
    println!("== GIF 编码自检 ==");
    match record::gif::gif_smoke(&path, 12, 2) {
        Ok((written, read, total_cs)) => {
            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            println!(
                "[ok] 写入 {written} 帧，读回 {read} 帧，总时长 {}ms，文件 {} KB",
                total_cs * 10,
                size / 1024
            );
            println!("[ok] 文件：{}", path.display());
            0
        }
        Err(e) => {
            println!("[fail] {e}");
            1
        }
    }
}

/// 真实 GIF 录屏自检：录 N 秒主屏，直出 GIF（不写 MP4）。
/// `snipforge.exe --gif-test 5 [宽度] [帧率]`
pub fn gif_screen_smoke(seconds: u64, width: u32, fps: u32) -> i32 {
    let mon = match xcap::Monitor::all().ok().and_then(|m| m.into_iter().next()) {
        Some(m) => m,
        None => {
            println!("[fail] 没有可用显示器");
            return 1;
        }
    };
    let rect = types::Rect {
        x: mon.x().unwrap_or(0),
        y: mon.y().unwrap_or(0),
        width: mon.width().unwrap_or(1920),
        height: mon.height().unwrap_or(1080),
    };
    drop(mon);

    let path = std::env::temp_dir().join("snipforge-gif-test.gif");
    let _ = std::fs::remove_file(&path);

    let plan = record::gif::GifPlan {
        path: path.clone(),
        fps,
        max_width: width,
    };
    let opts = record::RecordOptions {
        rect,
        fps: 30,
        bitrate: 4_000_000,
        draw_cursor: true,
        write_mp4: false,
    };
    println!("== 真实 GIF 录屏自检 {}s（宽 {width}，{fps}fps） ==", seconds);
    let rec = match record::start(None, opts, path.clone(), Some(plan)) {
        Ok(r) => r,
        Err(e) => {
            println!("[fail] 启动失败: {e}");
            return 1;
        }
    };
    rec.begin();
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    match rec.stop() {
        Ok(r) => {
            println!(
                "[ok] 采集 {} 帧 / {}ms → GIF {} 帧 {}×{} {} KB",
                r.frames,
                r.duration_ms,
                r.gif_frames,
                r.gif_width,
                r.gif_height,
                r.gif_bytes / 1024
            );
            println!("[ok] 文件：{}", r.path);
            0
        }
        Err(e) => {
            println!("[fail] {e}");
            1
        }
    }
}

/// 把 GIF 的某一帧导成 PNG，肉眼确认颜色和方向。
/// `snipforge.exe --gif-inspect <gif> [out.png] [index]`
pub fn gif_inspect(src: &str, out: Option<&str>, index: u32) -> i32 {
    let src = std::path::PathBuf::from(src);
    let out = out
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("snipforge-gif-frame.png"));
    match record::gif::extract_frame(&src, &out, index) {
        Ok((w, h)) => {
            println!("[ok] 画布 {w}×{h}，第 {index} 帧已存 {}", out.display());
            0
        }
        Err(e) => {
            println!("[fail] {e}");
            1
        }
    }
}

/// 真实录屏自检：录 N 秒主屏，报告帧数/时长/体积。
/// `snipforge.exe --record-test 3`
pub fn record_screen_smoke(seconds: u64) -> i32 {
    let mon = match xcap::Monitor::all().ok().and_then(|m| m.into_iter().next()) {
        Some(m) => m,
        None => {
            println!("[fail] 没有可用显示器");
            return 1;
        }
    };
    let rect = types::Rect {
        x: mon.x().unwrap_or(0),
        y: mon.y().unwrap_or(0),
        width: mon.width().unwrap_or(1920),
        height: mon.height().unwrap_or(1080),
    };
    drop(mon);

    let path = std::env::temp_dir().join("snipforge-record-test.mp4");
    let _ = std::fs::remove_file(&path);

    let opts = record::RecordOptions {
        rect,
        fps: 30,
        bitrate: 8_000_000,
        draw_cursor: true,
        write_mp4: true,
    };
    println!("== 真实录屏自检 {}s ==", seconds);
    let rec = match record::start(None, opts, path.clone(), None) {
        Ok(r) => r,
        Err(e) => {
            println!("[fail] 启动失败: {e}");
            return 1;
        }
    };
    rec.begin();
    std::thread::sleep(std::time::Duration::from_secs(seconds));
    match rec.stop() {
        Ok(r) => {
            let real_fps = r.frames as f64 * 1000.0 / r.duration_ms.max(1) as f64;
            println!(
                "[ok] {} 帧，{}ms，{} KB，{}×{}，平均 {:.1} fps",
                r.frames,
                r.duration_ms,
                r.bytes / 1024,
                r.width,
                r.height,
                real_fps
            );
            match record::mf::probe_duration_ms(&path) {
                Ok(ms) => println!("[ok] 容器时长 {ms}ms"),
                Err(e) => {
                    println!("[fail] 读回失败: {e}");
                    return 1;
                }
            }
            0
        }
        Err(e) => {
            println!("[fail] {e}");
            1
        }
    }
}

/// 抽录制文件的第一帧存 PNG，确认画面方向与颜色。
/// `snipforge.exe --record-inspect <mp4> [out.png]`
pub fn record_inspect(
    src: &str,
    out: Option<&str>,
    crop: Option<(u32, u32, u32, u32)>,
    skip: u32,
) -> i32 {
    let src = std::path::PathBuf::from(src);
    let out = out
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("snipforge-frame0.png"));
    match record::mf::extract_frame(&src, &out, skip) {
        Ok((w, h)) => {
            if let Some((cx, cy, cw, ch)) = crop {
                let cw = cw.min(w.saturating_sub(cx));
                let ch = ch.min(h.saturating_sub(cy));
                match image::open(&out) {
                    Ok(img) => {
                        let sub = img.crop_imm(cx, cy, cw, ch);
                        let _ = sub.save_with_format(&out, image::ImageFormat::Png);
                        println!("[ok] 解出 {w}×{h}，裁 {cw}×{ch} 后存 {}", out.display());
                        return 0;
                    }
                    Err(e) => {
                        println!("[fail] 裁剪失败: {e}");
                        return 1;
                    }
                }
            }
            println!("[ok] 解出 {w}×{h}，已存 {}", out.display());
            0
        }
        Err(e) => {
            println!("[fail] {e}");
            1
        }
    }
}

/// 光标自检：导出当前光标位图
pub fn cursor_test(out: &str) -> i32 {
    let path = std::path::PathBuf::from(out);
    match record::cursor_smoke(&path) {
        Ok((w, h)) => {
            println!("[ok] 光标 {w}×{h}，已存 {}", path.display());
            0
        }
        Err(e) => {
            println!("[fail] {e}");
            1
        }
    }
}
