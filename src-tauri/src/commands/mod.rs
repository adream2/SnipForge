pub mod capture;
pub mod features;
pub mod hosts;
pub mod icons;
pub mod clipboard;
pub mod image;
pub mod plugins;
pub mod record;
pub mod settings;
pub mod update;
pub mod upload;
pub mod vault;

pub use features::*;
pub use settings::*;

use crate::error::{AppError, AppResult};

/// 用系统默认程序打开文件/目录（内部复用，不走 IPC）
pub fn app_open_path_inner(path: &std::path::Path) -> AppResult<()> {
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(path)
            .spawn()
            .map_err(|e| AppError::Msg(format!("打开失败: {e}")))?;
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        Err(AppError::Msg("仅支持 Windows".into()))
    }
}
