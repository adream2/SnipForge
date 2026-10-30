//! 鼠标手势 —— 一个通用的手势工具，不只是本软件的快捷键。
//!
//! 分层：
//!   - `win32/mousehook`：监听触发键，把位移翻译成方向串（U/D/L/R）+ 轨迹点
//!   - `win32/trail`：把轨迹画出来
//!   - `win32/input` `win32/window`：真正执行动作（发键、开程序、窗口操作）
//!   - 这里：读配置、匹配方向串、分发动作
//!
//! 为什么用"方向串"而不是模板匹配 / 概率识别：
//!   用户要的是"记住我划的方向"。四个方向、精确相等匹配，行为完全可预测 ——
//!   划错了就是没反应，而不是"有时候执行错动作"。配置也能用箭头直接显示。

use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::OnceLock;

use parking_lot::RwLock;
use tauri::{AppHandle, Emitter};

use crate::config::{GestureAction, GestureConfig};
use crate::win32::mousehook::{self, Event};

static CFG: OnceLock<RwLock<GestureConfig>> = OnceLock::new();

/// 手势开始时的前台窗口。
///
/// 动作用异步执行，那时 `GetForegroundWindow()` 可能已经变成 NULL
/// （窗口激活的瞬间就是这样）。窗口类动作要用"用户划手势时看着的那个窗口"，
/// 所以在这里先记一份。
static BEGIN_HWND: AtomicIsize = AtomicIsize::new(0);

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
        Event::Cancel => {
            crate::win32::trail::end();
        }
        Event::End { pattern } => {
            crate::win32::trail::end();

            let action = {
                let g = cfg_lock().read();
                g.bindings
                    .iter()
                    .find(|b| b.enabled && normalize(&b.pattern) == pattern)
                    .map(|b| b.action.clone())
            };
            match action {
                Some(a) => {
                    log::debug!("手势 {pattern} → {}", describe(&a));
                    run(app, a);
                }
                None => log::debug!("手势 {pattern} 没有绑定"),
            }
        }
    }
}

/// 配置里手写的方向串统一成大写，`ur` 和 `UR` 是一回事
fn normalize(pattern: &str) -> String {
    pattern
        .chars()
        .filter_map(|c| match c.to_ascii_uppercase() {
            'U' | 'D' | 'L' | 'R' => Some(c.to_ascii_uppercase()),
            _ => None,
        })
        .collect()
}

/// 日志用的一句话描述
fn describe(a: &GestureAction) -> String {
    match a {
        GestureAction::Keys { keys } => format!("按键 {keys}"),
        GestureAction::Window { op } => format!("窗口 {op}"),
        GestureAction::App { path } => format!("打开 {path}"),
        GestureAction::Url { url } => format!("打开网址 {url}"),
        GestureAction::Cmd { cmd } => format!("运行 {cmd}"),
        GestureAction::Media { key } => format!("媒体键 {key}"),
        GestureAction::Text { text } => format!("输入文字（{} 字）", text.chars().count()),
        GestureAction::Scroll { dir, amount } => format!("滚动 {dir} x{amount}"),
        GestureAction::Snip { id } => format!("SnipForge {id}"),
    }
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
                // 手势里"上划"= 内容往上走 = 滚轮向上（正数）
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

/// 钩子是否装上了（页面显示状态用）
pub fn is_active() -> bool {
    mousehook::is_installed() && cfg_lock().read().enabled
}

/// 当前配置快照（给命令层用）
pub fn snapshot() -> GestureConfig {
    cfg_lock().read().clone()
}
