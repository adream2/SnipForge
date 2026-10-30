//! 顶层窗口枚举与前台窗口识别。
//!
//! 两个关键细节，踩过的坑都在这：
//!   1. 必须先把进程设为 Per-Monitor DPI Aware，否则在 125%/150% 缩放下
//!      Windows 会给虚拟化后的逻辑坐标，跟 Tauri 的物理像素完全对不上。
//!   2. 用 DWMWA_EXTENDED_FRAME_BOUNDS 而不是 GetWindowRect 取边界 ——
//!      后者含窗口阴影，截窗口时会多出一圈透明边。

use crate::types::{NativeWindowInfo, Rect};
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
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
    EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowLongW, GetWindowRect,
    GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GWL_EXSTYLE,
    WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

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
        let mut raw: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut raw as *mut u32));
        pid = raw;
    };
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

/// 对当前前台窗口做一件事：close / minimize / maximize / restore / topmost
///
/// 用手势操作窗口时，前台窗口就是用户看着的那个 —— 不需要再问他"哪个窗口"。
pub fn act_on_foreground(op: &str) -> Result<(), String> {
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return Err("当前没有前台窗口".into());
        }
        act_on(hwnd.0 as isize, op)
    }
}

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
