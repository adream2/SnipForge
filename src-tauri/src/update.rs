//! 检查新版本。
//!
//! 只做一件事：问 GitHub「最新 release 是哪个版本」，和当前版本比一比。
//! **不自动下载、不自动安装** —— 那需要签名和增量更新基础设施，
//! 一个预览版阶段的项目不该假装自己有。这里只负责"告诉你新版在哪"。
//!
//! 三个设计取舍：
//!
//! 1. **失败一律静默**。这是个托盘工具，公司网络里出不去很正常；
//!    每次启动弹一句"检查更新失败"比不检查更烦人。错误只写日志。
//! 2. **预发布版也算数**。现在自己就是预览版，只看正式版的话
//!    永远收不到提示。谁更新谁由 `version::is_newer` 判断（semver 规则：
//!    同名正式版大于它的预发布版）。
//! 3. **草稿跳过**。GitHub 上没发布的 draft 带 `draft: true`，
//!    拿到会把内部版本号泄露给用户。

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use serde::Serialize;
use std::sync::Arc;
use std::time::Duration;

/// 发布仓库。tag 形如 `v0.1.0`。
const REPO: &str = "adream2/SnipForge";
const API: &str = "https://api.github.com/repos";
const UA: &str = concat!("SnipForge/", env!("CARGO_PKG_VERSION"));

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    /// 当前运行版本
    pub current: String,
    /// 远端最新版本（去掉 `v` 前缀）
    pub latest: String,
    pub has_update: bool,
    /// 用户可读的标题，比如 "SnipForge 0.1.0 预览版"
    pub title: String,
    /// 发布说明（Markdown 原文，已截断）
    pub notes: String,
    pub published_at: String,
    pub prerelease: bool,
    /// release 页面地址，供「打开发布页」按钮用
    pub url: String,
}

fn client() -> AppResult<reqwest::blocking::Client> {
    crate::net::with_blocking_proxy(
        reqwest::blocking::Client::builder()
            // 检查更新不需要等太久，超时就当没有网
            .timeout(Duration::from_secs(10))
            .user_agent(UA),
    )
    .build()
    .map_err(|e| AppError::Msg(format!("HTTP 客户端创建失败: {e}")))
}

/// 取最新 release。同步阻塞，调用方负责放到后台线程里。
///
/// 两条路：先走 API（信息全），失败再走 releases 订阅（不受 API 配额限制）。
/// 顺序不能反：订阅里是 HTML 正文、也没有结构化字段，能拿到 API 就别将就。
pub fn fetch_latest() -> AppResult<UpdateInfo> {
    let current = env!("CARGO_PKG_VERSION").to_string();
    match fetch_via_api(&current) {
        Ok(info) => Ok(info),
        Err(api_err) => {
            log::warn!("GitHub API 不可用（{api_err}），改用 releases 订阅");
            fetch_via_atom(&current)
                .map_err(|atom_err| AppError::Msg(format!("{api_err}；订阅接口也失败：{atom_err}")))
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn build_info(
    current: &str,
    latest_ver: &snipforge_core::version::Version,
    tag: &str,
    title: &str,
    notes: &str,
    published_at: &str,
    prerelease: bool,
    url: &str,
) -> UpdateInfo {
    // 直接比解析出来的结构，而不是再拿字符串比一遍 ——
    // 少一次解析，也避免"展示用的字符串"和"比较用的版本"走岔
    let has_update = match snipforge_core::version::Version::parse(current) {
        Some(c) => latest_ver > &c,
        None => false,
    };
    let mut notes = notes.trim().to_string();
    // 发布说明可能很长，Toast 和卡片都放不下
    if notes.chars().count() > 1600 {
        notes = notes.chars().take(1600).collect::<String>() + "\n…";
    }
    UpdateInfo {
        current: current.to_string(),
        latest: tag.trim_start_matches(['v', 'V']).to_string(),
        has_update,
        title: if title.trim().is_empty() {
            tag.to_string()
        } else {
            title.trim().to_string()
        },
        notes,
        published_at: published_at.to_string(),
        prerelease,
        url: url.to_string(),
    }
}

fn fetch_via_api(current: &str) -> AppResult<UpdateInfo> {
    let url = format!("{API}/{REPO}/releases?per_page=20");
    let resp = client()?
        .get(&url)
        .header("Accept", "application/vnd.github+json")
        .send()
        .map_err(|e| AppError::Msg(format!("网络不通: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        // 404 / 403 单独说清楚：这两种情况下"检查更新"根本没有结果可拿，
        // 用户需要知道原因，而不是看到一句含糊的"网络错误"。
        let hint = match status.as_u16() {
            404 => "仓库不存在、未公开，或还没有发布过任何版本",
            403 => "GitHub 限流（未登录的 API 每小时 60 次，按出口 IP 算）",
            _ => "网络受限或被代理拦截",
        };
        return Err(AppError::Msg(format!("GitHub 返回 {status}：{hint}")));
    }

    let list: serde_json::Value = resp
        .json()
        .map_err(|e| AppError::Msg(format!("响应解析失败: {e}")))?;
    let releases = list
        .as_array()
        .ok_or_else(|| AppError::Msg("响应不是 release 列表".into()))?;

    // 在这批 release 里挑版本号最大的那一个。
    // 按版本号挑而不是按时间：重新补发一个旧版本时（hotfix 打错 tag），
    // 按时间会选中那个旧的。
    let mut best: Option<(snipforge_core::version::Version, &serde_json::Value)> = None;
    for r in releases {
        if r["draft"].as_bool().unwrap_or(false) {
            continue;
        }
        let tag = r["tag_name"].as_str().unwrap_or("");
        let Some(v) = snipforge_core::version::Version::parse(tag) else {
            log::debug!("跳过无法解析的 tag: {tag}");
            continue;
        };
        match &best {
            Some((cur, _)) if *cur >= v => {}
            _ => best = Some((v, r)),
        }
    }

    let Some((latest_ver, rel)) = best else {
        return Err(AppError::Msg("仓库里还没有已发布的版本".into()));
    };

    Ok(build_info(
        current,
        &latest_ver,
        rel["tag_name"].as_str().unwrap_or(""),
        rel["name"].as_str().unwrap_or(""),
        rel["body"].as_str().unwrap_or(""),
        rel["published_at"].as_str().unwrap_or(""),
        rel["prerelease"].as_bool().unwrap_or(false),
        rel["html_url"].as_str().unwrap_or(""),
    ))
}

/// API 不可用时的兜底：读 releases 的 Atom 订阅。
///
/// 它不是 API，没有 60 次/小时 的限制，共享代理出口也能稳定拿到。
/// 代价是拿不到发布说明的纯文本（里面是 HTML），所以 `notes` 留空，
/// 界面会自动不显示那块 —— 有版本号和发布页链接，用户照样能更新。
fn fetch_via_atom(current: &str) -> AppResult<UpdateInfo> {
    let url = format!("https://github.com/{REPO}/releases.atom");
    let resp = client()?
        .get(&url)
        .header("Accept", "application/atom+xml")
        .send()
        .map_err(|e| AppError::Msg(format!("网络不通: {e}")))?;

    let status = resp.status();
    if !status.is_success() {
        let hint = match status.as_u16() {
            404 => "仓库不存在或未公开",
            _ => "网络受限或被代理拦截",
        };
        return Err(AppError::Msg(format!("订阅返回 {status}：{hint}")));
    }

    let xml = resp
        .text()
        .map_err(|e| AppError::Msg(format!("订阅内容读取失败: {e}")))?;
    let entries = snipforge_core::atom::parse_releases(&xml);
    if entries.is_empty() {
        return Err(AppError::Msg("仓库里还没有已发布的版本".into()));
    }

    type Best<'a> = (
        snipforge_core::version::Version,
        &'a snipforge_core::atom::ReleaseEntry,
    );
    let mut best: Option<Best> = None;
    for e in &entries {
        let Some(v) = snipforge_core::version::Version::parse(&e.tag) else {
            log::debug!("订阅里有无法解析的 tag: {}", e.tag);
            continue;
        };
        match &best {
            Some((cur, _)) if *cur >= v => {}
            _ => best = Some((v, e)),
        }
    }
    let Some((latest_ver, entry)) = best else {
        return Err(AppError::Msg("订阅里的 tag 都不是合法版本号".into()));
    };

    Ok(build_info(
        current,
        &latest_ver,
        &entry.tag,
        &entry.title,
        "", // 订阅里是 HTML 正文，不解析
        &entry.updated,
        // 订阅不带 prerelease 标记。0.x 一律当预览版，和发布流程保持一致
        latest_ver.major == 0,
        &entry.url,
    ))
}

/// 启动后台静默检查。
///
/// 延后几秒再发请求：启动瞬间有一堆事在做（抓屏预热、托盘、快捷键、剪贴板监听），
/// 抢在同一时刻联网只会让首帧更慢。用户感觉不到这几秒。
pub fn spawn_startup_check(app: tauri::AppHandle, state: Arc<AppState>) {
    if !state.settings.read().general.check_updates {
        log::debug!("自动检查更新已关闭");
        return;
    }

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(6));
        match fetch_latest() {
            Ok(info) => {
                if info.has_update {
                    log::info!("发现新版本 {}（当前 {}）", info.latest, info.current);
                    let _ = tauri::Emitter::emit(&app, "update://available", &info);
                } else {
                    log::debug!("已是最新版本 {}", info.current);
                }
                *state.update.write() = Some(info);
            }
            Err(e) => {
                // 没网是常态，不要打扰用户
                log::debug!("检查更新失败（不影响使用）: {e}");
            }
        }
    });
}

/// 手动检查：同步执行，错误要如实返回给界面。
pub fn check_blocking(state: &AppState) -> AppResult<UpdateInfo> {
    let info = fetch_latest()?;
    *state.update.write() = Some(info.clone());
    Ok(info)
}

/// 只允许打开 GitHub 上的链接。
///
/// 地址来自我们自己的后端，但它终究是**网络数据**（release 的 `html_url`）。
/// 万一仓库被劫持或响应被改，`file:///C:/...` 或自定义协议都可能被塞进来 ——
/// 那不是能交给 `ShellExecute` 的东西。
pub fn open_release(url: &str) -> AppResult<()> {
    if !url.starts_with("https://github.com/") {
        return Err(AppError::Msg("拒绝打开非 GitHub 链接".into()));
    }
    crate::win32::window::shell_open(url).map_err(AppError::Msg)
}
