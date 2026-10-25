use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// 单一配置结构的唯一真相。
/// 所有字段都带 `#[serde(default)]`——老版本配置文件缺字段时自动补默认值，
/// 这是升级不炸用户配置的前提。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppSettings {
    pub version: u32,
    pub hotkeys: Hotkeys,
    pub save: SaveConfig,
    pub clipboard: ClipboardConfig,
    pub uploader: UploaderConfig,
    pub editor: EditorConfig,
    pub ocr: OcrConfig,
    pub record: RecordConfig,
    pub general: GeneralConfig,
    /// 插件开关表：plugin_id -> enabled
    pub plugin_enabled: HashMap<String, bool>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: 1,
            hotkeys: Hotkeys::default(),
            save: SaveConfig::default(),
            clipboard: ClipboardConfig::default(),
            uploader: UploaderConfig::default(),
            editor: EditorConfig::default(),
            ocr: OcrConfig::default(),
            record: RecordConfig::default(),
            general: GeneralConfig::default(),
            plugin_enabled: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Hotkeys {
    pub region_capture: String,
    pub fullscreen_capture: String,
    pub active_window_capture: String,
    pub scroll_capture: String,
    pub clipboard_panel: String,
    pub toggle_recording: String,
    pub pin_from_clipboard: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            region_capture: "Alt+Q".into(),
            fullscreen_capture: "Alt+Shift+Q".into(),
            active_window_capture: "Alt+W".into(),
            scroll_capture: "Alt+S".into(),
            clipboard_panel: "Alt+V".into(),
            toggle_recording: "Alt+R".into(),
            pin_from_clipboard: "Alt+P".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct SaveConfig {
    pub dir: String,
    pub name_template: String,
    pub after_save: String,
}

impl Default for SaveConfig {
    fn default() -> Self {
        Self {
            dir: String::new(),
            name_template: "SnipForge_{yyyy}{MM}{dd}_{HH}{mm}{ss}".into(),
            after_save: "copy".into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ClipboardConfig {
    pub enabled: bool,
    pub max_items: usize,
    pub poll_interval_ms: u64,
    pub ignore_apps: Vec<String>,
    pub skip_sensitive: bool,
    pub retention_days: u32,
    pub capture_images: bool,
    /// 用键盘钩子接管 Win+V，弹本工具的面板
    pub takeover_win_v: bool,
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_items: 2000,
            poll_interval_ms: 500,
            ignore_apps: Vec::new(),
            // 默认全量记录。过滤掉"看起来像密钥"的内容会误伤正常文本，
            // 对一个本地工具来说，宁可全存也别让用户找不到刚复制的东西。
            skip_sensitive: false,
            retention_days: 0,
            capture_images: true,
            // 默认接管 Win+V：这个工具的存在意义就是替代系统剪贴板，
            // 默认关掉等于功能默认不存在（用户会以为"按了没反应"）。
            // 不想要的可以在设置里关，钩子仍挂着但只放行。
            takeover_win_v: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UploaderConfig {
    pub enabled: bool,
    pub active_provider: String,
    pub auto_upload_on_capture: bool,
    pub copy_as_markdown: bool,
    /// provider_id -> 该 provider 的配置键值
    pub providers: HashMap<String, HashMap<String, serde_json::Value>>,
}

impl Default for UploaderConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            active_provider: "github".into(),
            auto_upload_on_capture: false,
            copy_as_markdown: false,
            providers: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct EditorConfig {
    pub stroke_color: String,
    pub stroke_width: f32,
    pub font_size: f32,
    pub pin_opacity: f32,
    pub watermark_text: String,
    pub watermark_enabled: bool,
}

impl Default for EditorConfig {
    fn default() -> Self {
        Self {
            stroke_color: "#FF3B30".into(),
            stroke_width: 3.0,
            font_size: 18.0,
            pin_opacity: 1.0,
            watermark_text: String::new(),
            watermark_enabled: false,
        }
    }
}

/// OCR 引擎选择。
/// 默认走系统自带的 Windows.Media.Ocr（零依赖、够用但精度一般）；
/// 追求中文精度可以换成 PaddleOCR-json 这类外部引擎。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct OcrConfig {
    /// windows | paddle
    pub engine: String,
    /// 外部引擎可执行文件路径，留空则自动在引擎目录里找
    pub paddle_path: String,
    /// 外部引擎超时（秒）
    pub timeout_secs: u64,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            engine: "windows".into(),
            paddle_path: String::new(),
            timeout_secs: 60,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RecordConfig {
    pub audio: bool,
    pub fps: u32,
    pub video_bits_per_second: u32,
    pub countdown: u32,
    pub show_cursor: bool,
}

impl Default for RecordConfig {
    fn default() -> Self {
        Self {
            audio: true,
            fps: 30,
            video_bits_per_second: 8_000_000,
            countdown: 3,
            show_cursor: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GeneralConfig {
    pub language: String,
    pub launch_at_login: bool,
    pub theme: String,
    pub minimize_to_tray: bool,
    pub show_notifications: bool,
    pub enable_plugins: bool,
    /// 关闭主窗口时是否真的退出
    pub close_to_tray: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            language: "zh-CN".into(),
            launch_at_login: false,
            theme: "system".into(),
            minimize_to_tray: true,
            show_notifications: true,
            enable_plugins: true,
            close_to_tray: true,
        }
    }
}

/// 配置版本迁移。
///
/// 老版本默认开了敏感内容过滤，导致很多正常文本被跳过。
/// 升到 v2 时统一关掉——用户升级后不该发现"复制了却没记录"。
pub fn migrate(cfg: &mut AppSettings) {
    if cfg.version < 2 {
        cfg.clipboard.skip_sensitive = false;
        cfg.version = 2;
        log::info!("配置迁移到 v2：关闭剪贴板敏感内容过滤");
    }
}

impl AppSettings {
    /// 局部更新：把 JSON patch 合并进当前配置。
    /// 手写深度合并而不是换整份，这样前端可以只提交改动的字段。
    pub fn apply_patch(&mut self, patch: &serde_json::Value) {
        let mut base = serde_json::to_value(&*self).unwrap_or(serde_json::Value::Null);
        deep_merge(&mut base, patch);
        if let Ok(next) = serde_json::from_value::<AppSettings>(base) {
            *self = next;
        }
    }
}

fn deep_merge(base: &mut serde_json::Value, patch: &serde_json::Value) {
    match (base, patch) {
        (serde_json::Value::Object(b), serde_json::Value::Object(p)) => {
            for (k, v) in p {
                match b.get_mut(k) {
                    Some(slot) => deep_merge(slot, v),
                    None => {
                        b.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        (slot, v) => {
            *slot = v.clone();
        }
    }
}
