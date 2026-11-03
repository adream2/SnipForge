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
// 这里 re-export，`crate::win32::shape::xxx` 的老路径继续可用。
pub use snipforge_core::shape;
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
