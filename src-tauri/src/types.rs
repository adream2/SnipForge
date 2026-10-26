use serde::{Deserialize, Serialize};

/* ---------------- 几何 ---------------- */

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    pub fn right(&self) -> i32 {
        self.x + self.width as i32
    }
    pub fn bottom(&self) -> i32 {
        self.y + self.height as i32
    }
    pub fn is_empty(&self) -> bool {
        self.width == 0 || self.height == 0
    }

    /// 求交集，无交集返回 None
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        if r <= x || b <= y {
            None
        } else {
            Some(Rect {
                x,
                y,
                width: (r - x) as u32,
                height: (b - y) as u32,
            })
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

/* ---------------- 截图 ---------------- */

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayInfo {
    pub id: u32,
    pub name: String,
    /// 逻辑坐标（DIP）
    pub bounds: Rect,
    pub scale_factor: f64,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureResult {
    /// data URL，PNG
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f64,
    pub source: String,
    pub timestamp: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FramePayload {
    pub union: Rect,
    pub displays: Vec<DisplayPreview>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DisplayPreview {
    pub id: u32,
    pub bounds: Rect,
    pub scale_factor: f64,
    /// JPEG data URL，尺寸为逻辑像素，只用于展示
    pub preview: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeWindowInfo {
    pub hwnd: i64,
    pub title: String,
    pub class_name: String,
    pub rect: Rect,
    pub exe: String,
    pub ignored: bool,
}

/* ---------------- 剪贴板 ---------------- */

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardItem {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub html: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blob: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub width: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub height: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub files: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    pub hash: String,
    pub size: u64,
    /// 来源应用进程名（explorer / msedge / Code...）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_app: Option<String>,
    pub pinned: bool,
    pub tags: Vec<String>,
    pub created_at: i64,
    pub last_used_at: i64,
    pub hits: u32,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardQuery {
    pub keyword: Option<String>,
    pub kind: Option<String>,
    pub pinned_only: Option<bool>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardStats {
    pub total: usize,
    pub by_kind: std::collections::HashMap<String, usize>,
    pub db_bytes: u64,
}

/* ---------------- OCR ---------------- */

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrWord {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrLine {
    pub text: String,
    pub words: Vec<OcrWord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OcrResult {
    pub text: String,
    pub lines: Vec<OcrLine>,
    pub language: String,
    pub duration_ms: u64,
}

/* ---------------- 图床 ---------------- */

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub field_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Vec<FieldOption>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visible_when: Option<VisibleWhen>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldOption {
    pub label: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibleWhen {
    pub key: String,
    pub equals: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderMeta {
    pub id: String,
    pub name: String,
    /// builtin | plugin | declarative
    pub origin: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub fields: Vec<ConfigField>,
    pub requires_network: bool,
    /// 能否给单次上传指定目录（GitHub / S3 / 本地目录这类有路径概念的）
    #[serde(default)]
    pub supports_dir: bool,
    /// 能否列出远端已有目录（用于"选已有文件夹"）
    #[serde(default)]
    pub can_browse: bool,
}

/// 图床里的一个目录（浏览用）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntry {
    /// 显示名（最后一段）
    pub name: String,
    /// 相对该图床根的完整路径
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadResult {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delete_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thumb_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadOutcome {
    pub provider_id: String,
    pub provider_name: String,
    pub result: UploadResult,
    pub duration_ms: u64,
}

/* ---------------- 插件 ---------------- */

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInfo {
    pub id: String,
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default)]
    pub author: Option<String>,
    /// builtin | user
    pub origin: String,
    pub enabled: bool,
    pub loaded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(default)]
    pub hooks: Vec<String>,
    #[serde(default)]
    pub providers: Vec<String>,
    pub dir: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginState {
    pub plugins: Vec<PluginInfo>,
    pub builtin_dir: String,
    pub user_dir: String,
}

/* ---------------- 应用信息 ---------------- */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsView {
    pub settings: crate::config::AppSettings,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    pub platform: String,
    pub data_dir: String,
    pub ocr_languages: Vec<String>,
    pub hotkey_results: Vec<HotkeyResult>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyResult {
    pub action: String,
    pub accelerator: String,
    pub ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageInfoView {
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub bytes: u64,
}

/// 从磁盘读一张图片交给「图片工具」。
///
/// 为什么不走 `convertFileSrc` + fetch：
/// 那需要开 asset 协议并把它加进 CSP 的 connect-src，等于给 webview 开一个
/// 任意文件读取口子；而且默认配置下根本不通 —— 图片工具"点了没反应"就是这个。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImageFileView {
    pub data_url: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOutcome {
    pub path: String,
    pub bytes: u64,
    pub format: String,
}
