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
    pub capture: CaptureConfig,
    pub save: SaveConfig,
    pub clipboard: ClipboardConfig,
    pub uploader: UploaderConfig,
    pub editor: EditorConfig,
    pub ocr: OcrConfig,
    pub record: RecordConfig,
    pub general: GeneralConfig,
    /// 鼠标手势
    pub gestures: GestureConfig,
    /// 翻译服务
    pub translate: TranslateConfig,
    /// 插件开关表：plugin_id -> enabled
    pub plugin_enabled: HashMap<String, bool>,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: 1,
            hotkeys: Hotkeys::default(),
            capture: CaptureConfig::default(),
            save: SaveConfig::default(),
            clipboard: ClipboardConfig::default(),
            uploader: UploaderConfig::default(),
            editor: EditorConfig::default(),
            ocr: OcrConfig::default(),
            record: RecordConfig::default(),
            general: GeneralConfig::default(),
            gestures: GestureConfig::default(),
            translate: TranslateConfig::default(),
            plugin_enabled: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Hotkeys {
    /// 唯一的截图入口。全屏 / 窗口 / 滚动长截图都在选区里选 ——
    /// 窗口一直是这样（没有独立入口），现在全屏（Ctrl+A）和
    /// 滚动（S）也并进去了，省掉两个只多一个按键的"独立动作"。
    pub region_capture: String,
    pub clipboard_panel: String,
    pub toggle_recording: String,
    /// 录 GIF（和录视频分开，选中什么录什么）
    pub toggle_recording_gif: String,
    pub pin_from_clipboard: String,
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            region_capture: "Alt+Q".into(),
            clipboard_panel: "Alt+V".into(),
            // Alt+R 在不少机器上被别的软件占着，Ctrl+Alt+R 更安全
            toggle_recording: "Ctrl+Alt+R".into(),
            toggle_recording_gif: "Ctrl+Alt+G".into(),
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

/// 截图。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct CaptureConfig {
    /// 截完立刻干什么：
    ///   - `editor` 进编辑器（标注 / 打码 / OCR / 上传的入口都在这儿）
    ///   - `copy`   直接进剪贴板，不再开窗口 —— "截图→粘到聊天框"是最快的路
    ///   - `save`   直接按 `save` 规则落盘
    ///   - `pin`    直接钉在屏幕上
    ///
    /// 这是**提交之后**的行为，和 `save.afterSave`（保存之后）不是一回事。
    pub after: String,
}

impl Default for CaptureConfig {
    fn default() -> Self {
        // 默认进编辑器：截错了还能改，而且多数人截完是要标注/发出去的
        Self {
            after: "editor".into(),
        }
    }
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
    pub copy_as_markdown: bool,
    /// 上传前压缩（在渲染进程用 canvas 做，能出**有损 WebP**；
    /// 后端 image crate 只支持无损 WebP，效果差一截）
    pub compress: bool,
    /// 转换目标：keep | webp | jpeg | png
    pub compress_format: String,
    /// 有损格式的质量 1-100
    pub compress_quality: u32,
    /// 最长边上限（像素），0 = 不限
    pub compress_max_edge: u32,
    /// provider_id -> 该 provider 的配置键值
    pub providers: HashMap<String, HashMap<String, serde_json::Value>>,
}

impl Default for UploaderConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            active_provider: "github".into(),
            copy_as_markdown: false,
            // 默认压缩：图床场景绝大多数是"发给别人看"，不是存档原图。
            // WebP q90 对截图几乎无损，体积常常只剩 PNG 的三分之一。
            compress: true,
            compress_format: "webp".into(),
            compress_quality: 90,
            compress_max_edge: 0,
            providers: HashMap::new(),
        }
    }
}

/// 翻译服务配置。
///
/// 默认腾讯：谷歌那个免 key 端点在**国内基本连不上**，用户点开翻译只看到
/// "不可达"；腾讯（transmart 网页那套）国内直连、免 key，还自带语言识别。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TranslateConfig {
    /// microsoft | google | deepl | custom
    pub provider: String,
    /// 目标语言（规范码：zh-CN / zh-TW / en / ja …）
    pub target: String,
    /// DeepL 的 key（`:fx` 结尾是免费版）
    pub deepl_key: String,
    /// 自定义接口：URL，可用 `{text}`、`{to}` 占位
    pub custom_url: String,
    /// GET | POST
    pub custom_method: String,
    /// 每行一个 `Header: value`
    pub custom_headers: String,
    /// 请求体模板（POST 用），可用 `{text}`、`{to}` 占位
    pub custom_body: String,
    /// 结果的点路径，如 `data.translations.0.text`；留空则整个响应当译文
    pub custom_result_path: String,
}

impl Default for TranslateConfig {
    fn default() -> Self {
        Self {
            provider: "tencent".into(),
            target: "zh-CN".into(),
            deepl_key: String::new(),
            custom_url: String::new(),
            custom_method: "POST".into(),
            custom_headers: "Content-Type: application/json".into(),
            custom_body: r#"{"text":"{text}","to":"{to}"}"#.into(),
            custom_result_path: String::new(),
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
    /// 下载源策略：auto（镜像优先）| direct（只走 GitHub）| custom（自定义前缀优先）
    pub download_mode: String,
    /// 自定义加速前缀，如 `https://ghfast.top/`（"前缀 + 原 GitHub 地址"形式）
    pub mirror_prefix: String,
}

impl Default for OcrConfig {
    fn default() -> Self {
        Self {
            engine: "windows".into(),
            paddle_path: String::new(),
            timeout_secs: 60,
            download_mode: "auto".into(),
            mirror_prefix: String::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct RecordConfig {
    /// 录系统声音（WASAPI loopback，抓的是默认播放设备的输出）
    pub audio: bool,
    /// 录麦克风。和系统声音同时开时两路混音。
    pub mic: bool,
    /// 摄像头画中画
    pub camera: bool,
    /// 摄像头设备名（空 = 用第一个可用设备）
    pub camera_device: String,
    /// 画中画宽度占录制画面宽度的百分比
    pub camera_size: u32,
    /// 画中画位置：br / bl / tr / tl
    pub camera_corner: String,
    /// 画中画是否水平镜像
    pub camera_mirror: bool,
    pub fps: u32,
    pub video_bits_per_second: u32,
    pub countdown: u32,
    pub show_cursor: bool,
    /// 鼠标高亮环
    pub cursor_highlight: bool,
    /// 放大镜
    pub magnifier: bool,
    /// 录像时允许在屏幕上标注
    pub annotate: bool,
    /// GIF 帧率（GIF 以 1/100 秒计时，太高只会白白变大）
    pub gif_fps: u32,
    /// GIF 输出最大宽度，0 = 原尺寸
    pub gif_width: u32,
}

impl Default for RecordConfig {
    fn default() -> Self {
        Self {
            // 默认录声音：录屏十次有九次是为了讲清楚一件事，
            // 无声视频等于白录，用户还得回头再录一遍
            audio: true,
            // 麦克风默认关：很多人录屏并不想把自己的声音录进去，
            // 而且会议室里默认开麦是件很尴尬的事
            mic: false,
            camera: false,
            camera_device: String::new(),
            camera_size: 20,
            camera_corner: "br".into(),
            camera_mirror: true,
            fps: 30,
            video_bits_per_second: 8_000_000,
            countdown: 3,
            show_cursor: true,
            // 三个"让录屏好看"的开关，默认全开 —— 用户要的就是这个
            cursor_highlight: true,
            magnifier: false,
            annotate: true,
            // 12fps：兼顾鼠标移动的顺滑和体积。GIF 到 15 以上基本只剩体积在涨。
            gif_fps: 12,
            // 900px：贴在聊天窗口里刚好，再宽体积翻倍涨
            gif_width: 900,
        }
    }
}

/// 手势能做的事。
///
/// 刻意**不只放本工具的功能** —— 鼠标手势首先是系统层面的通用工具：
/// 后退、关标签、显示桌面这些才是天天要用的。SnipForge 自己的截图/录屏
/// 只是 `Snip` 这一类，和别的动作平级，不占特殊位置。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum GestureAction {
    /// 发送按键，如 "Ctrl+Shift+T"；多组用逗号分隔，按顺序发
    Keys { keys: String },
    /// 当前窗口操作：close / minimize / maximize / restore / topmost
    Window { op: String },
    /// 打开程序、文件或文件夹（走 shell，.lnk 和文件夹都行）
    App { path: String },
    /// 用默认浏览器打开网址
    Url { url: String },
    /// 运行一条命令
    Cmd { cmd: String },
    /// 媒体与系统键：vol-up / vol-down / mute / play / next / prev
    Media { key: String },
    /// 输入一段文字（常用回复、邮箱、命令片段）
    Text { text: String },
    /// 滚动：up / down，amount 是格数
    Scroll { dir: String, amount: i32 },
    /// 本工具的功能：region / fullscreen / window / scroll / panel / pin / record / record-gif
    Snip { id: String },
}

/// 一条手势绑定：方向串（或形状字符）→ 动作。
///
/// 方向串只用四个字符：`U` `D` `L` `R`。不搞八方向 ——
/// 斜线人手画不稳，识别率会掉，而常用手势本来都是一两笔直着划。
/// 曲线手势用形状字符表示，目前只有 `S`（见 `win32::shape`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GestureBinding {
    /// 形如 "L"、"DR"、"UDL"，或者形状 "S"。
    /// 用户自己画的手势这里是空串（只靠 `points` 匹配）。
    pub pattern: String,
    /// 用户画的笔画模板（32 点，等比归一化到长边 1）。
    /// 非空就说明这是"自定义手势"，按形状匹配，不看方向串。
    #[serde(default)]
    pub points: Vec<[f32; 2]>,
    /// 老配置里的单个动作。只为读旧文件保留，迁移后会被搬进 `actions`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action: Option<GestureAction>,
    /// 动作序列。单动作 = 长度 1；**复合动作**就是按顺序做好几件事
    /// （比如「关闭窗口 → 切回上一个窗口」），中间隔 `COMPOUND_GAP`。
    #[serde(default)]
    pub actions: Vec<GestureAction>,
    #[serde(default = "default_true")]
    pub enabled: bool,
}

impl GestureBinding {
    /// 这一条要执行的动作序列。老的单动作写法也能取出来。
    pub fn steps(&self) -> Vec<GestureAction> {
        if !self.actions.is_empty() {
            self.actions.clone()
        } else {
            self.action.iter().cloned().collect()
        }
    }
}

fn default_true() -> bool {
    true
}

fn default_gesture_bindings() -> Vec<GestureBinding> {
    let b = |pattern: &str, action: GestureAction| GestureBinding {
        pattern: pattern.into(),
        points: Vec::new(),
        action: None,
        actions: vec![action],
        enabled: true,
    };
    let keys = |k: &str| GestureAction::Keys { keys: k.into() };
    let win = |op: &str| GestureAction::Window { op: op.into() };

    // 常规手势沿用 mousetroke / 主流手势软件的习惯（后退、关标签、显示桌面……），
    // 后面三条是本工具自己的：画个 S 截图、↓→↑← 框选贴图、↓→↑ 取字。
    //
    // **形状必须一眼能分开**：以前这里同时有 `→↓←↑`(OCR) 和 `↓→↑←`(贴图) ——
    // 两个都是闭合方框，笔画顺序不同但画出来一模一样，列表里看着就是
    // "两个一模一样的手势干了不同的事"。同理还删掉了 `↓↑`(恢复标签)：
    // 它和 `↑↓`(刷新) 只差一个起笔方向，静态看几乎分辨不出来。
    // 全部可以改、可以删。
    vec![
        // 编辑：上复制、下粘贴 —— 手势里最高频的两个
        b("U", keys("Ctrl+C")),
        b("D", keys("Ctrl+V")),
        // 浏览
        b("R", keys("Alt+Right")),
        b("L", keys("Alt+Left")),
        // 窗口
        b("D,R", win("close")),
        b("D,L", win("close-similar")),
        b("UR", win("toggle-max")),
        b("DL", win("minimize")),
        b("UL", keys("Esc")),
        b("DR", keys("Delete")),
        // 工具
        b("L,D,R,D,L", GestureAction::Snip {
            id: "search-selection".into(),
        }),
        b("U,R,D,L", GestureAction::Snip {
            id: "ping-selection".into(),
        }),
        b("D,U,R,D", GestureAction::Snip {
            id: "hosts".into(),
        }),
        b("D,R,U,L", GestureAction::Snip {
            id: "pin-region".into(),
        }),
        b("D,L,U,R", GestureAction::Snip {
            id: "ocr-region".into(),
        }),
        // 截图：↙↘↙
        b("DL,DR,DL", GestureAction::Snip {
            id: "region".into(),
        }),
    ]
}

/// 一套"程序专用"手势。
///
/// 命中这些进程时**先用这里的绑定**，没写到的动作再回落到通用手势 ——
/// 浏览器里"右键划 `↓→` 关标签"很自然，但在资源管理器里同样一划会把人吓一跳。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GestureProfile {
    /// 稳定 id：预设用固定串（browser），自定义程序用 exe 名
    pub id: String,
    pub name: String,
    /// 进程名（小写，带不带 .exe 都认）
    #[serde(default)]
    pub exes: Vec<String>,
    #[serde(default)]
    pub bindings: Vec<GestureBinding>,
}

/// 出厂带一个"浏览器"专用集：主流的几个壳都塞进去，
/// 用户不用自己去填 exe 名 —— 这是这一页最容易被卡住的地方。
fn default_profiles() -> Vec<GestureProfile> {
    let b = |pattern: &str, action: GestureAction| GestureBinding {
        pattern: pattern.into(),
        points: Vec::new(),
        action: None,
        actions: vec![action],
        enabled: true,
    };
    let keys = |k: &str| GestureAction::Keys { keys: k.into() };
    vec![GestureProfile {
        id: "browser".into(),
        name: "浏览器".into(),
        exes: vec![
            "chrome".into(),
            "msedge".into(),
            "firefox".into(),
            "brave".into(),
            "vivaldi".into(),
            "opera".into(),
            "360se".into(),
            "360chrome".into(),
            "qqbrowser".into(),
            "sogouexplorer".into(),
        ],
        bindings: vec![
            b("U", keys("Ctrl+Home")),
            b("D", keys("Ctrl+End")),
            b("DR,UR", keys("F5")),
            b("UR,DR", keys("Ctrl+Shift+R")),
            b("D,R", keys("Ctrl+W")),
            b("U,R", keys("Ctrl+Tab")),
            b("U,L", keys("Ctrl+Shift+Tab")),
            b("D,R,D", keys("Alt+Home")),
        ],
    }]
}

/// 出厂的默认手势（通用 + 程序专用）。前端"恢复默认"要用它，
/// 不能自己再抄一份表 —— 抄了就会和后端漂移。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GestureDefaults {
    pub bindings: Vec<GestureBinding>,
    pub profiles: Vec<GestureProfile>,
}

impl GestureDefaults {
    pub fn load() -> Self {
        let d = GestureConfig::default();
        Self {
            bindings: d.bindings,
            profiles: d.profiles,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GestureConfig {
    pub enabled: bool,
    /// 触发键：right / middle / x1（侧键）
    pub trigger: String,
    /// 按下后移动超过这么多像素才算手势，否则当普通点击
    pub threshold: i32,
    /// 判定一次"转向"所需的最小位移
    pub step: i32,
    /// 画轨迹
    pub show_trail: bool,
    /// 轨迹颜色（#RRGGBB）
    pub trail_color: String,
    /// 前台窗口全屏时自动暂停（游戏、视频、演示、远程桌面）
    pub pause_fullscreen: bool,
    /// 把触发键的"按下"也吞掉（推荐）。
    /// 关掉会退回"只吞抬起"的老做法 —— 万一补发点击在你的机器上不灵，
    /// 切到这里至少右键还在，代价是某些程序里会有状态残留。
    pub swallow_down: bool,
    /// 不响应手势的程序（exe 文件名，小写）
    pub excluded: Vec<String>,
    /// 在这个程序里划一下就能把它加进排除列表（默认 Z 形 →←→）
    pub exclude_pattern: String,
    pub bindings: Vec<GestureBinding>,
    /// 程序专用手势集（命中进程时优先，其余回落通用）
    pub profiles: Vec<GestureProfile>,
}

impl Default for GestureConfig {
    fn default() -> Self {
        Self {
            // 默认开着：按住右键划一下就是手势，普通右键点击（不移动）完全不受影响。
            // 关掉的开关在手势页，一眼能看到。
            enabled: true,
            trigger: "right".into(),
            // 这两个值是"手感"的全部。30/55 是上一版默认，实测太迟钝：
            // 用户划一小段完全没反应（以为坏了），短促手势划不出方向。
            // 16/38 对齐 MouseInc / StrokesPlus 一类成熟工具的量级。
            threshold: 16,
            step: 38,
            show_trail: true,
            trail_color: "#6C5CE7".into(),
            // 默认开：全屏多半是在玩游戏/看视频/演示，手势在那时候只会帮倒忙
            pause_fullscreen: true,
            swallow_down: true,
            excluded: Vec::new(),
            // 横、横回来、横 —— 手写 Z 的简化
            exclude_pattern: "R,DL,R".into(),
            bindings: default_gesture_bindings(),
            profiles: default_profiles(),
        }
    }
}

/// 通用设置。
///
/// 只放**真的被代码读过**的字段。以前这里有 `language` / `theme` /
/// `minimize_to_tray` / `show_notifications` 四个字段：没有任何实现，
/// 连界面都没有，却每次写进 settings.json —— 看配置文件的人会以为
/// 这些功能存在。要加就等真做的时候一起加。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct GeneralConfig {
    pub launch_at_login: bool,
    pub enable_plugins: bool,
    /// 关闭主窗口时是否真的退出
    pub close_to_tray: bool,
    /// 启动后静默检查一次新版本。
    ///
    /// 默认开：这是个长期装在托盘里的工具，用户不会主动来看"有没有更新"。
    /// 不想联网的人可以关掉 —— 关了就连那一次 API 请求都不发。
    pub check_updates: bool,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            launch_at_login: false,
            enable_plugins: true,
            close_to_tray: true,
            check_updates: true,
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
    if cfg.version < 3 {
        // 手势灵敏度旧默认（30/55）实测太迟钝：划一小段完全没反应，
        // 短促手势划不出方向 —— 用户只会认为"这手势不灵"。
        // 两个值都还是旧默认说明从没调过，直接升；只调过其中一个的不动，
        // 那是他自己选的口味。
        if cfg.gestures.threshold == 30 && cfg.gestures.step == 55 {
            cfg.gestures.threshold = 16;
            cfg.gestures.step = 38;
            log::info!("配置迁移到 v3：手势灵敏度 30/55 → 16/38");
        }
        cfg.version = 3;
    }
    if cfg.version < 4 {
        // 形状改成 `@` 前缀。原因：字母 D/L/R/U 和方向 D/L/R/U 同名，
        // 不隔离命名空间就没法同时支持"向下划"和"画一个 D"。
        // 老配置里的 "S" / "H" 都是形状，加前缀即可；方向串（UDLR 组合）不动。
        fn fix(list: &mut [GestureBinding]) {
            for b in list.iter_mut() {
                let p = b.pattern.trim().to_ascii_uppercase();
                if p.len() == 1 && !matches!(p.as_str(), "U" | "D" | "L" | "R") {
                    b.pattern = format!("@{p}");
                }
            }
        }
        fix(&mut cfg.gestures.bindings);
        for prof in &mut cfg.gestures.profiles {
            fix(&mut prof.bindings);
        }
        cfg.version = 4;
        log::info!("配置迁移到 v4：形状手势加 @ 前缀（S → @S、H → @H）");
    }
    if cfg.version < 5 {
        // 手势引擎升级到 8 方向：旧绑定在新语义下意思全变了
        // （旧的 `DR` 是"先下后右"，新的 `DR` 是"一条右下斜线"）。
        // 没有可靠的自动转换 —— "下再右"应该写成 `D,R`，但"一条右下斜线"
        // 在旧体系里根本不存在，分不清用户当初想的是哪个。
        // 干脆整份换成新默认，用户自己再调。
        cfg.gestures.bindings = default_gesture_bindings();
        cfg.gestures.profiles = default_profiles();
        cfg.gestures.exclude_pattern = "R,DL,R".into();
        cfg.version = 5;
        log::info!("配置迁移到 v5：手势升级为 8 方向，绑定重置为默认");
    }
    if cfg.version < 6 {
        // 默认手势定稿：按用户给的这套（复制/粘贴/关窗/搜索/ping/贴图/OCR…）
        // 全部换成新表，旧的手工调整一并作废 —— 8 方向语义和之前完全不同。
        cfg.gestures.bindings = default_gesture_bindings();
        cfg.gestures.profiles = default_profiles();
        cfg.gestures.exclude_pattern = "R,DL,R".into();
        cfg.version = 6;
        log::info!("配置迁移到 v6：默认手势换成 8 方向定稿版");
    }
    if cfg.version < 7 {
        // 单动作 → 动作序列（复合动作）。老字段 `action` 搬进 `actions`，
        // 用户已经调好的绑定一条都不丢。
        fn fix(list: &mut [GestureBinding]) {
            for b in list.iter_mut() {
                if b.actions.is_empty() {
                    if let Some(a) = b.action.take() {
                        b.actions = vec![a];
                    }
                }
                b.action = None;
            }
        }
        fix(&mut cfg.gestures.bindings);
        for prof in &mut cfg.gestures.profiles {
            fix(&mut prof.bindings);
        }
        cfg.version = 7;
        log::info!("配置迁移到 v7：手势支持复合动作（动作序列）");
    }
}

impl AppSettings {
    /// 局部更新：把 JSON patch 合并进当前配置。
    /// 手写深度合并而不是换整份，这样前端可以只提交改动的字段。
    ///
    /// 合并后必须能反序列化回完整配置 —— 失败就**原样回滚并报错**。
    /// 以前这里是 `if let Ok(next) = ...`，类型不对时安静地什么都不做，
    /// 前端拿到旧配置却以为保存成功（"改了没反应"最难查的一种）。
    pub fn apply_patch(&mut self, patch: &serde_json::Value) -> Result<(), String> {
        let mut base = serde_json::to_value(&*self).map_err(|e| format!("配置序列化失败: {e}"))?;
        deep_merge(&mut base, patch);
        let next: AppSettings = serde_json::from_value(base)
            .map_err(|e| format!("配置值不合法：{e}"))?;
        *self = next;
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_merges_only_given_fields() {
        let mut cfg = AppSettings::default();
        let before_items = cfg.clipboard.max_items;

        cfg.apply_patch(&serde_json::json!({ "clipboard": { "captureImages": false } }))
            .expect("合法 patch 应当成功");

        assert!(!cfg.clipboard.capture_images);
        assert_eq!(cfg.clipboard.max_items, before_items, "没提交的字段不该被动");
    }

    /// 类型不对时必须**报错并且不留半截状态**。
    ///
    /// 以前这里是 `if let Ok(next) = ...`，失败就静默丢弃 ——
    /// 前端拿到旧配置却以为保存成功，用户看到的是"改了没反应"。
    #[test]
    fn patch_rejects_bad_type_without_side_effect() {
        let mut cfg = AppSettings::default();
        let before = cfg.clipboard.max_items;

        let err = cfg.apply_patch(&serde_json::json!({
            "clipboard": { "maxItems": "这不是数字" }
        }));
        assert!(err.is_err(), "类型不对必须报错");
        assert_eq!(cfg.clipboard.max_items, before, "失败后配置必须保持原样");
    }

    /// 未知字段不报错（老版本配置里有、新版本删掉的字段要能容忍）
    #[test]
    fn patch_tolerates_unknown_fields() {
        let mut cfg = AppSettings::default();
        assert!(cfg
            .apply_patch(&serde_json::json!({ "general": { "theme": "dark" } }))
            .is_ok());
    }
}
