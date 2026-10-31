//! 鼠标手势 —— 一个通用的手势工具，不只是本软件的快捷键。
//!
//! 分层：
//!   - `win32/mousehook`：监听触发键，把位移翻译成方向串（U/D/L/R）+ 轨迹点
//!   - `win32/trail`：把轨迹和"识别成了什么"画在屏幕上
//!   - `win32/input` `win32/window`：真正执行动作（发键、开程序、窗口操作）
//!   - 这里：读配置、匹配方向串、分发动作、处理"画一下排除这个程序"
//!
//! 手势有两种表示：
//!   - **方向串**（`"DR"`）：折线手势，精确相等匹配。四个方向、行为完全可预测 ——
//!     划错了就是没反应，而不是"有时候执行错动作"。配置里能直接用箭头显示。
//!   - **形状**（`"S"`）：曲线手势。方向串表达不了 S，拿 `DLD` 硬凑等于让用户
//!     把 S 画成折线，手感全错。所以这类手势在钩子里由 `win32::shape` 认，
//!     认出来就是一个形状字符，和方向串一样参与匹配。

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::OnceLock;

use parking_lot::RwLock;
use tauri::{AppHandle, Emitter, Manager};

use crate::config::{GestureAction, GestureConfig};
use crate::win32::mousehook::{self, Event};

static CFG: OnceLock<RwLock<GestureConfig>> = OnceLock::new();

/// 手势开始时的前台窗口。
///
/// 动作用异步执行，那时 `GetForegroundWindow()` 可能已经变成 NULL
/// （窗口激活的瞬间就是这样）。窗口类动作要用"用户划手势时看着的那个窗口"，
/// 所以在这里先记一份。
static BEGIN_HWND: AtomicIsize = AtomicIsize::new(0);

/// 排除确认框正在弹 → 别重复弹
static ASKING_EXCLUDE: AtomicBool = AtomicBool::new(false);

fn cfg_lock() -> &'static RwLock<GestureConfig> {
    CFG.get_or_init(|| RwLock::new(GestureConfig::default()))
}

/// 安装手势（幂等）。
pub fn install(app: &AppHandle, cfg: GestureConfig) {
    *cfg_lock().write() = cfg.clone();
    mousehook::update_config(cfg);

    // 轨迹层提前建好：第一次划手势时再建窗口会有肉眼可见的延迟
    crate::win32::trail::ensure();

    let handle = app.clone();
    mousehook::install(move |ev| handle_event(&handle, ev));
}

/// 配置变更。钩子仍然挂着，只是换成新规则。
pub fn update(cfg: GestureConfig) {
    let color = cfg.trail_color.clone();
    let show = cfg.show_trail;
    *cfg_lock().write() = cfg.clone();
    mousehook::update_config(cfg);
    crate::win32::trail::set_style(show, &color);
}

/// 方向串 → 箭头。屏幕浮标和页面里用同一套符号。
/// 形状手势（`S`）不是方向，原样显示。
fn arrows(pattern: &str) -> String {
    pattern
        .chars()
        .map(|c| match c {
            'U' => '↑',
            'D' => '↓',
            'L' => '←',
            'R' => '→',
            other => other,
        })
        .collect()
}

/// 这条方向串现在会干什么？没有绑定就返回 None。
fn lookup(pattern: &str) -> Option<GestureAction> {
    let g = cfg_lock().read();
    g.bindings
        .iter()
        .find(|b| b.enabled && normalize(&b.pattern) == pattern)
        .map(|b| b.action.clone())
}

fn handle_event(app: &AppHandle, ev: Event) {
    let (show_trail, color) = {
        let g = cfg_lock().read();
        (g.show_trail, g.trail_color.clone())
    };
    match ev {
        Event::Begin { x, y } => {
            BEGIN_HWND.store(crate::win32::focus::foreground_hwnd(), Ordering::SeqCst);
            if show_trail {
                crate::win32::trail::begin_with(&color);
                crate::win32::trail::push(x, y);
            }
        }
        Event::Move { x, y } => {
            if show_trail {
                crate::win32::trail::push(x, y);
            }
        }
        // 方向一变就更新浮标：让用户在**划的过程中**就知道会执行什么，
        // 而不是划完才发现"啊，识别错了一个方向"。
        Event::Turn { pattern } => {
            if show_trail {
                let text = match lookup(&pattern) {
                    Some(a) => format!("{}  {}", arrows(&pattern), describe(&a)),
                    None => format!("{}  未绑定", arrows(&pattern)),
                };
                crate::win32::trail::set_label(text);
            }
        }
        Event::Cancel => {
            crate::win32::trail::end();
        }
        Event::End { pattern } => {
            crate::win32::trail::end();
            log::debug!("手势 {pattern}");

            if let Some(action) = lookup(&pattern) {
                run(app, action);
                return;
            }

            // 没绑动作：看看是不是"画一下排除当前程序"
            let exclude_pattern = normalize(&cfg_lock().read().exclude_pattern);
            if !exclude_pattern.is_empty() && pattern == exclude_pattern {
                ask_exclude(app);
            }
        }
    }
}

/// 配置里手写的方向串统一成大写，`ur` 和 `UR` 是一回事。
///
/// 形状字符（目前只有 `S`）不是方向，但必须留下 —— 否则用户写的 `"S"`
/// 会被过滤成空串，整条绑定凭空失效。
fn normalize(pattern: &str) -> String {
    pattern
        .chars()
        .filter_map(|c| match c.to_ascii_uppercase() {
            'U' | 'D' | 'L' | 'R' | 'S' => Some(c.to_ascii_uppercase()),
            _ => None,
        })
        .collect()
}

/// 日志和浮标用的一句话描述
fn describe(a: &GestureAction) -> String {
    match a {
        GestureAction::Keys { keys } => format!("按键 {keys}"),
        GestureAction::Window { op } => format!(
            "窗口{}",
            match op.as_str() {
                "close" => "关闭",
                "minimize" => "最小化",
                "maximize" => "最大化",
                "restore" => "还原",
                "topmost" => "置顶切换",
                other => other,
            }
        ),
        GestureAction::App { path } => format!("打开 {}", short(path)),
        GestureAction::Url { url } => format!("打开 {}", short(url)),
        GestureAction::Cmd { cmd } => format!("运行 {}", short(cmd)),
        GestureAction::Media { key } => format!("媒体键 {key}"),
        GestureAction::Text { text } => format!("输入 {} 字", text.chars().count()),
        GestureAction::Scroll { dir, amount } => {
            format!(
                "{}滚动 {amount} 格",
                if dir == "down" { "向下" } else { "向上" }
            )
        }
        GestureAction::Snip { id } => match id.as_str() {
            "region" => "截图".into(),
            "fullscreen" => "全屏截图".into(),
            "window" => "窗口截图".into(),
            "scroll" => "长截图".into(),
            "panel" => "剪贴板面板".into(),
            "pin" => "剪贴板贴图".into(),
            "record" => "录屏".into(),
            "record-gif" => "录 GIF".into(),
            "pin-region" => "框选贴图".into(),
            "ocr-region" => "框选取字".into(),
            other => other.into(),
        },
    }
}

/// 浮标空间有限，长的路径只留尾巴
fn short(s: &str) -> String {
    let s = s.trim();
    if s.chars().count() <= 22 {
        return s.to_string();
    }
    let tail: String = s
        .chars()
        .rev()
        .take(20)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    format!("…{tail}")
}

/// 执行动作。
///
/// 必须 spawn 到别的线程：钩子回调里不能做重活（超时会被系统摘钩），
/// 而"打开程序""发一串按键"都可能阻塞几十毫秒。
fn run(app: &AppHandle, action: GestureAction) {
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let result = match &action {
            GestureAction::Keys { keys } => crate::win32::input::send_keys(keys),
            GestureAction::Window { op } => {
                // 优先用当前前台窗口（用户可能中途切了窗口，那才符合直觉）；
                // 取不到就退回手势开始时的那个。
                let mut hwnd = crate::win32::focus::foreground_hwnd();
                if hwnd == 0 {
                    hwnd = BEGIN_HWND.load(Ordering::SeqCst);
                }
                crate::win32::window::act_on(hwnd, op)
            }
            GestureAction::App { path } => crate::win32::window::shell_open(path),
            GestureAction::Url { url } => crate::win32::window::shell_open(url),
            GestureAction::Cmd { cmd } => crate::win32::window::run_command(cmd),
            GestureAction::Media { key } => match key.as_str() {
                // 媒体键本质就是发一个虚拟键
                "vol-up" => crate::win32::input::send_keys("VolUp"),
                "vol-down" => crate::win32::input::send_keys("VolDown"),
                "mute" => crate::win32::input::send_keys("Mute"),
                "play" => crate::win32::input::send_keys("Play"),
                "next" => crate::win32::input::send_keys("Next"),
                "prev" => crate::win32::input::send_keys("Prev"),
                other => Err(format!("不认识的媒体键：{other}")),
            },
            GestureAction::Text { text } => crate::win32::input::send_text(text),
            GestureAction::Scroll { dir, amount } => {
                let step = crate::win32::input::WHEEL_STEP * amount.max(&1).min(&20);
                // "上划"= 内容往上走 = 滚轮向上（正数）
                let delta = if dir == "down" { -step } else { step };
                crate::win32::input::wheel(delta)
            }
            GestureAction::Snip { id } => {
                run_snip(&handle, id);
                Ok(())
            }
        };

        if let Err(e) = result {
            log::warn!("手势动作失败: {e}");
            // 手势失败必须让用户知道 —— 否则他只会觉得"这手势坏了"
            let _ = handle.emit_to(
                "main",
                "app://toast",
                serde_json::json!({ "type": "error", "message": format!("手势失败：{e}") }),
            );
        }
    });
}

/// 本工具自己的功能，和其他动作平级
fn run_snip(app: &AppHandle, id: &str) {
    match id {
        "region" => crate::commands::capture::trigger(app, "region"),
        "fullscreen" => crate::commands::capture::trigger(app, "fullscreen"),
        "window" => crate::commands::capture::trigger(app, "active-window"),
        "scroll" => crate::commands::capture::trigger(app, "scroll"),
        // 框选完直接贴到屏幕上 / 直接取字，不进编辑器
        "pin-region" => crate::commands::capture::trigger(app, "pin-region"),
        "ocr-region" => crate::commands::capture::trigger(app, "ocr-region"),
        "panel" => crate::panel::toggle(app),
        "record" => crate::commands::record::toggle(app, crate::commands::record::Fmt::Mp4),
        "record-gif" => crate::commands::record::toggle(app, crate::commands::record::Fmt::Gif),
        "pin" => {
            let h = app.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = crate::commands::clipboard::clipboard_pin_current(h).await {
                    log::warn!("手势贴图失败: {e}");
                }
            });
        }
        other => log::warn!("不认识的 SnipForge 动作：{other}"),
    }
}

/// "画一下排除这个程序"的确认流程。
///
/// 用系统 MessageBox 而不是自建窗口：这是个一次性、阻塞式的确认，
/// 弹在后台线程里不会卡界面，也不用为它多维护一个 webview。
fn ask_exclude(app: &AppHandle) {
    if ASKING_EXCLUDE.swap(true, Ordering::SeqCst) {
        return;
    }
    let exe = crate::win32::window::foreground_exe_cached();
    if exe.is_empty() {
        ASKING_EXCLUDE.store(false, Ordering::SeqCst);
        return;
    }
    let handle = app.clone();
    std::thread::spawn(move || {
        use windows::core::PCWSTR;
        use windows::Win32::UI::WindowsAndMessaging::{
            MessageBoxW, IDYES, MB_ICONQUESTION, MB_SETFOREGROUND, MB_TOPMOST, MB_YESNO,
        };

        let text = format!(
            "以后在「{exe}」里不响应鼠标手势？\n\n（随时可以在「鼠标手势」页的排除列表里删掉）"
        );
        let wide: Vec<u16> = text.encode_utf16().chain(std::iter::once(0)).collect();
        let title: Vec<u16> = "SnipForge 鼠标手势"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        let ret = unsafe {
            MessageBoxW(
                None,
                PCWSTR(wide.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_YESNO | MB_ICONQUESTION | MB_TOPMOST | MB_SETFOREGROUND,
            )
        };

        if ret == IDYES {
            if let Some(st) = handle.try_state::<std::sync::Arc<crate::state::AppState>>() {
                let state = st.inner().clone();
                {
                    let mut s = state.settings.write();
                    if !s
                        .gestures
                        .excluded
                        .iter()
                        .any(|e| e.eq_ignore_ascii_case(&exe))
                    {
                        s.gestures.excluded.push(exe.clone());
                    }
                }
                if let Err(e) = state.persist_settings() {
                    log::warn!("保存排除列表失败: {e}");
                } else {
                    log::info!("已排除程序: {exe}");
                }
                // 让钩子立刻用上新配置
                let cfg = state.settings.read().gestures.clone();
                update(cfg);
                let _ = handle.emit_to("main", "app://gesture-changed", serde_json::json!({}));
            }
        }
        ASKING_EXCLUDE.store(false, Ordering::SeqCst);
    });
}

/// 钩子是否装上了（页面显示状态用）
pub fn is_active() -> bool {
    mousehook::is_installed() && cfg_lock().read().enabled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shape_token_survives_normalize() {
        // 这条以前会挂：过滤器只放行 UDLR，"S" 被吃成空串，
        // 用户写的 S 绑定就等于没写。形状字符必须原样留下。
        assert_eq!(normalize("S"), "S");
        assert_eq!(normalize(" s "), "S");
        assert_eq!(normalize("DLD"), "DLD");
        assert_eq!(normalize("d l d"), "DLD");
        // 浮标和列表里的显示：方向是箭头，形状就是它自己
        assert_eq!(arrows("DR"), "↓→");
        assert_eq!(arrows("S"), "S");
    }
}
