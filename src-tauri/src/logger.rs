//! 极简文件日志。
//!
//! 不引 env_logger / tracing：桌面应用只需要"出问题时能翻到一行记录"，
//! 不需要订阅过滤、span、结构化查询那一套。自己写 60 行，打包体积少 300KB。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

struct FileLogger {
    file: Mutex<Option<File>>,
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Info
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let now = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let line = format!(
            "[{}] [{:<5}] [{}] {}\n",
            now,
            record.level().as_str(),
            record.target(),
            record.args()
        );

        if let Ok(mut guard) = self.file.lock() {
            if let Some(f) = guard.as_mut() {
                let _ = f.write_all(line.as_bytes());
                let _ = f.flush();
            }
        }
        // 开发时同时打到 stderr，跑 tauri dev 能直接看到
        #[cfg(debug_assertions)]
        eprint!("{line}");
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock() {
            if let Some(f) = guard.as_mut() {
                let _ = f.flush();
            }
        }
    }
}

static LOGGER: std::sync::OnceLock<FileLogger> = std::sync::OnceLock::new();

pub fn init(log_dir: &PathBuf) {
    let _ = std::fs::create_dir_all(log_dir);
    let path = log_dir.join(format!(
        "snipforge-{}.log",
        chrono::Local::now().format("%Y-%m-%d")
    ));

    // 单文件超过 4MB 滚一次，别让日志吃掉磁盘
    if let Ok(meta) = std::fs::metadata(&path) {
        if meta.len() > 4 * 1024 * 1024 {
            let _ = std::fs::rename(&path, path.with_extension("log.1"));
        }
    }

    let file = OpenOptions::new().create(true).append(true).open(&path).ok();
    let logger = LOGGER.get_or_init(|| FileLogger {
        file: Mutex::new(None),
    });
    if let Ok(mut guard) = logger.file.lock() {
        *guard = file;
    }
    let _ = log::set_logger(logger);
    log::set_max_level(log::LevelFilter::Info);
    log::info!("===== SnipForge {} 启动 =====", env!("CARGO_PKG_VERSION"));
    log::info!("日志文件: {}", path.display());
}
