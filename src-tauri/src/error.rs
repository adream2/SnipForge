use serde::{Serialize, Serializer};

/// 统一错误类型。
/// 实现了 Serialize，所以能直接当 Tauri command 的错误返回，
/// 前端拿到的永远是一个可读的字符串，不会看到 Rust 的 Debug 噪音。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    Msg(String),

    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON 解析失败: {0}")]
    Json(#[from] serde_json::Error),

    #[error("图像处理失败: {0}")]
    Image(#[from] image::ImageError),

    #[error("网络请求失败: {0}")]
    Http(#[from] reqwest::Error),

    #[error("剪贴板不可用: {0}")]
    Clipboard(String),

    #[error("系统调用失败: {0}")]
    Win32(String),
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.to_string())
    }
}

impl From<String> for AppError {
    fn from(s: String) -> Self {
        AppError::Msg(s)
    }
}

impl From<&str> for AppError {
    fn from(s: &str) -> Self {
        AppError::Msg(s.to_string())
    }
}

impl From<arboard::Error> for AppError {
    fn from(e: arboard::Error) -> Self {
        AppError::Clipboard(e.to_string())
    }
}

impl From<tauri::Error> for AppError {
    fn from(e: tauri::Error) -> Self {
        AppError::Msg(e.to_string())
    }
}

/// Win32 / Media Foundation 的 HRESULT 直接转过来。
/// 录屏那段全是 COM 调用，逐处 map_err 会把代码淹没。
#[cfg(windows)]
impl From<windows::core::Error> for AppError {
    fn from(e: windows::core::Error) -> Self {
        AppError::Win32(format!("{} ({:#x})", e.message(), e.code().0))
    }
}

impl From<base64::DecodeError> for AppError {
    fn from(e: base64::DecodeError) -> Self {
        AppError::Msg(format!("Base64 解码失败: {e}"))
    }
}

pub type AppResult<T> = Result<T, AppError>;
