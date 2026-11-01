//! 统一的 HTTP 出口。
//!
//! 存在的理由只有一个：**让所有出网请求都走系统代理**。
//!
//! reqwest 只认 `HTTP_PROXY` / `HTTPS_PROXY` / `ALL_PROXY` 这些**环境变量**，
//! 不读 Windows 的「Internet 选项」。而国内绝大多数代理软件（Clash / v2rayN /
//! Shadowsocks 客户端）默认打开的是**系统代理**（WinINET），环境变量是空的。
//! 结果就是：浏览器和 `curl` 都能上 GitHub，应用里的下载 OCR、翻译、
//! 检查更新却一律"网络不通" —— 用户只会觉得"这软件的网络功能是坏的"。
//!
//! 所以这里自己去读注册表，把系统代理喂给 reqwest。
//! 已经设了环境变量的话以环境变量为准（那是用户显式指定的，优先级更高）。

use windows::core::PCWSTR;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::{
    RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, REG_DWORD,
};

const INET_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn read_dword(key: HKEY, name: &str) -> Option<u32> {
    let n = wide(name);
    let mut data = [0u8; 4];
    let mut size = data.len() as u32;
    let mut kind = Default::default();
    let rc = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(n.as_ptr()),
            None,
            Some(&mut kind),
            Some(data.as_mut_ptr()),
            Some(&mut size),
        )
    };
    (rc == ERROR_SUCCESS && kind == REG_DWORD && size == 4)
        .then(|| u32::from_le_bytes(data))
}

fn read_string(key: HKEY, name: &str) -> Option<String> {
    let n = wide(name);
    // 先探长度，再按实际长度分配 —— 代理地址可能很长（PAC/多协议串）
    let mut size = 0u32;
    let mut kind = Default::default();
    let probe = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(n.as_ptr()),
            None,
            Some(&mut kind),
            None,
            Some(&mut size),
        )
    };
    if probe != ERROR_SUCCESS || size == 0 {
        return None;
    }
    let mut buf = vec![0u8; size as usize + 2];
    let rc = unsafe {
        RegQueryValueExW(
            key,
            PCWSTR(n.as_ptr()),
            None,
            Some(&mut kind),
            Some(buf.as_mut_ptr()),
            Some(&mut size),
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }
    let units: Vec<u16> = buf
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .take_while(|u| *u != 0)
        .collect();
    Some(String::from_utf16_lossy(&units))
}

/// 系统代理地址，形如 `http://127.0.0.1:7893`。没开或读不到就是 `None`。
///
/// `ProxyServer` 有两种写法：
///   - `127.0.0.1:7893`（所有协议共用一个）
///   - `http=127.0.0.1:7893;https=127.0.0.1:7893`（分协议）
/// 分协议时要挑 `https`（我们的请求全是 https），挑不到再退 `http`。
pub fn system_proxy() -> Option<String> {
    // 用户显式设了环境变量就尊重它，别再插一脚
    for k in ["HTTPS_PROXY", "https_proxy", "ALL_PROXY", "all_proxy", "HTTP_PROXY", "http_proxy"] {
        if std::env::var_os(k).is_some_and(|v| !v.is_empty()) {
            return None;
        }
    }

    let sub = wide(INET_KEY);
    let mut key = HKEY::default();
    let rc = unsafe {
        RegOpenKeyExW(
            HKEY_CURRENT_USER,
            PCWSTR(sub.as_ptr()),
            None,
            KEY_QUERY_VALUE,
            &mut key,
        )
    };
    if rc != ERROR_SUCCESS {
        return None;
    }

    let result = (|| {
        if read_dword(key, "ProxyEnable")? == 0 {
            return None;
        }
        let server = read_string(key, "ProxyServer")?;
        let server = server.trim();
        if server.is_empty() {
            return None;
        }
        let host = if server.contains('=') {
            // 分协议写法：https 优先，其次 http
            let mut https = None;
            let mut http = None;
            for part in server.split(';') {
                let (scheme, addr) = part.split_once('=')?;
                match scheme.trim().to_ascii_lowercase().as_str() {
                    "https" => https = Some(addr.trim().to_string()),
                    "http" => http = Some(addr.trim().to_string()),
                    _ => {}
                }
            }
            https.or(http)?
        } else {
            server.to_string()
        };
        // socks 代理 reqwest 用不了（没开 socks 特性），配了也只会更糟
        if host.starts_with("socks") || host.is_empty() {
            return None;
        }
        let url = if host.contains("://") {
            host
        } else {
            format!("http://{host}")
        };
        Some(url)
    })();

    unsafe {
        let _ = RegCloseKey(key);
    }
    result
}

/// 取一个可用的代理配置；地址无效时返回 None 并按直连处理。
fn proxy_of(url: &str) -> Option<reqwest::Proxy> {
    match reqwest::Proxy::all(url) {
        Ok(p) => {
            log::debug!("HTTP 走系统代理: {url}");
            Some(p)
        }
        Err(e) => {
            log::warn!("系统代理地址无效（{url}），按直连处理: {e}");
            None
        }
    }
}

/// 给**异步** reqwest builder 挂上系统代理。
pub fn with_proxy(builder: reqwest::ClientBuilder) -> reqwest::ClientBuilder {
    match system_proxy().as_deref().and_then(proxy_of) {
        Some(p) => builder.proxy(p),
        None => builder,
    }
}

/// 给**阻塞** reqwest builder 挂上系统代理。
pub fn with_blocking_proxy(
    builder: reqwest::blocking::ClientBuilder,
) -> reqwest::blocking::ClientBuilder {
    match system_proxy().as_deref().and_then(proxy_of) {
        Some(p) => builder.proxy(p),
        None => builder,
    }
}
