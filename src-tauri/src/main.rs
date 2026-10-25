// Windows 发布版不要黑框
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // 启动早期就装钩子：任何 panic 都落一份到 exe 旁边的 crash.log。
    // 发布版没有控制台，没有这个就等于黑盒崩溃，用户只能跟你说"打不开"。
    std::panic::set_hook(Box::new(|info| {
        let text = format!(
            "[{}] PANIC: {}\n",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
            info
        );
        if let Some(dir) = std::env::current_exe().ok().and_then(|p| p.parent().map(|d| d.to_path_buf())) {
            use std::io::Write;
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("crash.log"))
            {
                let _ = f.write_all(text.as_bytes());
            }
        }
        eprintln!("{text}");
    }));

    if let Some(i) = std::env::args().position(|a| a == "--cursor-test") {
        let out = std::env::args()
            .nth(i + 1)
            .unwrap_or_else(|| "cursor.png".into());
        std::process::exit(snipforge_lib::cursor_test(&out));
    }

    if let Some(i) = std::env::args().position(|a| a == "--record-inspect") {
        let src = std::env::args().nth(i + 1).unwrap_or_default();
        let out = std::env::args().nth(i + 2);
        // 可选：--record-inspect <mp4> <png> <x> <y> <w> <h> 只裁一块
        // 可选：--record-inspect <mp4> <png> [x y w h] [--skip N]
        let rest: Vec<String> = std::env::args().skip(i + 3).collect();
        let skip_at = rest.iter().position(|v| v == "--skip");
        let skip = skip_at
            .and_then(|k| rest.get(k + 1))
            .and_then(|v| v.parse().ok())
            .unwrap_or(0u32);
        let nums: Vec<u32> = rest
            .iter()
            .take(skip_at.unwrap_or(rest.len()))
            .filter_map(|v| v.parse().ok())
            .collect();
        let crop = (nums.len() == 4).then(|| (nums[0], nums[1], nums[2], nums[3]));
        std::process::exit(snipforge_lib::record_inspect(&src, out.as_deref(), crop, skip));
    }

    if let Some(i) = std::env::args().position(|a| a == "--record-test") {
        let secs = std::env::args()
            .nth(i + 1)
            .and_then(|s| s.parse().ok())
            .unwrap_or(3);
        std::process::exit(snipforge_lib::record_screen_smoke(secs));
    }

    if std::env::args().any(|a| a == "--record-smoke") {
        std::process::exit(snipforge_lib::record_smoke());
    }

    if std::env::args().any(|a| a == "--smoke") {
        std::process::exit(snipforge_lib::smoke_test());
    }

    snipforge_lib::run()
}
