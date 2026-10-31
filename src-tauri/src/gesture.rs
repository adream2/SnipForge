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
/// 在一组绑定里找这一划命中的那一条。
///
/// 顺序：先比自定义笔画（阈值 `CUSTOM_MATCH` 很紧，只认几乎一样的），
/// 再比方向串。`#n` 是钩子给的"疑似自定义手势"提示，这里不当串处理。
fn find_in(
    bindings: &[crate::config::GestureBinding],
    stroke: &[(i32, i32)],
    pattern: &str,
) -> Option<usize> {
    let path = crate::win32::shape::canonical_stroke(stroke);
    if path.len() >= 8 {
        let mut best: Option<(f32, usize)> = None;
        for (i, b) in bindings.iter().enumerate() {
            if !b.enabled || b.points.len() != path.len() {
                continue;
            }
            let d = crate::win32::shape::stroke_distance(&path, &b.points);
            if best.map_or(true, |(bd, _)| d < bd) {
                best = Some((d, i));
            }
        }
        if let Some((d, i)) = best {
            if d <= crate::win32::shape::CUSTOM_MATCH {
                return Some(i);
            }
        }
    }
    if pattern.starts_with('#') {
        return None;
    }
    bindings
        .iter()
        .position(|b| b.enabled && normalize(&b.pattern) == pattern)
}

/// 这一划该执行什么：**先用当前程序的专用集，没命中再回落通用**。
///
/// 手势执行和页面"试划"共用同一套查找 —— 两边各写一套，
/// 早晚出现"这里试得出来、真划没反应"。
pub fn match_action(stroke: &[(i32, i32)], pattern: &str) -> Option<GestureAction> {
    if let Some(p) = active_profile() {
        if let Some(i) = find_in(&p.bindings, stroke, pattern) {
            return Some(p.bindings[i].action.clone());
        }
    }
    let g = cfg_lock().read();
    find_in(&g.bindings, stroke, pattern).map(|i| g.bindings[i].action.clone())
}

/// 页面查重用：在指定范围（某个专用集 / 通用）里找命中的下标。
pub fn match_scoped(scope: Option<&str>, stroke: &[(i32, i32)], pattern: &str) -> Option<usize> {
    let g = cfg_lock().read();
    match scope {
        Some(id) => {
            let p = g.profiles.iter().find(|p| p.id == id)?;
            find_in(&p.bindings, stroke, pattern)
        }
        None => find_in(&g.bindings, stroke, pattern),
    }
}

fn lookup(pattern: &str) -> Option<GestureAction> {
    // 专用集优先：同一个 `DR`，在浏览器里是关标签，在桌面别处可能是别的
    if let Some(p) = active_profile() {
        if let Some(b) = p
            .bindings
            .iter()
            .find(|b| b.enabled && normalize(&b.pattern) == pattern)
        {
            return Some(b.action.clone());
        }
    }
    let g = cfg_lock().read();
    g.bindings
        .iter()
        .find(|b| b.enabled && normalize(&b.pattern) == pattern)
        .map(|b| b.action.clone())
}

/// 进程名比较：忽略大小写和 `.exe` 后缀（用户填 `chrome` 也得认）
fn same_exe(a: &str, b: &str) -> bool {
    // 先去空白再转小写，最后才剥后缀 —— 先剥的话 "Chrome.EXE" 剥不掉，
    // trim_end_matches 是大小写敏感的
    fn stem(s: &str) -> String {
        let t = s.trim().to_ascii_lowercase();
        t.strip_suffix(".exe").unwrap_or(&t).to_string()
    }
    let a = stem(a);
    !a.is_empty() && a == stem(b)
}

/// 当前前台程序命中的专用手势集（多个命中取第一个）。
fn active_profile() -> Option<crate::config::GestureProfile> {
    let exe = crate::win32::window::foreground_exe_cached();
    if exe.is_empty() {
        return None;
    }
    let g = cfg_lock().read();
    g.profiles
        .iter()
        .find(|p| p.exes.iter().any(|e| same_exe(e, &exe)))
        .cloned()
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
                let custom = pattern.starts_with('#');
                let text = match describe_pattern(&pattern) {
                    // 自定义手势没有箭头可画，用 ✍ 标明"这是你画的那个形状"
                    Some(d) if custom => format!("✍ {d}"),
                    Some(d) => format!("{}  {d}", arrows(&pattern)),
                    None if custom => "✍ 自定义手势".to_string(),
                    None => format!("{}  未绑定", arrows(&pattern)),
                };
                crate::win32::trail::set_label(text);
            }
        }
        Event::Cancel => {
            crate::win32::trail::end();
        }
        Event::End { pattern, stroke } => {
            crate::win32::trail::end();
            log::debug!("手势 {pattern}");

            // 以落笔的笔画为准重新匹配一次：钩子里那个 #下标 只用于浮标提示，
            // 配置若在手势中途改过可能已经错位。
            if let Some(a) = match_action(&stroke, &pattern) {
                run(app, a);
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
        // 名字统一由注册表给：同一个功能在浮标、托盘、下拉里必须是同一个叫法
        GestureAction::Snip { id } => crate::features::label(id).to_string(),
    }
}

/// 一条"手势标识"对应的一句话描述。
///
/// 方向串/形状字符走 `lookup`；`#3` 是钩子实时认出的自定义手势（bindings 下标）——
/// 用它只是为了让浮标能当场显示"这一划会干什么"，真正执行时再按笔画重算。
fn describe_pattern(pattern: &str) -> Option<String> {
    if let Some(idx) = pattern
        .strip_prefix('#')
        .and_then(|n| n.parse::<usize>().ok())
    {
        let g = cfg_lock().read();
        return g.bindings.get(idx).map(|b| describe(&b.action));
    }
    lookup(pattern).map(|a| describe(&a))
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

/// 要不要在动手前先把轨迹层收干净。
///
/// 只有"抓屏幕"的动作才需要：截图/录屏会把屏幕内容抓走，而轨迹层的淡出是
/// 520ms —— 不等它藏起来，用户刚画的那道 S 就会出现在截图里。
/// 哪些算"抓屏幕"写在 `features::FEATURES` 里，不在这重复一遍。
fn needs_clean_screen(a: &GestureAction) -> bool {
    match a {
        GestureAction::Snip { id } => crate::features::grabs_screen(id),
        _ => false,
    }
}

/// 执行动作。
///
/// 必须 spawn 到别的线程：钩子回调里不能做重活（超时会被系统摘钩），
/// 而"打开程序""发一串按键"都可能阻塞几十毫秒。
fn run(app: &AppHandle, action: GestureAction) {
    let handle = app.clone();
    let clean_first = needs_clean_screen(&action);
    tauri::async_runtime::spawn_blocking(move || {
        if clean_first {
            // 同步等窗口真的隐藏 —— 这一步在阻塞线程里做，不会卡住钩子
            crate::win32::trail::hide_blocking(150);
        }
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
                // 分发也走注册表：加功能不用回来改这里
                if !crate::features::run(&handle, id) {
                    Err(format!("未知功能：{id}"))
                } else {
                    Ok(())
                }
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

#[cfg(test)]
mod profile_tests {
    use super::*;

    /// 专用集和通用集必须是两套独立的表：
    /// `UL` 只在浏览器集里有（切左侧标签），`UDR` 只在通用里有（静音）。
    #[test]
    fn scoped_lookup_is_separate() {
        assert_eq!(match_scoped(Some("browser"), &[], "UL"), Some(8));
        assert_eq!(match_scoped(None, &[], "UL"), None);
        assert_eq!(match_scoped(None, &[], "UDR"), Some(12));
        assert_eq!(match_scoped(Some("browser"), &[], "UDR"), None);
        // 不存在的专用集不能炸
        assert_eq!(match_scoped(Some("nope"), &[], "UL"), None);
    }

    #[test]
    fn exe_name_matching() {
        assert!(same_exe("chrome", "chrome.exe"));
        assert!(same_exe("Chrome.EXE", "chrome"));
        assert!(same_exe(" msedge ", "MSEDGE.exe"));
        assert!(!same_exe("", "chrome"));
        assert!(!same_exe("chrome", "firefox"));
    }
}
