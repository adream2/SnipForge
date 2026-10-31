//! 输入模拟 —— 长截图要自动滚动，剪贴板粘贴要模拟 Ctrl+V，
//! 鼠标手势要发送任意快捷键 / 文字 / 滚轮。
//!
//! 早先这里用 `keybd_event` / `mouse_event`（少写点 INPUT 的 union），
//! 但**发文字**必须走 `SendInput` + `KEYEVENTF_UNICODE`：
//! `keybd_event` 的扫描码参数只有 8 位，中文这种多字节字符根本传不进去。
//! 既然已经为文字引进了 SendInput，按键也一并用它 —— 一次调用发完整组键，
//! 中间不会被别的输入插队。

use windows::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, mouse_event, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
    KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, KEYEVENTF_UNICODE, MOUSEEVENTF_WHEEL, VIRTUAL_KEY,
    VK_CONTROL, VK_V,
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

/* ============================ 手势用的通用输入 ============================ */

fn key_event(vk: u16, scan: u16, flags: KEYBD_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: scan,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// 键名 → 虚拟键码。
///
/// 覆盖"写快捷键会用到的全部"：修饰键、字母数字、F1-F24、编辑键、方向键、
/// 媒体键、常见符号。认不出来就报错，而不是静默发一个空按键 ——
/// "手势没反应"是最难查的问题。
fn vk_for(name: &str) -> Option<u16> {
    let n = name.trim().to_ascii_lowercase();

    // 单字符：字母/数字直接就是 VK 码
    if n.chars().count() == 1 {
        let c = n.chars().next().unwrap();
        if c.is_ascii_alphabetic() {
            return Some(c.to_ascii_uppercase() as u16);
        }
        if c.is_ascii_digit() {
            return Some(c as u16);
        }
        return match c {
            '-' => Some(0xBD),
            '=' => Some(0xBB),
            ',' => Some(0xBC),
            '.' => Some(0xBE),
            '/' => Some(0xBF),
            ';' => Some(0xBA),
            '`' => Some(0xC0),
            '[' => Some(0xDB),
            ']' => Some(0xDD),
            '\\' => Some(0xDC),
            _ => None,
        };
    }

    // F1..F24
    if let Some(num) = n.strip_prefix('f') {
        if let Ok(i) = num.parse::<u16>() {
            if (1..=24).contains(&i) {
                return Some(0x70 + i - 1);
            }
        }
    }

    Some(match n.as_str() {
        "ctrl" | "control" => 0x11,
        "shift" => 0x10,
        "alt" | "option" => 0x12,
        "win" | "super" | "meta" | "cmd" => 0x5B,
        "tab" => 0x09,
        "enter" | "return" => 0x0D,
        "esc" | "escape" => 0x1B,
        "space" => 0x20,
        "backspace" | "back" => 0x08,
        "delete" | "del" => 0x2E,
        "insert" | "ins" => 0x2D,
        "home" => 0x24,
        "end" => 0x23,
        "pageup" | "pgup" => 0x21,
        "pagedown" | "pgdn" => 0x22,
        "up" => 0x26,
        "down" => 0x28,
        "left" => 0x25,
        "right" => 0x27,
        "capslock" => 0x14,
        "printscreen" | "prtsc" => 0x2C,
        "pause" => 0x13,
        "apps" | "menu" => 0x5D,
        "volup" | "volumeup" => 0xAF,
        "voldown" | "volumedown" => 0xAE,
        "mute" => 0xAD,
        "play" | "playpause" => 0xB3,
        "next" | "nexttrack" => 0xB0,
        "prev" | "prevtrack" => 0xB1,
        "stop" => 0xB2,
        "browserback" => 0xA6,
        "browserforward" => 0xA7,
        "browserrefresh" => 0xA8,
        _ => return None,
    })
}

fn is_modifier(vk: u16) -> bool {
    matches!(vk, 0x10 | 0x11 | 0x12 | 0x5B | 0x5C)
}

/// 发送按键序列。
///
/// 语法：`Ctrl+Shift+T`；多组用逗号分隔，按顺序依次发送（`Ctrl+A,Ctrl+C`）。
pub fn send_keys(seq: &str) -> Result<(), String> {
    let combos: Vec<&str> = seq
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if combos.is_empty() {
        return Err("按键是空的".into());
    }

    for combo in combos {
        let parts: Vec<&str> = combo
            .split('+')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        if parts.is_empty() {
            continue;
        }

        let mut mods: Vec<u16> = Vec::new();
        let mut main: Option<u16> = None;
        for (i, p) in parts.iter().enumerate() {
            let vk = vk_for(p).ok_or_else(|| format!("认不出的按键：{p}"))?;
            // 只有排在中间的修饰键才当修饰键用；单独的 "Ctrl" 就是按一下 Ctrl
            if i + 1 < parts.len() && is_modifier(vk) {
                mods.push(vk);
            } else {
                main = Some(vk);
            }
        }

        let mut inputs: Vec<INPUT> = Vec::new();
        for m in &mods {
            inputs.push(key_event(*m, 0, KEYBD_EVENT_FLAGS(0)));
        }
        if let Some(k) = main {
            inputs.push(key_event(k, 0, KEYBD_EVENT_FLAGS(0)));
            inputs.push(key_event(k, 0, KEYEVENTF_KEYUP));
        }
        for m in mods.iter().rev() {
            inputs.push(key_event(*m, 0, KEYEVENTF_KEYUP));
        }

        unsafe {
            if SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) == 0 {
                return Err("按键注入被系统拒绝".into());
            }
        }
        // 组合之间留间隔：连发两次 Alt+Tab 太快会被系统合并成一次
        std::thread::sleep(std::time::Duration::from_millis(60));
    }
    Ok(())
}

/// 逐字输入一段文本（中文可用）。
pub fn send_text(text: &str) -> Result<(), String> {
    for ch in text.chars() {
        // 换行单独发 Enter：UNICODE 包里塞 '\n' 有些程序不认
        if ch == '\n' {
            send_keys("Enter")?;
            continue;
        }
        let mut buf = [0u16; 2];
        for unit in ch.encode_utf16(&mut buf) {
            let down = key_event(0, *unit, KEYEVENTF_UNICODE);
            let up = key_event(0, *unit, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP);
            unsafe {
                SendInput(&[down, up], std::mem::size_of::<INPUT>() as i32);
            }
        }
    }
    Ok(())
}

/// 滚轮：在当前光标位置滚动，delta > 0 向上
pub fn wheel(delta: i32) -> Result<(), String> {
    unsafe {
        mouse_event(MOUSEEVENTF_WHEEL, 0, 0, delta, 0);
    }
    Ok(())
}

/* ============================ 手势用：补发右键点击 ============================ */

/// 注入标记：钩子看到这个 `dwExtraInfo` 就知道"这次点击是我们自己补发的"，
/// 直接放行，不再当作手势输入处理。没有这个标记会无限递归。
pub const INJECT_MAGIC: usize = 0x534E_4950; // 'SNIP'

fn mouse_input(flags: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: windows::Win32::UI::Input::KeyboardAndMouse::INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: windows::Win32::UI::Input::KeyboardAndMouse::MOUSEINPUT {
                dx: 0,
                dy: 0,
                mouseData: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: INJECT_MAGIC,
            },
        },
    }
}

/// 补发一次右键点击。
///
/// 手势的策略是"吞掉按下"，所以普通右键点击必须由我们补回来 ——
/// 否则用户的右键就没了。必须用 `SendInput` 而不是 `mouse_event`：
/// 只有前者能带 `dwExtraInfo`，没有标记钩子会把自己注入的点击再吞一次。
pub fn inject_right_click() -> Result<(), String> {
    use windows::Win32::UI::Input::KeyboardAndMouse::{MOUSEEVENTF_RIGHTDOWN, MOUSEEVENTF_RIGHTUP};
    let inputs = [
        mouse_input(MOUSEEVENTF_RIGHTDOWN),
        mouse_input(MOUSEEVENTF_RIGHTUP),
    ];
    unsafe {
        if SendInput(&inputs, std::mem::size_of::<INPUT>() as i32) == 0 {
            return Err("补发右键失败".into());
        }
    }
    Ok(())
}
