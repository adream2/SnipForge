//! 开机自启。
//!
//! 以前设置里的「开机自启」只写进了 settings.json —— 开关是纯装饰的，
//! 重启电脑什么都不会发生。这里把它落到
//! `HKCU\Software\Microsoft\Windows\CurrentVersion\Run`：
//! 用户级、不需要管理员权限，而且"现在到底有没有开"可以直接读回来。
//!
//! 为什么用注册表而不是启动文件夹的快捷方式：读写都是几次 API 调用，
//! 不用碰 COM（IShellLink），也不会因为快捷方式被手工删掉而状态失真。
//!
//! 开机会话里带着 `--silent` 启动：只驻留托盘，不弹主界面。
//! 登录 Windows 的瞬间弹一个大窗口抢焦点，比不开自启更烦人 —— 这工具
//! 99% 的时间是"后台待命 + 热键唤起"，启动时没人需要看主界面。

use crate::error::{AppError, AppResult};
use windows::core::PCWSTR;
use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY,
    HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SAM_FLAGS, REG_SZ,
};

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "SnipForge";

/// 静默启动开关，写在 Run 项的命令行里。
///
/// 靠命令行参数而不是"读配置"来判断：配置是用户可改的，而"这次是谁把我
/// 拉起来的"只有命令行知道。手动双击不会带这个参数，所以照常弹主界面。
pub const SILENT_FLAG: &str = "--silent";

/// 本次进程是不是被开机自启拉起来的（→ 只驻留托盘，不显示主界面）。
pub fn silent_start() -> bool {
    std::env::args()
        .skip(1)
        .any(|a| a == SILENT_FLAG || a == "-silent")
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn open_key(access: REG_SAM_FLAGS) -> AppResult<HKEY> {
    let sub = wide(RUN_KEY);
    let mut key = HKEY::default();
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            None,
            access,
            &mut key,
        )
    };
    if rc != ERROR_SUCCESS {
        return Err(AppError::Msg(format!(
            "打开注册表 Run 项失败（错误码 {}）",
            rc.0
        )));
    }
    Ok(key)
}

/// 启动命令。路径里带空格时必须加引号 —— 否则 Windows 会截断到第一个空格，
/// 装在 `C:\Program Files\...` 下就永远起不来。
///
/// 引号必须只包住 exe 路径，`--silent` 要留在引号外面。
fn launch_command() -> AppResult<String> {
    let exe = std::env::current_exe()?;
    Ok(format!("\"{}\" {}", exe.display(), SILENT_FLAG))
}

/// 注册表里现在到底有没有这一项（不看配置，只信系统）。
pub fn is_enabled() -> bool {
    let Ok(key) = open_key(KEY_QUERY_VALUE) else {
        return false;
    };
    let name = wide(VALUE_NAME);
    let mut size = 0u32;
    let rc = unsafe {
        RegQueryValueExW(key, PCWSTR(name.as_ptr()), None, None, None, Some(&mut size))
    };
    unsafe {
        let _ = RegCloseKey(key);
    }
    rc == ERROR_SUCCESS && size > 2
}

/// 打开 / 关闭自启。
///
/// 打开时总是写入**当前** exe 的路径：换版本、改安装目录、或者把绿色版挪了位置之后，
/// 旧的自启项指向的是不存在的文件，表现就是"设了但不自启"。
/// 所以启动时也会调一次 `apply(true)` 做校正 —— 顺带把老版本写的
/// 没有 `--silent` 的命令行升级成静默启动。
pub fn apply(on: bool) -> AppResult<()> {
    let key = open_key(KEY_QUERY_VALUE | KEY_SET_VALUE)?;
    let name = wide(VALUE_NAME);

    let rc = unsafe {
        if on {
            let cmd = match launch_command() {
                Ok(c) => c,
                Err(e) => {
                    let _ = RegCloseKey(key);
                    return Err(e);
                }
            };
            let data = wide(&cmd);
            let bytes = std::slice::from_raw_parts(data.as_ptr() as *const u8, data.len() * 2);
            RegSetValueExW(key, PCWSTR(name.as_ptr()), None, REG_SZ, Some(bytes))
        } else {
            let rc = RegDeleteValueW(key, PCWSTR(name.as_ptr()));
            // 本来就没有这一项 == 已经是关闭状态
            if rc == ERROR_FILE_NOT_FOUND {
                ERROR_SUCCESS
            } else {
                rc
            }
        }
    };

    unsafe {
        let _ = RegCloseKey(key);
    }

    if rc != ERROR_SUCCESS {
        return Err(AppError::Msg(format!(
            "{}开机自启失败（错误码 {}）",
            if on { "设置" } else { "取消" },
            rc.0
        )));
    }
    log::info!("开机自启: {}", if on { "已登记" } else { "已取消" });
    Ok(())
}
