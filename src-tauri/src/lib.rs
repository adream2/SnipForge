//! SnipForge —— Windows 端截图 / 录屏 / 剪贴板 / 图床 / 图片处理 一体化工具。
//!
//! 分层约定（改代码前先读）：
//!   - `commands/` 是薄壳：参数校验 → 调 service → 返回，不写业务逻辑
//!   - `win32/` 是唯一 import `windows` crate 的地方，上层只碰干净的 Rust 类型
//!   - `capture/` `clipboard/` `uploader/` 互不直接调用，跨模块通信走 `hooks`
//!   - 第三方扩展只通过 `uploader::Provider` trait 或声明式 JSON 接入

mod capture;
mod clipboard;
mod commands;
mod config;
mod error;
mod hooks;
mod logger;
mod ocr;
mod panel;
mod paths;
mod plugins;
mod record;
mod shortcuts;
mod state;
mod tray;
mod types;
mod uploader;
mod win32;
mod windows;

use std::sync::Arc;
use tauri::Manager;

use config::AppSettings;
use paths::Paths;
use state::AppState;

fn load_settings(paths: &Paths) -> AppSettings {
    let file = paths.settings_file();
    if !file.exists() {
        let fresh = AppSettings::default();
        if let Ok(text) = serde_json::to_string_pretty(&fresh) {
            let _ = std::fs::write(&file, text);
        }
        return fresh;
    }
    match std::fs::read_to_string(&file) {
        Ok(text) => match serde_json::from_str::<AppSettings>(&text) {
            Ok(cfg) => cfg,
            Err(e) => {
                log::error!("配置解析失败，回退默认值: {e}");
                let backup = file.with_extension(format!(
                    "json.corrupt.{}",
                    chrono::Utc::now().timestamp()
                ));
                let _ = std::fs::copy(&file, backup);
                AppSettings::default()
            }
        },
        Err(e) => {
            log::error!("配置读取失败: {e}");
            AppSettings::default()
        }
    }
}

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

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let handle = app.handle().clone();

            let data_dir = handle
                .path()
                .app_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("."));
            let resource_dir = handle.path().resource_dir().ok();
            let pictures = handle
                .path()
                .picture_dir()
                .unwrap_or_else(|_| data_dir.clone())
                .join("SnipForge");

            let paths = Paths::new(data_dir, resource_dir, pictures);
            if let Err(e) = paths.ensure() {
                log::error!("目录初始化失败: {e}");
            }
            // 日志必须最先就位，后面任何一步炸了都要有记录
            logger::init(&paths.logs);
            log::info!("数据目录: {}", paths.data.display());

            let mut settings = load_settings(&paths);
            let needs_migrate = settings.version < 2;
            config::migrate(&mut settings);

            // 剪贴板历史载入
            let clipboard_service = Arc::new(clipboard::ClipboardService::new(&paths));
            clipboard_service.store.load(settings.clipboard.max_items);
            app.manage(clipboard_service);

            // 图床注册表
            app.manage(Arc::new(uploader::build_registry()));

            // 主状态
            let state = AppState::new(settings, paths);
            app.manage(state.clone());

            // 迁移完必须落盘，否则下次启动又按老配置走一遍
            if needs_migrate {
                if let Err(e) = state.persist_settings() {
                    log::error!("配置迁移写盘失败: {e}");
                }
            }

            win32::window::set_dpi_aware();

            if let Err(e) = plugins::init_runtime(&handle, &state) {
                log::error!("运行时初始化失败: {e}");
            }

            // Win+V 只能靠键盘钩子拿，注册表那条路走不通（实测）
            win32::keyhook::install(app.handle().clone());
            win32::keyhook::set_active(state.settings.read().clipboard.takeover_win_v);

            // 预建剪贴板面板（隐藏）：否则第一次按 Win+V 要等 webview 起来，
            // 那几百毫秒里面板是空白的，用户敲回车毫无反应。
            //
            // 必须放到后台线程：建窗口内部要把请求投递给主线程再等结果，
            // 在 setup 里（就在主线程上）直接调会自锁。延迟 400ms 是让启动
            // 优先把主窗口画出来，用户感知不到。
            {
                let h = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_millis(400));
                    panel::prewarm(&h);
                });
            }

            // 首次启动展示主窗口，之后只在托盘里待命
            if let Some(win) = app.get_webview_window("main") {
                let _ = win.show();
                let _ = win.set_focus();
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "main" {
                    let app = window.app_handle();
                    let close_to_tray = app
                        .try_state::<Arc<AppState>>()
                        .map(|s| s.settings.read().general.close_to_tray)
                        .unwrap_or(true);
                    if close_to_tray {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            // 设置
            commands::settings_get,
            commands::settings_patch,
            commands::settings_reset,
            commands::settings_pick_dir,
            commands::app_info,
            commands::app_open_dir,
            commands::app_log,
            commands::app_open_path,
            // 截图
            commands::capture::capture_start,
            commands::capture::capture_frame,
            commands::capture::capture_windows,
            commands::capture::capture_commit,
            commands::capture::capture_cancel,
            commands::capture::capture_pending,
            commands::capture::capture_scroll,
            commands::capture::capture_abort_scroll,
            commands::capture::capture_close_editor,
            // 图片
            commands::image::image_save,
            commands::image::image_save_as,
            // 录屏
            commands::record::record_start,
            commands::record::record_start_region,
            commands::record::record_pick_region,
            commands::record::record_start_fullscreen,
            commands::record::record_begin,
            commands::record::record_pause,
            commands::record::record_resume,
            commands::record::record_toggle_pause,
            commands::record::record_stop,
            commands::record::record_cancel,
            commands::record::record_status,
            commands::record::record_set_lens,
            commands::record::record_set_interactive,
            commands::image::image_copy,
            commands::image::image_pin,
            commands::image::image_ocr,
            commands::image::image_ocr_languages,
            commands::image::ocr_engines,
            commands::image::ocr_open_engine_dir,
            commands::image::ocr_download_paddle,
            commands::image::image_info,
            commands::image::image_open_folder,
            // 剪贴板
            commands::clipboard::clipboard_list,
            commands::clipboard::clipboard_thumb,
            commands::clipboard::clipboard_stats,
            commands::clipboard::clipboard_paste,
            commands::clipboard::clipboard_toggle_pin,
            commands::clipboard::clipboard_remove,
            commands::clipboard::clipboard_clear,
            commands::clipboard::clipboard_copy_text,
            commands::clipboard::clipboard_open_file,
            commands::clipboard::clipboard_pin,
            commands::clipboard::clipboard_pin_current,
            commands::clipboard::clipboard_system_history_get,
            commands::clipboard::clipboard_system_history_set,
            commands::clipboard::clipboard_set_takeover_win_v,
            commands::clipboard::clipboard_takeover_win_v_status,
            // 剪贴板悬浮面板
            commands::clipboard::clipboard_panel_paste,
            commands::clipboard::clipboard_panel_hide,
            commands::clipboard::clipboard_panel_ready,
            commands::clipboard::clipboard_panel_target,
            // 图床
            commands::upload::upload_providers,
            commands::upload::upload_provider_save,
            commands::upload::upload_set_active,
            commands::upload::upload_image,
            commands::upload::upload_test,
            // 插件
            commands::plugins::plugins_state,
            commands::plugins::plugins_toggle,
            commands::plugins::plugins_reload,
            commands::plugins::plugins_open_dir,
        ])
        .run(tauri::generate_context!())
        .expect("SnipForge 启动失败");
}
