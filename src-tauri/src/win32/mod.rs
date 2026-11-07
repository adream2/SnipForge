//! Win32 FFI 封装层。
//!
//! 全项目只有这个模块 import `windows` crate —— 上层拿到的是干净的 Rust 类型。
//! 这不是洁癖：FFI 细节（句柄生命周期、DPI 上下文、字符串编码）一旦泄漏到业务层，
//! 后面任何一处改动都要重新推理系统调用，成本会指数级上升。

#[cfg(windows)]
pub mod clipboard;
#[cfg(windows)]
pub mod focus;
#[cfg(windows)]
pub mod input;
#[cfg(windows)]
pub mod keyhook;
#[cfg(windows)]
pub mod mousehook;
// 形状识别是纯数学（只依赖 std），住在 core 里才能真的跑测试 ——
// 主 crate 的 test harness 在部分机器上加载不起来（0xc0000139）。
#[cfg(windows)]
pub mod trail;
#[cfg(windows)]
pub mod window;

#[cfg(not(windows))]
pub mod keyhook {
    pub fn install(_app: tauri::AppHandle) {}
    pub fn set_active(_v: bool) {}
    pub fn is_active() -> bool {
        false
    }
}

#[cfg(not(windows))]
pub mod shape {
    pub const MAX_POINTS: usize = 1200;
    pub fn recognize(_points: &[(i32, i32)]) -> Option<char> {
        None
    }
}

#[cfg(not(windows))]
pub mod mousehook {
    #[derive(Debug, Clone)]
    pub enum Event {
        Begin { x: i32, y: i32 },
        Move { x: i32, y: i32 },
        End {
            pattern: String,
            stroke: Vec<(i32, i32)>,
        },
        Cancel,
    }
    pub fn install(_sink: impl Fn(Event) + Send + Sync + 'static) {}
    pub fn update_config(_cfg: crate::config::GestureConfig) {}
    pub fn is_installed() -> bool {
        false
    }
}

#[cfg(not(windows))]
pub mod trail {
    pub fn ensure() {}
    pub fn begin() {}
    pub fn push(_x: i32, _y: i32) {}
    pub fn end() {}
}

#[cfg(not(windows))]
pub mod clipboard {
    pub fn set_files(_paths: &[String]) -> Result<(), String> {
        Err("仅支持 Windows".into())
    }
}

#[cfg(not(windows))]
pub mod focus {
    pub fn foreground_hwnd() -> isize {
        0
    }
    pub fn is_own_window(_hwnd: isize) -> bool {
        false
    }
    pub fn is_alive(_hwnd: isize) -> bool {
        false
    }
    pub fn activate(_hwnd: isize) -> Result<(), String> {
        Err("仅支持 Windows".into())
    }
}

#[cfg(not(windows))]
pub mod input {
    pub fn scroll_at(_x: i32, _y: i32, _delta: i32) -> Result<(), String> {
        Err("仅支持 Windows".into())
    }
    pub fn paste_hotkey() -> Result<(), String> {
        Err("仅支持 Windows".into())
    }
}

/// 只允许一个实例。
///
/// 两个实例会各装一套鼠标钩子，而低级钩子是**链式**的：谁先返回 1，
/// 后面的钩子就收不到这个事件。于是"按下"被 A 吞掉、"抬起"被 B 吞掉时，
/// 两边各缺一半状态 —— 有一边永远等不到抬起，轨迹就会一直跟着鼠标画下去
/// （用户看到的就是"松手了手势还没结束"）。顺带还会出现菜单弹两次、
/// 动作执行两遍这类怪事，排查时极难想到是"开了两份"。
///
/// 用会话内命名互斥体挡掉：第二个实例弹个提示就退出。
#[cfg(windows)]
pub fn ensure_single_instance() {
    use windows::core::w;
    use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS};
    use windows::Win32::System::Threading::CreateMutexW;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

    unsafe {
        match CreateMutexW(None, true, w!("fun.lxzt.snipforge.single")) {
            Ok(handle) => {
                if GetLastError() == ERROR_ALREADY_EXISTS {
                    let _ = MessageBoxW(
                        None,
                        w!("SnipForge 已经在运行。\r\n\r\n它常驻托盘，右键托盘图标可以打开设置或退出。\r\n同时跑两份会让鼠标钩子装两遍，手势会失灵。"),
                        w!("SnipForge"),
                        MB_OK | MB_ICONINFORMATION,
                    );
                    std::process::exit(0);
                }
                // HANDLE 是 Copy 的，忘掉它没有意义；只要**不 CloseHandle**，
                // 互斥体就活到这个进程退出为止 —— 这正是我们要的
                let _ = handle;
            }
            // 建不出来不影响主功能，只是少了一层保护
            Err(e) => eprintln!("单实例互斥体创建失败: {e}"),
        }
    }
}

#[cfg(not(windows))]
pub fn ensure_single_instance() {}

#[cfg(not(windows))]
pub mod window {
    use crate::types::NativeWindowInfo;

    pub fn list_top_level_windows(_self_pid: u32) -> Vec<NativeWindowInfo> {
        Vec::new()
    }
    pub fn foreground_window() -> Option<NativeWindowInfo> {
        None
    }
    pub fn set_dpi_aware() {}
}
