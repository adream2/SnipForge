//! 低级键盘钩子 —— 接管 Win+V 的唯一可靠方式。
//!
//! 为什么不用 `RegisterHotKey`？
//!   Win+V 由系统（ShellExperienceHost）在登录时就占住了，`RegisterHotKey` 一律返回失败。
//!   改注册表 `DisabledHotkeys` 也没用 —— 实测重启 explorer 之后依然注册不上。
//!   唯一能拿到这个组合键的办法是在键盘事件进入系统之前截住它，也就是 WH_KEYBOARD_LL。
//!
//! 代价与约束（写在这里免得以后踩）：
//!   1. 钩子必须在**有消息循环的线程**里安装，否则收不到回调；
//!   2. 回调有 1 秒超时限制，超时会被系统静默摘钩 —— 所以回调里绝不能做重活，
//!      只判断 + 发信号 + 立刻返回；
//!   3. 吞掉按键要返回 `LRESULT(1)`，放行要 `CallNextHookEx`。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use tauri::AppHandle;

use windows::Win32::Foundation::{LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage,
    KBDLLHOOKSTRUCT, MSG, WH_KEYBOARD_LL,
};

/// 是否正在接管 Win+V。关掉时钩子仍然挂着，但只放行不拦截。
static ACTIVE: AtomicBool = AtomicBool::new(false);
static APP: OnceLock<AppHandle> = OnceLock::new();

pub fn set_active(active: bool) {
    ACTIVE.store(active, Ordering::SeqCst);
    log::info!("Win+V 接管: {}", if active { "开启" } else { "关闭" });
}

pub fn is_active() -> bool {
    ACTIVE.load(Ordering::SeqCst)
}

fn key_down(vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY) -> bool {
    unsafe { (GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000) != 0 }
}

/// 安装钩子。幂等，重复调用无副作用。
pub fn install(app: AppHandle) {
    let _ = APP.set(app);

    std::thread::spawn(|| unsafe {
        use windows::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows::Win32::Foundation::HINSTANCE;

        let hinst = match GetModuleHandleW(None) {
            Ok(h) => HINSTANCE(h.0),
            Err(e) => {
                log::error!("获取模块句柄失败，Win+V 接管不可用: {e}");
                return;
            }
        };

        match SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_proc), Some(hinst), 0) {
            Ok(_hook) => log::info!("Win+V 键盘钩子已安装"),
            Err(e) => {
                log::error!("键盘钩子安装失败（可能被安全软件拦截）: {e}");
                return;
            }
        }

        // 钩子必须活在有消息循环的线程里，否则收不到任何回调
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    });
}

unsafe extern "system" fn keyboard_proc(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // code < 0 表示系统要求直接放行
    if code >= 0 && ACTIVE.load(Ordering::SeqCst) {
        let kb = &*(lparam.0 as *const KBDLLHOOKSTRUCT);
        const WM_KEYDOWN: usize = 0x0100;
        const WM_SYSKEYDOWN: usize = 0x0104;
        let is_down = wparam.0 == WM_KEYDOWN || wparam.0 == WM_SYSKEYDOWN;

        // 只处理按下事件，抬起事件直接放行。
        //
        // 这里**故意不做** LLKHF_INJECTED 过滤：远程桌面、AutoHotkey、宏键盘
        // 这类场景发出的按键都带注入标记，过滤掉就等于 Win+V 在这些环境里失灵。
        // 循环风险也不存在 —— 我们自己只会注入 Ctrl+V，从不会注入 Win+V。
        if is_down && kb.vkCode == VK_V.0 as u32 {
            let win = key_down(VK_LWIN) || key_down(VK_RWIN);
            let other = key_down(VK_CONTROL) || key_down(VK_MENU) || key_down(VK_SHIFT);

            if win && !other {
                if let Some(app) = APP.get() {
                    let handle = app.clone();
                    // 回调里不能直接操作窗口：建窗口可能耗时，超时会被摘钩。
                    // 丢给异步运行时，这里立刻返回。
                    // 弹的是剪贴板面板，不是主窗口 —— 用户按 Win+V 想粘贴，
                    // 不是想看一个全屏应用。
                    tauri::async_runtime::spawn(async move {
                        crate::panel::toggle(&handle);
                    });
                }
                // 吞掉这次按键，系统就不会弹它自己的剪贴板面板
                return LRESULT(1);
            }
        }
    }

    CallNextHookEx(None, code, wparam, lparam)
}
