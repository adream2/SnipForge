//! OCR 引擎管理层。
//!
//! 默认用 Windows 自带的 Windows.Media.Ocr：零依赖、零下载、不联网，
//! 对英文和常规排版够用，但中文小字、复杂背景的识别率确实一般。
//!
//! 所以做成可插拔的：追求精度可以换 PaddleOCR 这类外部引擎。
//! 选型上坚持一条 —— **引擎本体不随安装包分发**，用户自己决定要不要下、
//! 下哪个版本、放哪。安装包永远 2MB，不因为 OCR 变成 30MB。

pub mod paddle;
pub mod windows;

use crate::error::AppResult;
use crate::types::OcrResult;

/// 一个可用引擎的描述，给设置页展示
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineInfo {
    pub id: String,
    pub name: String,
    /// 当前是否可用（外部引擎要检测文件是否存在）
    pub available: bool,
    /// 不可用时的原因 / 可用时的补充说明
   pub detail: String,
    /// 识别语言（Windows 引擎支持多语言，Paddle 看模型）
    pub languages: Vec<String>,
    pub builtin: bool,
}

pub const WINDOWS: &str = "windows";
pub const PADDLE: &str = "paddle";

/// 列出所有引擎及其实时可用状态
pub fn engines(paddle_path: &str) -> Vec<EngineInfo> {
    let win_langs = windows::available_languages();
    let paddle_exe = paddle::find_exe(paddle_path);

    vec![
        EngineInfo {
            id: WINDOWS.into(),
            name: "Windows 原生".into(),
            available: !win_langs.is_empty(),
            detail: if win_langs.is_empty() {
                "系统未安装 OCR 语言包".into()
            } else {
                "零依赖 · 不联网 · 精度一般".into()
            },
            languages: win_langs,
            builtin: true,
        },
        EngineInfo {
            id: PADDLE.into(),
            name: "PaddleOCR".into(),
            available: paddle_exe.is_some(),
            detail: match &paddle_exe {
                Some(p) => format!("已就绪 · {}", p.display()),
                None => "未安装 · 需要下载 PaddleOCR-json 放到引擎目录".into(),
            },
            languages: if paddle_exe.is_some() {
                vec!["中文".into(), "英文".into()]
            } else {
                Vec::new()
            },
            builtin: false,
        },
    ]
}

/// 按配置选择引擎执行识别。
/// 外部引擎不可用时**自动回退到 Windows**，而不是直接报错 ——
/// 用户配了 Paddle 又把目录删了，不该让功能整个失效。
pub fn recognize(png: &[u8], cfg: &crate::config::OcrConfig) -> AppResult<OcrResult> {
    match cfg.engine.as_str() {
        PADDLE => match paddle::find_exe(&cfg.paddle_path) {
            Some(exe) => match paddle::recognize(png, &exe, cfg.timeout_secs) {
                Ok(result) => Ok(result),
                Err(e) => {
                    log::warn!("PaddleOCR 识别失败，回退到 Windows OCR: {e}");
                    windows::recognize(png)
                }
            },
            None => {
                log::warn!("未找到 PaddleOCR，回退到 Windows OCR");
                windows::recognize(png)
            }
        },
        _ => windows::recognize(png),
    }
}

pub fn available_languages() -> Vec<String> {
    windows::available_languages()
}
