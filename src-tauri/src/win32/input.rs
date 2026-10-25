//! 输入模拟 —— 长截图要自动滚动，剪贴板粘贴要模拟 Ctrl+V。
//!
//! 用 mouse_event / keybd_event 而不是 SendInput：
//! 后者的 INPUT 结构体在 Rust 里要手搓 union，代码量翻三倍，
//! 而这两个老 API 对"滚一格""按一下组合键"这种低频操作完全够用。

use windows::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, mouse_event, MOUSEEVENTF_WHEEL, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::SetCursorPos;

/// WHEEL_DELTA 的标准步长
pub const WHEEL_STEP: i32 = 120;

/// 在物理像素坐标 (x, y) 处滚动滚轮。delta > 0 向上，< 0 向下。
pub fn scroll_at(x: i32, y: i32, delta: i32) -> Result<(), String> {
    super::window::set_dpi_aware();
    unsafe {
        SetCursorPos(x, y).map_err(|e| e.to_string())?;
        // 给目标窗口一点时间响应光标移动，否则滚轮可能落在旧位置
        std::thread::sleep(std::time::Duration::from_millis(30));
        mouse_event(MOUSEEVENTF_WHEEL, 0, 0, delta, 0);
    }
    Ok(())
}

/// 模拟 Ctrl+V。调用前应确保目标窗口已获得焦点。
pub fn paste_hotkey() -> Result<(), String> {
    unsafe {
        keybd_event(VK_CONTROL.0 as u8, 0, Default::default(), 0);
        std::thread::sleep(std::time::Duration::from_millis(12));
        keybd_event(VK_V.0 as u8, 0, Default::default(), 0);
        std::thread::sleep(std::time::Duration::from_millis(24));
        keybd_event(VK_V.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        std::thread::sleep(std::time::Duration::from_millis(12));
        keybd_event(VK_CONTROL.0 as u8, 0, KEYEVENTF_KEYUP, 0);
    }
    Ok(())
}
