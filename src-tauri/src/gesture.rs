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
// 匹配规则住在 core 里：纯函数、能跑单测（主 crate 的 test harness 在本机加载不起来）
use snipforge_core::pattern::{find_in, normalize, pick};

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

/// 方向串 → 箭头。屏幕浮标和页面里共用同一套符号。
///
/// token 用逗号分隔，显示成 `↓ →`；单条斜线是 `↘`。
fn arrows(pattern: &str) -> String {
    pattern
        .split(',')
        .filter(|t| !t.is_empty())
        .map(|t| match t {
            "U" => "↑",
            "D" => "↓",
            "L" => "←",
            "R" => "→",
            "UL" => "↖",
            "UR" => "↗",
            "DL" => "↙",
            "DR" => "↘",
            other => other,
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// 这一划该执行什么：**先用当前程序的专用集，没命中再回落通用**。
///
/// 手势执行和页面"试划"共用同一套查找 —— 两边各写一套，
/// 早晚出现"这里试得出来、真划没反应"。
pub fn match_action(dir: &str) -> Option<Vec<GestureAction>> {
    // 先把专用集取出来（内部会拿一次读锁），再拿通用集的锁，避免重入
    let profile = active_profile();
    let g = cfg_lock().read();
    let p_hit = profile.as_ref().and_then(|p| find_in(&p.bindings, dir));
    let g_hit = find_in(&g.bindings, dir);
    match pick(p_hit, g_hit)? {
        (true, i) => profile.as_ref().map(|p| p.bindings[i].steps()),
        (false, i) => Some(g.bindings[i].steps()),
    }
}

/// 页面查重用：在指定范围（某个专用集 / 通用）里找命中的下标。
pub fn match_scoped(scope: Option<&str>, dir: &str) -> Option<usize> {
    let g = cfg_lock().read();
    match scope {
        Some(id) => {
            let p = g.profiles.iter().find(|p| p.id == id)?;
            find_in(&p.bindings, dir).map(|(_, i)| i)
        }
        None => find_in(&g.bindings, dir).map(|(_, i)| i),
    }
}

/// 点到线段的垂距（RDP 用）
fn perp(p: (i32, i32), a: (i32, i32), b: (i32, i32)) -> f32 {
    let (dx, dy) = ((b.0 - a.0) as f32, (b.1 - a.1) as f32);
    let len = (dx * dx + dy * dy).sqrt();
    if len < 1e-3 {
        let (px, py) = ((p.0 - a.0) as f32, (p.1 - a.1) as f32);
        return (px * px + py * py).sqrt();
    }
    let (px, py) = ((p.0 - a.0) as f32, (p.1 - a.1) as f32);
    ((px * dy - py * dx) / len).abs()
}

/// Ramer–Douglas–Peucker 折线简化：只保留真正的拐点。
fn rdp(pts: &[(i32, i32)], eps: f32) -> Vec<(i32, i32)> {
    let n = pts.len();
    if n < 3 {
        return pts.to_vec();
    }
    let mut keep = vec![false; n];
    keep[0] = true;
    keep[n - 1] = true;
    let mut stack = vec![(0usize, n - 1)];
    while let Some((a, b)) = stack.pop() {
        if b <= a + 1 {
            continue;
        }
        let mut best = (a, 0.0f32);
        for i in a + 1..b {
            let d = perp(pts[i], pts[a], pts[b]);
            if d > best.1 {
                best = (i, d);
            }
        }
        if best.1 > eps {
            keep[best.0] = true;
            stack.push((a, best.0));
            stack.push((best.0, b));
        }
    }
    (0..n).filter(|&i| keep[i]).map(|i| pts[i]).collect()
}

/// 轨迹 → 方向串。
///
/// **不用逐点判方向**：手画的折线在拐角处会来回抖，逐点判定会把一段切成一堆
/// 废 token（用户画一个三段折线，浮标显示 `↙,↓,→,↓,↙` 五个）。
/// 改成先做折线简化（RDP），只留真正的拐点，再对每段算方向 ——
/// 段数就等于"用户画了几笔"。
pub fn path_to_dir(pts: &[(i32, i32)]) -> String {
    if pts.len() < 3 {
        return String::new();
    }
    let xs: Vec<i32> = pts.iter().map(|p| p.0).collect();
    let ys: Vec<i32> = pts.iter().map(|p| p.1).collect();
    let span = (xs.iter().max().unwrap() - xs.iter().min().unwrap())
        .max(ys.iter().max().unwrap() - ys.iter().min().unwrap());
    // 容差跟着手势尺度走：画得大就容忍大一点的抖动
    let eps = (span as f32 * 0.055).clamp(6.0, 22.0);
    let key = rdp(pts, eps);

    let mut toks: Vec<&'static str> = Vec::new();
    for pair in key.windows(2) {
        let dx = pair[1].0 - pair[0].0;
        let dy = pair[1].1 - pair[0].1;
        let len = ((dx * dx + dy * dy) as f32).sqrt();
        // 太短的段丢掉：RDP 之后还可能留下五六个像素的碎段
        if len < 22.0 {
            continue;
        }
        let d = crate::win32::mousehook::dir8(dx, dy);
        if toks.last() != Some(&d) {
            toks.push(d);
        }
    }
    toks.join(",")
}

/// 录制框用：和真手势同一套算法
pub fn trace_dir(pts: &[(i32, i32)], _step: i32) -> String {
    path_to_dir(pts)
}

/// 进程名比较：忽略大小写和 `.exe` 后缀（用户填 `chrome` 也得认）
///
/// 实现统一放在 `win32::window::exe_name_eq`，剪贴板的"排除程序"用同一份，
/// 免得两处规则漂移。
fn same_exe(a: &str, b: &str) -> bool {
    crate::win32::window::exe_name_eq(a, b)
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
        // 方向一变就更新浮标：让用户在**划的过程中**就知道会执行什么，
        // 而不是划完才发现"啊，识别错了一个方向"。
        Event::Turn { dir } => {
            if show_trail {
                // 排除手势不是"绑定"，`describe_pattern` 查不到 —— 单独认一下，
                // 否则画对了 Z 也显示「未绑定」，用户会以为手势画错了。
                let exclude = normalize(&cfg_lock().read().exclude_pattern);
                let text = if !exclude.is_empty() && dir == exclude {
                    format!("{}  排除当前程序", arrows(&dir))
                } else {
                    match describe_pattern(&dir) {
                        Some(d) => format!("{}  {d}", arrows(&dir)),
                        None => format!("{}  未绑定", arrows(&dir)),
                    }
                };
                crate::win32::trail::set_label(text);
            }
        }
        Event::Cancel => {
            crate::win32::trail::end();
        }
        Event::End { dir } => {
            crate::win32::trail::end();
            log::debug!("手势方向串 {dir}");

            if let Some(a) = match_action(&dir) {
                run(app, a);
                return;
            }

            // 没绑动作：看看是不是"画一下排除当前程序"
            let exclude = normalize(&cfg_lock().read().exclude_pattern);
            if !exclude.is_empty() && dir == exclude {
                ask_exclude(app);
            }
        }
    }
}

/// 日志和浮标用的一句话描述
fn describe(a: &GestureAction) -> String {
    match a {
        GestureAction::Keys { keys } => format!("按键 {keys}"),
        GestureAction::Window { op } => format!(
            "窗口{}",
            match op.as_str() {
                "close" => "关闭",
                "close-similar" => "关闭同程序其他窗口",
                "toggle-max" => "最大化/还原",
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

/// 方向串对应的一句话描述，没绑定就是 None。
fn describe_pattern(dir: &str) -> Option<String> {
    match_action(dir).map(|steps| {
        steps
            .iter()
            .map(describe)
            .collect::<Vec<_>>()
            .join(" → ")
    })
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
/// 240ms —— 不等它藏起来，用户刚画的那道 S 就会出现在截图里。
/// 哪些算"抓屏幕"写在 `features::FEATURES` 里，不在这重复一遍。
fn needs_clean_screen(a: &GestureAction) -> bool {
    match a {
        GestureAction::Snip { id } => crate::features::grabs_screen(id),
        _ => false,
    }
}

/// 复合动作之间的间隔。
///
/// 有些动作要等前一个生效：`Ctrl+C` 之后立刻粘贴，粘到的可能是旧内容。
/// 120ms 覆盖绝大多数串键场景，又不会让"连做三件事"明显变慢。
const COMPOUND_GAP: u64 = 120;

/// 执行一个手势的动作序列。
///
/// 必须 spawn 到别的线程：钩子回调里不能做重活（超时会被系统摘钩），
/// 而"打开程序""发一串按键"都可能阻塞几十毫秒。
fn run(app: &AppHandle, steps: Vec<GestureAction>) {
    if steps.is_empty() {
        return;
    }
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        for (i, action) in steps.iter().enumerate() {
            if i > 0 {
                std::thread::sleep(std::time::Duration::from_millis(COMPOUND_GAP));
            }
            // 抓屏幕的动作要等轨迹层收干净，否则刚画的那一笔会被截进图里
            if needs_clean_screen(action) {
                crate::win32::trail::hide_blocking(150);
            }
            if let Err(e) = run_one(&handle, action) {
                log::warn!("手势动作失败: {e}");
                // 失败就停：后面的动作多半依赖前一步的结果
                let _ = handle.emit_to(
                    "main",
                    "app://toast",
                    serde_json::json!({ "type": "error", "message": format!("手势失败：{e}") }),
                );
                break;
            }
        }
    });
}

fn run_one(handle: &AppHandle, action: &GestureAction) -> Result<(), String> {
    match action {
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
            let delta = if dir == "down" { -step } else { step };
            crate::win32::input::wheel(delta)
        }
        GestureAction::Snip { id } => {
            if crate::features::run(handle, id) {
                Ok(())
            } else {
                Err(format!("未知功能：{id}"))
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 串的解析与匹配规则在 `snipforge_core::pattern`（那边能跑单测），
    /// 这里只验 fe 侧共用的箭头显示。
    #[test]
    fn arrows_render_eight_directions() {
        assert_eq!(arrows("D,R"), "↓ →");
        assert_eq!(arrows("DR"), "↘");
        assert_eq!(arrows("DR,UR"), "↘ ↗");
        assert_eq!(arrows("UL,UR,DL"), "↖ ↗ ↙");
    }
}


#[cfg(test)]
mod profile_tests {
    use super::*;

    /// 专用集和通用集必须是两套独立的表
    #[test]
    fn scoped_lookup_is_separate() {
        assert_eq!(match_scoped(Some("browser"), "D,L"), Some(8));
        assert_eq!(match_scoped(None, "D,L"), None);
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
