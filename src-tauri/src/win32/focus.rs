//! 前台窗口接管 —— 剪贴板面板「点一下就能粘到原处」的关键一环。
//!
//! 面板弹出时会抢走焦点，所以必须**在弹窗之前**记下用户当时在哪个窗口；
//! 用户点完条目后先把焦点还回去，再模拟 Ctrl+V。
//!
//! 少任何一步，Ctrl+V 都会落在面板自己身上，用户看到的就是「点了没反应」。

use windows::Win32::Foundation::HWND;
use windows::Win32::System::Threading::AttachThreadInput;
use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowThreadProcessId, IsIconic, IsWindow, SetForegroundWindow,
    ShowWindow, SW_RESTORE,
};

fn hwnd_of(raw: isize) -> HWND {
    HWND(raw as *mut core::ffi::c_void)
}

fn pid_of(hwnd: HWND) -> u32 {
    let mut pid: u32 = 0;
    unsafe {
        GetWindowThreadProcessId(hwnd, Some(&mut pid as *mut u32));
    }
    pid
}

/// 当前前台窗口的原始句柄值，0 表示拿不到。
pub fn foreground_hwnd() -> isize {
    unsafe {
        let h = GetForegroundWindow();
        if h.is_invalid() {
            0
        } else {
            h.0 as isize
        }
    }
}

/// 这个窗口是不是我们自己的？如果用户在 SnipForge 自己的界面里呼出面板，
/// 把焦点还给自己毫无意义，粘贴会变成空操作，这种情况应该退化成「只复制」。
pub fn is_own_window(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    pid_of(hwnd_of(hwnd)) == std::process::id()
}

pub fn is_alive(hwnd: isize) -> bool {
    if hwnd == 0 {
        return false;
    }
    unsafe { IsWindow(Some(hwnd_of(hwnd))).as_bool() }
}

/// 把焦点还给指定窗口。
///
/// 难点在 Windows 的**前台锁**：不是前台进程时 `SetForegroundWindow` 会被静默拒绝。
/// 所以先直接试；失败就挂到目标线程的输入队列上（`AttachThreadInput`），
/// 借用它的前台资格再试一次。这招是 Win32 里唯一稳定可靠的做法。
pub fn activate(hwnd: isize) -> Result<(), String> {
    if hwnd == 0 {
        return Err("没有记录到目标窗口".into());
    }
    let h = hwnd_of(hwnd);
    unsafe {
        if !IsWindow(Some(h)).as_bool() {
            return Err("目标窗口已关闭".into());
        }
        // 最小化的窗口要先还原，否则粘贴会「成功」但用户什么也看不到
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        if SetForegroundWindow(h).as_bool() {
            return Ok(());
        }

        let fg = GetForegroundWindow();
        let fg_tid = GetWindowThreadProcessId(fg, None);
        let target_tid = GetWindowThreadProcessId(h, None);
        if target_tid != 0 && fg_tid != target_tid {
            let _ = AttachThreadInput(fg_tid, target_tid, true);
            let ok = SetForegroundWindow(h).as_bool();
            let _ = AttachThreadInput(fg_tid, target_tid, false);
            if ok {
                return Ok(());
            }
        }
    }
    Err("系统拒绝了焦点切换".into())
}
