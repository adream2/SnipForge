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

    if std::env::args().any(|a| a == "--smoke") {
        std::process::exit(snipforge_lib::smoke_test());
    }

    snipforge_lib::run()
}
