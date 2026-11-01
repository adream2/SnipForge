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
        // GIF 没有音轨
        audio: false,
        mic: false,
        camera: false,
        camera_device: String::new(),
        camera_size: 20,
        camera_corner: "br".into(),
        camera_mirror: true,
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

    let opts_cam = std::env::args().any(|a| a == "--with-camera");
    let opts_mic = std::env::args().any(|a| a == "--with-mic");
    let opts = record::RecordOptions {
        rect,
        fps: 30,
        bitrate: 8_000_000,
        draw_cursor: true,
        // 真机自检带上声音：这是唯一能验证 WASAPI + AAC 链路的地方
        audio: true,
        mic: opts_mic,
        camera: opts_cam,
        camera_device: String::new(),
        camera_size: 20,
        camera_corner: "br".into(),
        camera_mirror: true,
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
            // 音频链路能不能通，只有这里能看出来 —— 录出来没声音时先看这一行
            if r.audio_ms > 0 {
                println!(
                    "[ok] 音轨 {}ms（与画面相差 {}ms）",
                    r.audio_ms,
                    r.audio_ms as i64 - r.duration_ms as i64
                );
            } else {
                println!("[--] 本次没有音轨（没开声音 / 没有播放设备 / AAC 编码器不可用）");
            }
            if opts_cam || opts_mic {
                println!(
                    "[{}] 人像 {} / 麦克风 {}",
                    if (!opts_cam || r.camera) && (!opts_mic || r.mic) { "ok" } else { "--" },
                    if r.camera { "已录到" } else { "没有" },
                    if r.mic { "已录到" } else { "没有" },
                );
            }
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

/// 主密码保险箱自检：`snipforge.exe --vault-selftest`
///
/// 为什么要有它：`cargo test` 在某些机器上链接不过（测试 harness 加载失败），
/// 而加密这种东西"读代码觉得对"没有意义 —— 必须真跑一遍。
/// 这里用真实的 `AppState` + 临时目录走完整流程：
/// 设密码 → 检查磁盘上有没有明文 → 模拟重启 → 解锁 → 关密码 → 检查明文回写。
pub fn vault_selftest() -> i32 {
    use crate::config::AppSettings;
    use crate::paths::Paths;
    use crate::state::AppState;

    let mut failures = 0;
    macro_rules! check {
        ($name:expr, $ok:expr) => {{
            if $ok {
                println!("[ok]   {}", $name);
            } else {
                println!("[FAIL] {}", $name);
                failures += 1;
            }
        }};
    }

    println!("== 主密码保险箱自检 ==");
    let dir = std::env::temp_dir().join("snipforge-vault-selftest");
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        println!("[FAIL] 建临时目录: {e}");
        return 1;
    }
    let paths = Paths::new(dir.clone(), None, dir.join("pictures"));
    if let Err(e) = paths.ensure() {
        println!("[FAIL] 初始化目录: {e}");
        return 1;
    }

    const TOKEN: &str = "SELFTEST-TOKEN-9f3a";
    const SECRET: &str = "selftest-secret-value";
    const DEEPL: &str = "selftest-deepl-key";
    const PW: &str = "selftest-password-123";

    let mut settings = AppSettings::default();
    settings
        .uploader
        .providers
        .entry("github".into())
        .or_default()
        .insert("token".into(), serde_json::json!(TOKEN));
    settings.translate.deepl_key = DEEPL.into();

    let st = AppState::new(settings, paths.clone());
    if let Err(e) = st.secrets.lock().upsert("", "自检条目", SECRET, "") {
        println!("[FAIL] 写入收藏: {e}");
        return 1;
    }
    if let Err(e) = st.persist_settings() {
        println!("[FAIL] 落盘设置: {e}");
        return 1;
    }
    check!("加密前 secrets.json 存在", paths.secrets_file().exists());

    // ---- 启用主密码 ----
    if let Err(e) = st.enable_vault(PW) {
        println!("[FAIL] 启用主密码: {e}");
        return 1;
    }
    check!("启用主密码", true);
    check!("明文 secrets.json 已删除", !paths.secrets_file().exists());

    let settings_raw = std::fs::read_to_string(paths.settings_file()).unwrap_or_default();
    check!("settings.json 不含图床 token", !settings_raw.contains(TOKEN));
    check!("settings.json 不含翻译 key", !settings_raw.contains(DEEPL));

    let vault_raw = std::fs::read_to_string(paths.data.join("vault.json")).unwrap_or_default();
    check!("vault.json 不含收藏明文", !vault_raw.contains(SECRET));
    check!("vault.json 不含 token", !vault_raw.contains(TOKEN));

    // ---- 模拟重启 ----
    let reloaded: AppSettings =
        serde_json::from_str(&settings_raw).unwrap_or_default();
    let st2 = AppState::new(reloaded, paths.clone());
    check!("重启后状态为「已锁定」", st2.vault.is_enabled() && !st2.vault.is_unlocked());
    check!("锁定状态读不到收藏", st2.secrets.lock().list().is_empty());
    check!("锁定状态读不到 token", st2.settings.read().uploader.providers.is_empty());

    check!("错误密码被拒绝", st2.unlock_vault("wrong-password").is_err());
    match st2.unlock_vault(PW) {
        Ok(()) => check!("正确密码可解锁", true),
        Err(e) => check!(format!("正确密码可解锁（{e}）"), false),
    }
    let secrets = st2.secrets.lock().list();
    check!(
        "解锁后收藏内容一致",
        secrets.len() == 1 && secrets[0].value == SECRET
    );
    let provider_token = st2
        .settings
        .read()
        .uploader
        .providers
        .get("github")
        .and_then(|m| m.get("token"))
        .and_then(|v| v.as_str())
        .unwrap_or_default()
        .to_string();
    check!("解锁后 token 一致", provider_token == TOKEN);
    check!("解锁后翻译 key 一致", st2.settings.read().translate.deepl_key == DEEPL);

    // ---- 关闭主密码：明文要写回来 ----
    match st2.disable_vault() {
        Ok(()) => check!("关闭主密码", true),
        Err(e) => check!(format!("关闭主密码（{e}）"), false),
    }
    let secrets_raw = std::fs::read_to_string(paths.secrets_file()).unwrap_or_default();
    check!("明文 secrets.json 已写回", secrets_raw.contains(SECRET));
    let settings_raw2 = std::fs::read_to_string(paths.settings_file()).unwrap_or_default();
    check!("token 已写回 settings.json", settings_raw2.contains(TOKEN));
    check!("vault.json 已删除", !paths.data.join("vault.json").exists());

    let _ = std::fs::remove_dir_all(&dir);

    if failures == 0 {
        println!("== 全部通过 ==");
        0
    } else {
        println!("== {failures} 项失败 ==");
        1
    }
}

/// 画中画合成自检：`snipforge.exe --pip-test [out.png]`
///
/// 为什么需要它：这台开发机没有摄像头，画中画链路没法真机验证。
/// 但"能不能合成对"（位置、圆角、描边、镜像、不越界）是纯像素逻辑，
/// 可以用合成图验证 —— 比"读代码觉得对"强得多。
pub fn pip_test(out: Option<&str>) -> i32 {
    use crate::record::camera;

    let (w, h) = (1280u32, 720u32);
    let mut bg = vec![0u8; (w * h * 4) as usize];
    for y in 0..h {
        for x in 0..w {
            let c: u8 = if ((x / 40) + (y / 40)) % 2 == 0 { 58 } else { 92 };
            let o = ((y * w + x) * 4) as usize;
            bg[o] = c;
            bg[o + 1] = c;
            bg[o + 2] = c.saturating_add(24);
            bg[o + 3] = 255;
        }
    }

    // 假"人像"的尺寸必须**等于布局尺寸**：composite 是 1:1 贴图
    // （缩放由 MF 在采集端做完），尺寸不一致会被裁掉一部分。
    let proto = camera::layout_for(w, h, 25, 2, "br", 16.0 / 9.0);
    let (fw, fh) = (proto.w, proto.h);
    let mut face = vec![0u8; (fw * fh * 4) as usize];
    for y in 0..fh {
        for x in 0..fw {
            let o = ((y * fw + x) * 4) as usize;
            // 左红右蓝 —— 镜像一眼就能看出来（正确时蓝色在左边）
            let (r, g, b) = if x < fw / 2 { (220, 64, 64) } else { (64, 64, 220) };
            face[o] = r;
            face[o + 1] = g;
            face[o + 2] = b;
            face[o + 3] = 255;
        }
    }

    let mask = camera::Mask::rounded(fw, fh, 12, 4.0);
    let layout = camera::place(w, h, fw, fh, 2, "br");
    println!("布局: {layout:?}");
    camera::composite(
        &mut bg,
        w,
        h,
        &face,
        fw,
        fh,
        layout,
        true,
        &mask,
        [255, 255, 255, 255],
    );

    let path = out
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("snipforge-pip.png"));

    // ---- 自动断言：不靠肉眼，锁住几个"一眼看不出来但会坏"的点 ----
    let px = |x: u32, y: u32| -> [u8; 4] {
        let o = ((y * w + x) * 4) as usize;
        [bg[o], bg[o + 1], bg[o + 2], bg[o + 3]]
    };
    let mut fails = 0;
    let mut check = |name: &str, ok: bool| {
        println!("[{}] {name}", if ok { "ok" } else { "FAIL" });
        if !ok {
            fails += 1;
        }
    };

    let (l, t) = (layout.x as u32, layout.y as u32);
    let (r, b) = (l + layout.w - 1, t + layout.h - 1);
    let mid_y = t + layout.h / 2;
    let mid_x = l + layout.w / 2;

    // 镜像：源图左红右蓝，镜像后左边应该是蓝
    let left = px(l + 30, mid_y);
    let right = px(r - 30, mid_y);
    check(
        "镜像正确（左蓝右红）",
        left[2] > 150 && left[0] < 100 && right[0] > 150 && right[2] < 100,
    );
    // 描边：四条边中点内侧都应该是白的
    check("上边有描边", px(mid_x, t + 1).iter().take(3).all(|c| *c > 200));
    check("下边有描边", px(mid_x, b - 1).iter().take(3).all(|c| *c > 200));
    check("左边有描边", px(l + 1, mid_y).iter().take(3).all(|c| *c > 200));
    check("右边有描边", px(r - 1, mid_y).iter().take(3).all(|c| *c > 200));
    // 圆角：角上必须透出背景（棋盘色是灰蓝，不是红也不是白）
    let corner = px(l + 1, t + 1);
    check(
        "左上角是圆角（露出背景）",
        corner[0] < 140 && corner[2] < 160 && !corner.iter().take(3).all(|c| *c > 200),
    );
    // 不越界：画中画外侧一圈必须还是棋盘背景
    check(
        "右侧未越界",
        px((r + 5).min(w - 1), mid_y)[0] < 140,
    );
    check(
        "下方未越界",
        px(mid_x, (b + 5).min(h - 1))[0] < 140,
    );

    let img = match image::RgbaImage::from_raw(w, h, bg.clone()) {
        Some(i) => i,
        None => {
            println!("[fail] 缓冲尺寸不对");
            return 1;
        }
    };
    if let Err(e) = img.save(&path) {
        println!("[fail] 保存失败: {e}");
        return 1;
    }
    println!("[ok] 已生成 {}", path.display());

    // 再存一张 2 倍放大的画中画特写，方便人眼核对圆角/描边
    let pad = 16u32;
    let cx = (layout.x.max(0) as u32).saturating_sub(pad);
    let cy = (layout.y.max(0) as u32).saturating_sub(pad);
    let cw = (layout.w + pad * 2).min(w - cx);
    let chh = (layout.h + pad * 2).min(h - cy);
    let crop = image::imageops::crop_imm(&img, cx, cy, cw, chh).to_image();
    let big = image::imageops::resize(
        &crop,
        cw * 2,
        chh * 2,
        image::imageops::FilterType::Nearest,
    );
    let crop_path = path.with_file_name("pip-test-crop.png");
    if big.save(&crop_path).is_ok() {
        println!("[ok] 特写 {}", crop_path.display());
    }

    // 顺带把四个角的布局也打出来，人工核对用
    for corner in ["tl", "tr", "bl", "br"] {
        let l = camera::layout_for(w, h, 20, 2, corner, 16.0 / 9.0);
        println!("  {corner}: x={} y={} w={} h={}", l.x, l.y, l.w, l.h);
    }

    if fails == 0 {
        println!("== 全部通过 ==");
        0
    } else {
        println!("== {fails} 项失败 ==");
        1
    }
}

/// 图标提取自检：`snipforge.exe --icon-test <文件> [输出目录]`
///
/// 真机验证用：打印每个图标的尺寸和来源，并把最大的那个存成 PNG，
/// 这样"提取出来的到底是不是图标"能直接看图确认，不用靠猜。
pub fn icon_test(file: &str, out_dir: Option<&str>) -> i32 {
    let path = std::path::Path::new(file);
    println!("== 图标提取自检 ==");
    println!("文件: {}", path.display());

    let bundle = match crate::icons::extract(path) {
        Ok(b) => b,
        Err(e) => {
            println!("[fail] {e}");
            return 1;
        }
    };

    println!("类型: {}  名称: {}", bundle.kind, bundle.name);
    for n in &bundle.notes {
        println!("  · {n}");
    }
    if bundle.icons.is_empty() {
        println!("[fail] 没有提取到任何图标");
        return 1;
    }

    let dir = out_dir
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::env::temp_dir().join("snipforge-icons"));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        println!("[fail] 建输出目录: {e}");
        return 1;
    }

    for (i, icon) in bundle.icons.iter().enumerate() {
        println!(
            "[ok] #{} {}×{}  {} KB  <- {}",
            i + 1,
            icon.width,
            icon.height,
            icon.bytes / 1024,
            icon.source
        );
        // 只存前 3 个（已按尺寸降序），够看效果了
        if i < 3 {
            let Some((_, payload)) = icon.data_url.split_once(',') else {
                continue;
            };
            use base64::Engine;
            if let Ok(bytes) =
                base64::engine::general_purpose::STANDARD.decode(payload)
            {
                let f = dir.join(format!("icon-{i}-{}x{}.png", icon.width, icon.height));
                if let Err(e) = std::fs::write(&f, bytes) {
                    println!("[fail] 写 {f:?}: {e}");
                } else {
                    println!("      -> {}", f.display());
                }
            }
        }
    }
    println!("== 完成，共 {} 个 == ", bundle.icons.len());
    0
}

/// 检查更新自检：`snipforge.exe --update-check`
///
/// 为什么要有它：更新检查失败时界面只会说"检查失败"，用户和我们都不知道
/// 是网络、限流，还是仓库没公开。这条命令把原始判断打出来，问题一眼看清。
pub fn update_check_test() -> i32 {
    println!("== 检查更新 ==");
    println!("当前版本: {}", env!("CARGO_PKG_VERSION"));
    match crate::update::fetch_latest() {
        Ok(i) => {
            println!(
                "最新版本: {}（{}）",
                i.latest,
                if i.prerelease { "预发布" } else { "正式版" }
            );
            println!("发布时间: {}", i.published_at);
            println!(
                "{}",
                if i.has_update {
                    format!("→ 有新版本可用（当前 {}）", i.current)
                } else {
                    "→ 已是最新".to_string()
                }
            );
            println!("发布页: {}", i.url);
            if !i.notes.is_empty() {
                println!("--- 发布说明 ---");
                for line in i.notes.lines().take(20) {
                    println!("{line}");
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
