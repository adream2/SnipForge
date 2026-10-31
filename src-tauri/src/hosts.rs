//! hosts 文件读写。
//!
//! 难点只有一个：**权限**。`C:\Windows\System32\drivers\etc\hosts` 普通用户
//! 能读、不能写。所以保存分两条路：
//!
//!   - 程序本身就以管理员身份运行 → 直接写；
//!   - 不是 → 内容先写进临时文件，再用 `ShellExecuteEx("runas")` 提权复制回去
//!     （会弹 UAC）。用户在 UAC 里点"取消"必须**明确告诉他没保存**，
//!     而不是安静地失败 —— 改 hosts 的人最怕的就是"以为改了，其实没改"。
//!
//! 另外两个细节：
//!   - 行尾：Windows 的 hosts 惯例是 CRLF。文件本身可能是 CRLF / LF / 纯 CR
//!     （见过被老编辑器写成纯 CR 的，记事本打开是一整行），保存时统一成 CRLF。
//!   - 备份：覆盖前先把原文件存一份到应用数据目录，改坏了能翻回来。

use crate::error::{AppError, AppResult};
use std::path::{Path, PathBuf};

/// hosts 的真实路径。用 `%SystemRoot%` 而不是写死 `C:\Windows`（系统盘不一定在 C）。
pub fn path() -> PathBuf {
    let root = std::env::var_os("SystemRoot")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    root.join("System32")
        .join("drivers")
        .join("etc")
        .join("hosts")
}

/// 当前进程是否已经有管理员权限。
///
/// 用 `IsUserAnAdmin`：它问的正是"这个令牌能不能写系统目录"。
#[cfg(windows)]
pub fn is_elevated() -> bool {
    unsafe { windows::Win32::UI::Shell::IsUserAnAdmin().as_bool() }
}

#[cfg(not(windows))]
pub fn is_elevated() -> bool {
    false
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostsInfo {
    pub path: String,
    pub text: String,
    /// 程序本身是否已有管理员权限（没有的话保存会弹 UAC）
    pub elevated: bool,
    /// 文件里有非 UTF-8 字节：读的时候是"猜"的，写回去会变成 UTF-8
    pub non_utf8: bool,
    /// 文件的换行风格：`crlf` / `lf` / `cr` / `mixed`（保存统一成 CRLF）
    pub eol: String,
    /// 现在真的能写吗（直接试一次写句柄，不看 readonly 属性）
    pub writable: bool,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostsBackup {
    pub name: String,
    pub size: u64,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveOutcome {
    /// 这次是否真的走了提权（用来解释"刚才那个 UAC 是干什么的"）
    pub elevated: bool,
    /// 覆盖前的备份文件名（空 = 文件本来不存在，没东西可备）
    pub backup: String,
}

/// 把任意换行统一成 CRLF，并保证文件以换行结尾。
///
/// 不能"遇到换行就切"再拼：那样文件里**有意的空行**会被吃掉。
/// 先统一成 LF，只收拾末尾，中间的空行原样保留。
fn normalize_eol(text: &str) -> String {
    let unified = text.replace("\r\n", "\n").replace('\r', "\n");
    let body = unified.trim_end_matches('\n');
    if body.is_empty() {
        return "\r\n".to_string();
    }
    let mut out = body.replace('\n', "\r\n");
    out.push_str("\r\n");
    out
}

/// 认出文件用的是哪种换行。
///
/// 别小看这个：真见过 hosts 被写成"只有 CR"（老编辑器或脚本干的），
/// 系统照样能读，但任何按行处理的工具都会把它当成一整行 ——
/// 所以界面上要说清"保存会统一成 CRLF"，而不是悄悄改掉。
fn eol_style(bytes: &[u8]) -> &'static str {
    let crlf = bytes.windows(2).filter(|w| w == b"\r\n").count();
    let cr = bytes.iter().filter(|b| **b == b'\r').count() - crlf;
    let lf = bytes.iter().filter(|b| **b == b'\n').count() - crlf;
    match (crlf > 0, cr > 0, lf > 0) {
        (true, false, false) => "crlf",
        (false, true, false) => "cr",
        (false, false, true) => "lf",
        (false, false, false) => "crlf", // 没有任何换行，随便算一种
        _ => "mixed",
    }
}

pub fn read_info() -> AppResult<HostsInfo> {
    let p = path();
    let bytes = std::fs::read(&p).map_err(|e| AppError::Msg(format!("读取 hosts 失败：{e}")))?;
    let non_utf8 = std::str::from_utf8(&bytes).is_err();
    let text = String::from_utf8_lossy(&bytes)
        .trim_start_matches('\u{feff}')
        .to_string();
    Ok(HostsInfo {
        path: p.to_string_lossy().to_string(),
        text,
        elevated: is_elevated(),
        non_utf8,
        eol: eol_style(&bytes).to_string(),
        writable: can_write(&p),
    })
}

/// 真的能不能写：`readonly` 属性只说明文件属性，不说明 ACL ——
/// 普通用户对 hosts 的 readonly 位是 false，但写的时候照样 Access Denied。
/// 所以直接试着要一个写句柄。
fn can_write(p: &Path) -> bool {
    match std::fs::OpenOptions::new().write(true).open(p) {
        Ok(_) => true,
        // 除了"没权限"以外的错误（比如文件不存在）不算不可写
        Err(e) => e.kind() != std::io::ErrorKind::PermissionDenied,
    }
}

/// 备份目录：应用数据目录下的 `hosts-backups/`
fn backup_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("hosts-backups")
}

/// 把当前文件复制一份到备份目录，返回文件名。文件不存在返回空串。
fn backup(data_dir: &Path) -> AppResult<String> {
    let src = path();
    if !src.exists() {
        return Ok(String::new());
    }
    let dir = backup_dir(data_dir);
    std::fs::create_dir_all(&dir).map_err(|e| AppError::Msg(format!("创建备份目录失败：{e}")))?;
    let name = format!(
        "hosts-{}.txt",
        chrono::Local::now().format("%Y%m%d-%H%M%S")
    );
    std::fs::copy(&src, dir.join(&name))
        .map_err(|e| AppError::Msg(format!("备份 hosts 失败：{e}")))?;
    Ok(name)
}

pub fn list_backups(data_dir: &Path) -> Vec<HostsBackup> {
    let dir = backup_dir(data_dir);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<HostsBackup> = entries
        .flatten()
        .filter(|e| e.path().is_file())
        .map(|e| HostsBackup {
            size: e.metadata().map(|m| m.len()).unwrap_or(0),
            name: e.file_name().to_string_lossy().to_string(),
        })
        .collect();
    // 新的在前：文件名里是时间戳，倒序就是时间倒序
    out.sort_by(|a, b| b.name.cmp(&a.name));
    out.truncate(50);
    out
}

/// 备份文件名白名单：防止前端传 `../..` 把系统文件读出来
fn valid_backup_name(name: &str) -> bool {
    name.starts_with("hosts-") && name.ends_with(".txt") && !name.contains(['/', '\\', ':'])
}

pub fn read_backup(data_dir: &Path, name: &str) -> AppResult<String> {
    if !valid_backup_name(name) {
        return Err(AppError::Msg("备份文件名不合法".into()));
    }
    let p = backup_dir(data_dir).join(name);
    let bytes = std::fs::read(&p).map_err(|e| AppError::Msg(format!("读取备份失败：{e}")))?;
    Ok(String::from_utf8_lossy(&bytes).trim_start_matches('\u{feff}').to_string())
}

/// 保存。`data_dir` 用来放备份。
pub fn save(data_dir: &Path, text: &str) -> AppResult<SaveOutcome> {
    let target = path();
    let body = normalize_eol(text);
    let bak = backup(data_dir)?;

    if is_elevated() {
        std::fs::write(&target, body.as_bytes())
            .map_err(|e| AppError::Msg(format!("写入 hosts 失败：{e}")))?;
        return Ok(SaveOutcome {
            elevated: false,
            backup: bak,
        });
    }

    // 没权限：内容写临时文件，再提权复制回去
    let tmp = std::env::temp_dir().join(format!(
        "snipforge-hosts-{}.txt",
        chrono::Local::now().timestamp_millis()
    ));
    std::fs::write(&tmp, body.as_bytes())
        .map_err(|e| AppError::Msg(format!("写临时文件失败：{e}")))?;

    let result = copy_elevated(&tmp, &target);
    let _ = std::fs::remove_file(&tmp);
    result?;

    // 复制是另一个进程干的，必须回读确认 —— 否则"看起来成功了"最坑人
    let after = std::fs::read(&target)
        .map_err(|e| AppError::Msg(format!("保存后回读失败：{e}")))?;
    let after_text = String::from_utf8_lossy(&after).trim_start_matches('\u{feff}').to_string();
    if normalize_eol(&after_text) != body {
        return Err(AppError::Msg(
            "写入后回读内容不一致，可能有其他程序同时在改 hosts".into(),
        ));
    }

    Ok(SaveOutcome {
        elevated: true,
        backup: bak,
    })
}

/// 提权复制：`cmd /c copy /y from to`，跑在管理员上下文里。
#[cfg(windows)]
fn copy_elevated(from: &Path, to: &Path) -> AppResult<()> {
    use windows::core::{w, PCWSTR};
    use windows::Win32::Foundation::CloseHandle;
    use windows::Win32::System::Threading::{GetExitCodeProcess, WaitForSingleObject, INFINITE};
    use windows::Win32::UI::Shell::{
        ShellExecuteExW, SEE_MASK_FLAG_NO_UI, SEE_MASK_NOCLOSEPROCESS, SHELLEXECUTEINFOW,
    };
    use windows::Win32::UI::WindowsAndMessaging::SW_HIDE;

    let params: Vec<u16> = format!("/c copy /y \"{}\" \"{}\"", from.display(), to.display())
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();

    let mut info = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOCLOSEPROCESS | SEE_MASK_FLAG_NO_UI,
        lpVerb: w!("runas"),
        lpFile: w!("cmd.exe"),
        lpParameters: PCWSTR(params.as_ptr()),
        lpDirectory: PCWSTR::null(),
        nShow: SW_HIDE.0,
        ..Default::default()
    };

    if let Err(e) = unsafe { ShellExecuteExW(&mut info) } {
        // 1223 = ERROR_CANCELLED：用户在 UAC 上点了取消。
        // 注意 windows crate 把 Win32 错误包成了 HRESULT（0x800704C7），
        // 直接和 1223 比永远不相等 —— 要取低 16 位。
        let win32 = e.code().0 as u32 & 0xffff;
        return Err(AppError::Msg(if win32 == 1223 {
            "已取消：写入 hosts 需要管理员权限".into()
        } else {
            format!("申请管理员权限失败：{e}")
        }));
    }
    if info.hProcess.is_invalid() {
        return Err(AppError::Msg("提权进程没有启动".into()));
    }

    unsafe {
        WaitForSingleObject(info.hProcess, INFINITE);
        let mut code = 1u32;
        let _ = GetExitCodeProcess(info.hProcess, &mut code);
        let _ = CloseHandle(info.hProcess);
        if code != 0 {
            return Err(AppError::Msg(format!("复制失败（cmd 退出码 {code}）")));
        }
    }
    Ok(())
}

#[cfg(not(windows))]
fn copy_elevated(_from: &Path, _to: &Path) -> AppResult<()> {
    Err(AppError::Msg("仅支持 Windows".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eol_is_normalized() {
        // 纯 CR：真见过被老编辑器写成这样的 hosts（记事本打开是一整行）
        assert_eq!(normalize_eol("a\rb\r"), "a\r\nb\r\n");
        assert_eq!(normalize_eol("a\nb\n"), "a\r\nb\r\n");
        assert_eq!(normalize_eol("a\r\nb\r\n"), "a\r\nb\r\n");
        // 中间的空行是有意的，不能被吃掉
        assert_eq!(normalize_eol("a\n\nb"), "a\r\n\r\nb\r\n");
        // 末尾多余空行收掉
        assert_eq!(normalize_eol("a\r\n\r\n\r\n"), "a\r\n");
        // 空文件也能写
        assert_eq!(normalize_eol(""), "\r\n");
    }

    #[test]
    fn eol_style_detected() {
        assert_eq!(eol_style(b"a\r\nb\r\n"), "crlf");
        assert_eq!(eol_style(b"a\rb\rc"), "cr");
        assert_eq!(eol_style(b"a\nb\nc"), "lf");
        assert_eq!(eol_style(b"a\r\nb\nc"), "mixed");
        assert_eq!(eol_style(b"abc"), "crlf");
    }

    #[test]
    fn backup_name_is_sandboxed() {
        assert!(!valid_backup_name("../../Windows/System32/drivers/etc/hosts"));
        assert!(!valid_backup_name(r"..\..\secret"));
        assert!(!valid_backup_name(r"C:\Windows\hosts-1.txt"));
        assert!(valid_backup_name("hosts-20261031-161500.txt"));
    }
}
