//! 极简文件日志。
//!
//! 不引 env_logger / tracing：桌面应用只需要"出问题时能翻到一行记录"，
//! 不需要订阅过滤、span、结构化查询那一套。自己写 60 行，打包体积少 300KB。

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

/// 当前生效的日志级别。
///
/// 默认 Info：正常使用的日志量小，出问题时有足够的上下文。
/// 排查疑难（比如"剪贴板粘错条目"）时设 `SNIPFORGE_LOG=debug` 再启动，
/// 就能看到目标窗口句柄、列表首项、剪贴板占用重试这些细节。
static LEVEL: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(3);

fn level_rank(level: log::Level) -> usize {
    match level {
        log::Level::Error => 0,
        log::Level::Warn => 1,
        log::Level::Info => 2,
        log::Level::Debug => 3,
        log::Level::Trace => 4,
    }
}

struct FileLogger {
    file: Mutex<Option<File>>,
}

impl log::Log for FileLogger {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        level_rank(metadata.level()) <= LEVEL.load(std::sync::atomic::Ordering::Relaxed)
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
    // 级别可以由环境变量抬高，排查完重启就恢复，不用改代码重编
    let level = match std::env::var("SNIPFORGE_LOG").unwrap_or_default().to_lowercase().as_str() {
        "trace" => log::LevelFilter::Trace,
        "debug" => log::LevelFilter::Debug,
        "warn" => log::LevelFilter::Warn,
        "error" => log::LevelFilter::Error,
        _ => log::LevelFilter::Info,
    };
    LEVEL.store(
        level_rank(match level {
            log::LevelFilter::Trace => log::Level::Trace,
            log::LevelFilter::Debug => log::Level::Debug,
            log::LevelFilter::Warn => log::Level::Warn,
            log::LevelFilter::Error => log::Level::Error,
            _ => log::Level::Info,
        }),
        std::sync::atomic::Ordering::Relaxed,
    );
    log::set_max_level(level);
    log::info!("===== SnipForge {} 启动 =====", env!("CARGO_PKG_VERSION"));
    log::info!("日志文件: {}", path.display());
}
