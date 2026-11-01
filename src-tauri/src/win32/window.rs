//! 顶层窗口枚举与前台窗口识别。
//!
//! 两个关键细节，踩过的坑都在这：
//!   1. 必须先把进程设为 Per-Monitor DPI Aware，否则在 125%/150% 缩放下
//!      Windows 会给虚拟化后的逻辑坐标，跟 Tauri 的物理像素完全对不上。
//!   2. 用 DWMWA_EXTENDED_FRAME_BOUNDS 而不是 GetWindowRect 取边界 ——
//!      后者含窗口阴影，截窗口时会多出一圈透明边。

use crate::types::{NativeWindowInfo, Rect};
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS,
};
use windows::Win32::System::DataExchange::GetClipboardOwner;
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    ChildWindowFromPointEx, EnumWindows, GetClassNameW, GetClientRect, GetForegroundWindow,
    GetWindowLongW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic,
    IsWindowVisible, CWP_SKIPDISABLED, CWP_SKIPINVISIBLE, CWP_SKIPTRANSPARENT, GWL_EXSTYLE,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};
use windows::Win32::Graphics::Gdi::{ClientToScreen, ScreenToClient};

/// 进程级 DPI 感知。幂等，重复调用无害。
pub fn set_dpi_aware() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let list = &mut *(lparam.0 as *mut Vec<HWND>);
    list.push(hwnd);
    BOOL(1)
}

fn read_text(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let len = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

fn read_class(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let len = unsafe { GetClassNameW(hwnd, &mut buf) };
    if len <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..len as usize])
}

fn process_exe(pid: u32) -> String {
    if pid == 0 {
        return String::new();
    }
    unsafe {
        let handle = match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Ok(h) => h,
            Err(_) => return String::new(),
        };
        let mut buf = [0u16; 1024];
        let mut size = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut size,
        )
        .is_ok();
        let _ = CloseHandle(handle);
        if !ok {
            return String::new();
        }
        let full = String::from_utf16_lossy(&buf[..size as usize]);
        full.rsplit('\\')
            .next()
            .unwrap_or(&full)
            .trim_end_matches(".exe")
            .to_lowercase()
    }
}

/// 窗口的精确边界（不含阴影）。DWM 拿不到时退回 GetWindowRect。
fn window_bounds(hwnd: HWND) -> Option<Rect> {
    let mut rect = RECT::default();
    let mut ok = false;
    unsafe {
        if DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        )
        .is_ok()
        {
            ok = true;
        } else if GetWindowRect(hwnd, &mut rect).is_ok() {
            ok = true;
        }
    }
    if !ok {
        return None;
    }
    let width = (rect.right - rect.left).max(0) as u32;
    let height = (rect.bottom - rect.top).max(0) as u32;
    if width < 8 || height < 8 {
        return None;
    }
    Some(Rect {
        x: rect.left,
        y: rect.top,
        width,
        height,
    })
}

fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked: u32 = 0;
    unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut _ as *mut _,
            std::mem::size_of::<u32>() as u32,
        )
        .is_ok()
            && cloaked != 0
    }
}

fn describe(hwnd: HWND, self_pid: u32) -> Option<NativeWindowInfo> {
    let title = read_text(hwnd);
    if title.is_empty() {
        return None;
    }
    let class_name = read_class(hwnd);
    // 桌面 / 任务栏 / 输入法的壳层窗口不参与取窗
    if matches!(
        class_name.as_str(),
        "Progman" | "WorkerW" | "Shell_TrayWnd" | "Windows.UI.Core.CoreWindow" | "ForegroundStaging"
    ) {
        return None;
    }

    let mut pid: u32 = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
    }
    let exe = process_exe(pid);
    let rect = window_bounds(hwnd)?;

    Some(NativeWindowInfo {
        hwnd: hwnd.0 as i64,
        title,
        class_name,
        rect,
        exe: exe.clone(),
        ignored: pid == self_pid || exe == "snipforge",
    })
}

/// 枚举所有可截取的顶层窗口。坐标为物理像素，调用方按需换算。
pub fn list_top_level_windows(self_pid: u32) -> Vec<NativeWindowInfo> {
    set_dpi_aware();
    let mut hwnds: Vec<HWND> = Vec::with_capacity(128);
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut hwnds as *mut _ as isize));
    }

    let mut out = Vec::with_capacity(hwnds.len());
    for hwnd in hwnds {
        unsafe {
            if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
                continue;
            }
            let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
            // TOOLWINDOW / NOACTIVATE：工具浮窗、各种 HUD
            if style & WS_EX_TOOLWINDOW.0 != 0 || style & WS_EX_NOACTIVATE.0 != 0 {
                continue;
            }
            // TRANSPARENT：鼠标穿透的透明叠层（各种屏显、水印层）——
            // 用户看不见它，也就不该被它抢走命中，否则鼠标扫过去什么都不亮
            if style & WS_EX_TRANSPARENT.0 != 0 {
                continue;
            }
        }
        if is_cloaked(hwnd) {
            continue;
        }
        if let Some(info) = describe(hwnd, self_pid) {
            out.push(info);
        }
    }

    // **保持 EnumWindows 的 z 序（上层在前），不要按面积排。**
    //
    // 曾经为了"优先选后面的大窗口"按面积降序排过，结果命中测试永远选中
    // 某个铺满屏幕的背景窗口（最大化浏览器 / 设置页），鼠标移到小窗口上
    // 也高亮整屏 —— 用户的感受就是"根本识别不到窗口"。
    // 用户指的是光标底下**看得见**的那个窗口，那就是 z 序最上面那个。
    out
}

/* ---------------- 光标下的窗口 / 组件（区域截图的高亮） ---------------- */

/// 这个顶层窗口值不值得被"指着选"。
///
/// 和 `list_top_level_windows` 的筛法基本一致，但**不要求有标题**：
/// 悬停命中要找的是"看得见的那个"，而不是"能列进列表的那个"。
/// 便宜的初筛。每次鼠标移动都要跑一遍（几百个窗口），只做最基本的几个调用。
fn pickable_cheap(hwnd: HWND, self_pid: u32) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() {
            return false;
        }
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
        // 只比 pid：这里每次鼠标移动都会跑一遍，别为了一个 exe 名
        // 给每个窗口都 OpenProcess 一次
        if pid == self_pid {
            return false;
        }
        let style = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        // 鼠标穿透的叠层用户根本点不到，不该被它抢走命中
        if style & WS_EX_TRANSPARENT.0 != 0 {
            return false;
        }
    }
    true
}

/// 贵一点的筛法：DWM 是否被遮住、是不是桌面/任务栏壳层。
/// 只在"光标确实落在这个窗口里"之后才调 —— 几百个窗口里通常只剩一两个。
fn pickable_deep(hwnd: HWND) -> bool {
    if is_cloaked(hwnd) {
        return false;
    }
    let class_name = read_class(hwnd);
    !matches!(
        class_name.as_str(),
        "Progman"
            | "WorkerW"
            | "Shell_TrayWnd"
            | "Shell_SecondaryTrayWnd"
            | "Windows.UI.Core.CoreWindow"
            | "ForegroundStaging"
            | ""
    )
}

fn rect_has(r: &Rect, x: i32, y: i32) -> bool {
    x >= r.x && x < r.x + r.width as i32 && y >= r.y && y < r.y + r.height as i32
}

/// 从顶层窗口一层层往下钻到最深的子窗口。
///
/// 这就是"组件识别"：原生程序里它是按钮、编辑框、列表；Chromium/Electron
/// 里通常只剩一层渲染区，那也算一个合理的"内容区"。
/// `ChildWindowFromPointEx` 只查直接子窗口，所以要循环下钻。
fn deepest_child(root: HWND, x: i32, y: i32) -> HWND {
    let mut cur = root;
    for _ in 0..8 {
        let mut pt = POINT { x, y };
        let child = unsafe {
            if !ScreenToClient(cur, &mut pt).as_bool() {
                break;
            }
            ChildWindowFromPointEx(
                cur,
                pt,
                CWP_SKIPINVISIBLE | CWP_SKIPTRANSPARENT | CWP_SKIPDISABLED,
            )
        };
        if child.0.is_null() || child == cur {
            break;
        }
        cur = child;
    }
    cur
}

/// 子窗口的边界。优先 GetWindowRect，拿不到再退回客户区。
fn child_bounds(hwnd: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    unsafe {
        if GetWindowRect(hwnd, &mut r).is_ok() {
            let w = (r.right - r.left).max(0) as u32;
            let h = (r.bottom - r.top).max(0) as u32;
            if w >= 4 && h >= 4 {
                return Some(Rect {
                    x: r.left,
                    y: r.top,
                    width: w,
                    height: h,
                });
            }
        }
        if GetClientRect(hwnd, &mut r).is_ok() {
            let mut p0 = POINT { x: 0, y: 0 };
            let mut p1 = POINT {
                x: r.right,
                y: r.bottom,
            };
            if ClientToScreen(hwnd, &mut p0).as_bool() && ClientToScreen(hwnd, &mut p1).as_bool() {
                let w = (p1.x - p0.x).max(0) as u32;
                let h = (p1.y - p0.y).max(0) as u32;
                if w >= 4 && h >= 4 {
                    return Some(Rect {
                        x: p0.x,
                        y: p0.y,
                        width: w,
                        height: h,
                    });
                }
            }
        }
    }
    None
}

/// 光标底下的窗口 / 组件。`whole = true` 时只认整个窗口（按住 Ctrl）。
///
/// 为什么自己枚举 z 序，而不用 `WindowFromPoint`：
/// 选区覆盖层本身就是一块全屏置顶窗口，`WindowFromPoint` 永远返回它自己。
pub fn element_at(x: i32, y: i32, whole: bool) -> Option<crate::types::NativeElement> {
    set_dpi_aware();
    let mut hwnds: Vec<HWND> = Vec::with_capacity(128);
    unsafe {
        let _ = EnumWindows(Some(enum_proc), LPARAM(&mut hwnds as *mut _ as isize));
    }
    let self_pid = std::process::id();

    for hwnd in hwnds {
        if !pickable_cheap(hwnd, self_pid) {
            continue;
        }
        let Some(root_rect) = window_bounds(hwnd) else {
            continue;
        };
        if !rect_has(&root_rect, x, y) || !pickable_deep(hwnd) {
            continue;
        }

        let target = if whole { hwnd } else { deepest_child(hwnd, x, y) };
        let is_root = target == hwnd;
        let rect = if is_root {
            root_rect.clone()
        } else {
            child_bounds(target).unwrap_or_else(|| root_rect.clone())
        };
        // 组件把窗口铺满时按窗口报 —— 标签上写"组件"没有意义
        let same_as_root = rect.x == root_rect.x
            && rect.y == root_rect.y
            && rect.width == root_rect.width
            && rect.height == root_rect.height;
        let kind = if is_root || same_as_root {
            "window"
        } else {
            "component"
        };

        let mut pid: u32 = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
        }
        let title = read_text(target);
        return Some(crate::types::NativeElement {
            hwnd: target.0 as i64,
            rect,
            kind: kind.into(),
            class_name: read_class(target),
            title: if title.is_empty() && kind == "component" {
                read_class(target)
            } else {
                title.clone()
            },
            window_title: if is_root { title } else { read_text(hwnd) },
            exe: process_exe(pid),
        });
    }
    None
}

/// 当前前台窗口。用于"窗口截图"和剪贴板来源识别。
pub fn foreground_window() -> Option<NativeWindowInfo> {
    set_dpi_aware();
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        describe(hwnd, u32::MAX)
    }
}

/// 前台窗口的进程名（剪贴板来源识别用）
pub fn foreground_exe() -> String {
    foreground_window().map(|w| w.exe).unwrap_or_default()
}

/// 进程名比较：忽略首尾空白、大小写和 `.exe` 后缀。
///
/// 用户在配置里可能写 `chrome`、`Chrome.EXE`、` chrome `，都该认成同一个程序。
/// 手势的排除列表和剪贴板的"不记录这些程序"共用这一份实现 ——
/// 以前两处各写一遍，先剥后缀再转小写，`Chrome.EXE` 就剥不掉。
pub fn exe_name_eq(a: &str, b: &str) -> bool {
    fn stem(s: &str) -> String {
        let t = s.trim().to_ascii_lowercase();
        t.strip_suffix(".exe").unwrap_or(&t).to_string()
    }
    let a = stem(a);
    !a.is_empty() && a == stem(b)
}

/// 指定窗口的进程名。剪贴板面板用它提示「粘贴到 xxx」。
pub fn exe_of_hwnd(hwnd: isize) -> String {
    if hwnd == 0 {
        return String::new();
    }
    let mut pid: u32 = 0;
    unsafe {
        GetWindowThreadProcessId(HWND(hwnd as *mut core::ffi::c_void), Some(&mut pid as *mut u32));
    }
    if pid == 0 {
        return String::new();
    }
    process_exe(pid)
}

/// 剪贴板内容是从哪个进程复制进来的。
///
/// 优先用 `GetClipboardOwner` —— 这是**精确**答案，谁持有剪贴板就是谁复制的。
/// 但很多应用复制完立刻释放所有权（延迟渲染），这时回退到当前前台窗口。
/// 两者都拿不到就返回空，前端会显示"未知"而不是瞎猜。
pub fn clipboard_source_app() -> String {
    unsafe {
        if let Ok(owner) = GetClipboardOwner() {
            if !owner.is_invalid() {
                let mut pid: u32 = 0;
                GetWindowThreadProcessId(owner, Some(&mut pid as *mut u32));
                if pid > 0 {
                    let exe = process_exe(pid);
                    if !exe.is_empty() {
                        return exe;
                    }
                }
            }
        }
    }

    // 回退：前台窗口。自己写剪贴板时前台可能是自己，要排掉。
    let fg = foreground_exe();
    if fg.is_empty() || fg == "snipforge" {
        String::new()
    } else {
        fg
    }
}

/* ======================= 手势用的窗口操作 / 打开外部程序 ======================= */

use windows::Win32::UI::WindowsAndMessaging::{
    PostMessageW, SetWindowPos, ShowWindow, HWND_NOTOPMOST, HWND_TOPMOST, SW_MAXIMIZE, SW_MINIMIZE,
    SW_RESTORE, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WM_CLOSE,
};

/// 对指定窗口做一件事。
///
/// 为什么要能传句柄而不是只认 `GetForegroundWindow`：
/// 手势的动作是异步派发的（不能卡在钩子回调里），等到真正执行时，
/// 前台窗口可能已经变成 NULL（窗口切换的瞬间就是这样）。
/// 所以调用方在**手势开始时**就把当时的前台窗口记下来，执行时优先用它。
pub fn act_on(hwnd_raw: isize, op: &str) -> Result<(), String> {
    unsafe {
        let hwnd = HWND(hwnd_raw as *mut core::ffi::c_void);
        if hwnd.0.is_null() {
            return Err("没有可操作的窗口".into());
        }
        match op {
            "close" => {
                PostMessageW(Some(hwnd), WM_CLOSE, Default::default(), Default::default())
                    .map_err(|e| format!("关闭窗口失败: {e}"))?;
            }
            "minimize" => {
                let _ = ShowWindow(hwnd, SW_MINIMIZE);
            }
            "maximize" => {
                let _ = ShowWindow(hwnd, SW_MAXIMIZE);
            }
            "restore" => {
                let _ = ShowWindow(hwnd, SW_RESTORE);
            }
            "topmost" => {
                let ex = windows::Win32::UI::WindowsAndMessaging::GetWindowLongW(hwnd, GWL_EXSTYLE);
                let on_top = (ex & WS_EX_TOPMOST_RAW) != 0;
                let _ = SetWindowPos(
                    hwnd,
                    Some(if on_top { HWND_NOTOPMOST } else { HWND_TOPMOST }),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            other => return Err(format!("不认识的窗口操作：{other}")),
        }
    }
    Ok(())
}

/// WS_EX_TOPMOST 的原始值。窗口样式的常量在 0.62 里是 `WINDOW_EX_STYLE`，
/// 和 GetWindowLongW 返回的 i32 不能直接比，所以单独取一个数值。
const WS_EX_TOPMOST_RAW: i32 = 0x0000_0008;

/// 用系统默认方式打开一个目标：程序、文档、文件夹、网址都行。
///
/// 走 ShellExecute 而不是 `Command::new(path)`：
/// 后者对 `.lnk`、文件夹、`https://` 一律无能为力，还得自己判断类型。
pub fn shell_open(target: &str) -> Result<(), String> {
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;

    let target = target.trim();
    if target.is_empty() {
        return Err("目标是空的".into());
    }
    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    unsafe {
        let r = ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(wide.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        );
        // ShellExecute 的返回值 > 32 才算成功（这是它最坑的约定）
        if (r.0 as isize) <= 32 {
            return Err(format!("打不开：{target}"));
        }
    }
    Ok(())
}

/// 运行一条命令（隐藏控制台窗口）
pub fn run_command(cmd: &str) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let cmd = cmd.trim();
    if cmd.is_empty() {
        return Err("命令是空的".into());
    }
    std::process::Command::new("cmd")
        .arg("/c")
        .arg(cmd)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .map_err(|e| format!("运行命令失败: {e}"))?;
    Ok(())
}

/* ================== 手势用：全屏判断 / 前台程序（带缓存） ================== */

use std::sync::Mutex;
use std::time::{Duration, Instant};

/// 前台窗口是不是"真·全屏"。
///
/// 判据：窗口矩形**完全盖住**所在显示器的物理边界。
/// 不去查窗口样式（`WS_POPUP` + 无边框并不等于全屏，很多工具窗口也长这样），
/// 也不看标题 —— 只看几何，最不容易误判。
///
/// 桌面（Progman/WorkerW）和任务栏要排除掉：它们天生就是全屏尺寸，
/// 但那是"什么都没开"的状态，手势必须能用。
pub fn foreground_is_fullscreen() -> bool {
    use windows::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MonitorFromWindow, MONITORINFO, MONITOR_DEFAULTTONEAREST,
    };
    use windows::Win32::UI::WindowsAndMessaging::{GetClassNameW, GetWindowRect};

    set_dpi_aware();
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return false;
        }

        let mut cls = [0u16; 64];
        let n = GetClassNameW(hwnd, &mut cls);
        let class = String::from_utf16_lossy(&cls[..n.max(0) as usize]);
        if matches!(
            class.as_str(),
            "Progman" | "WorkerW" | "Shell_TrayWnd" | "Shell_SecondaryTrayWnd"
        ) {
            return false;
        }
        // 自己的窗口不算
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
        if pid == std::process::id() {
            return false;
        }

        let mut wr = RECT::default();
        if GetWindowRect(hwnd, &mut wr).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut mi = MONITORINFO {
            cbSize: std::mem::size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut mi).as_bool() {
            return false;
        }
        let m = mi.rcMonitor;
        // 允许 2px 误差：有些程序会把窗口撑到屏幕外一点点
        (wr.left - m.left).abs() <= 2
            && (wr.top - m.top).abs() <= 2
            && (wr.right - m.right).abs() <= 2
            && (wr.bottom - m.bottom).abs() <= 2
    }
}

/// 前台程序名缓存。
///
/// 每次按下都去 OpenProcess + 查路径要 1~2ms，钩子回调里虽然能承受，
/// 但完全没必要 —— 用户在同一个窗口里连续划手势时答案不会变。
/// 缓存 600ms，足够覆盖一次连续操作，又不会在切窗口后长时间用错答案。
static EXE_CACHE: Mutex<Option<(Instant, String)>> = Mutex::new(None);

pub fn foreground_exe_cached() -> String {
    let now = Instant::now();
    if let Ok(guard) = EXE_CACHE.lock() {
        if let Some((at, exe)) = guard.as_ref() {
            if now.duration_since(*at) < Duration::from_millis(600) {
                return exe.clone();
            }
        }
    }
    let exe = foreground_exe().to_ascii_lowercase();
    if let Ok(mut guard) = EXE_CACHE.lock() {
        *guard = Some((now, exe.clone()));
    }
    exe
}

#[cfg(test)]
mod element_probe {
    use super::*;

    /// 手动跑：`cargo test --lib element_probe -- --ignored --nocapture`
    /// 用来确认 z 序命中 + 组件下钻在真机上确实是通的（CI 里没有桌面，默认忽略）。
    #[test]
    #[ignore]
    fn probe() {
        let mut list: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(enum_proc), LPARAM(&mut list as *mut Vec<HWND> as isize));
        }
        let self_pid = std::process::id();
        let ok: Vec<HWND> = list
            .iter()
            .copied()
            .filter(|h| pickable_cheap(*h, self_pid) && pickable_deep(*h))
            .collect();
        println!("ENUM total={} pickable={}", list.len(), ok.len());
        for h in ok.iter().take(12) {
            println!(
                "  WIN title='{}' class='{}' rect={:?}",
                read_text(*h),
                read_class(*h),
                window_bounds(*h)
            );
        }
        let mut probes: Vec<(i32, i32)> = vec![(100, 100), (600, 400), (1200, 700)];
        if let Some(r) = ok.iter().find_map(|h| window_bounds(*h)) {
            probes.push((r.x + r.width as i32 / 2, r.y + r.height as i32 / 2));
        }
        for (x, y) in probes {
            match element_at(x, y, false) {
                Some(e) => println!(
                    "PROBE ({x},{y}) kind={} '{}' {}x{} at ({},{}) class='{}' win='{}'",
                    e.kind, e.title, e.rect.width, e.rect.height, e.rect.x, e.rect.y,
                    e.class_name, e.window_title
                ),
                None => println!("PROBE ({x},{y}) none"),
            }
        }
    }
}
