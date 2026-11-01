//! 「关于」页里的几个外链。
//!
//! URL 只在这里出现一次：界面传一个名字过来，后端查表再交给系统浏览器。
//! 好处是不用在 TS 里散落硬编码地址，也不可能因为拼错参数把任意链接塞进 shell。

use crate::error::{AppError, AppResult};

/// 产品主页（GitHub Pages）
const SITE: &str = "https://adream2.github.io/SnipForge/";
const REPO: &str = "https://github.com/adream2/SnipForge";
const RELEASES: &str = "https://github.com/adream2/SnipForge/releases/latest";
const ISSUES: &str = "https://github.com/adream2/SnipForge/issues";
const CHANGELOG: &str = "https://github.com/adream2/SnipForge/blob/main/CHANGELOG.md";
const LICENSE: &str = "https://github.com/adream2/SnipForge/blob/main/LICENSE";

fn url_for(target: &str) -> Option<&'static str> {
    match target {
        "site" => Some(SITE),
        "repo" => Some(REPO),
        "releases" => Some(RELEASES),
        "issues" => Some(ISSUES),
        "changelog" => Some(CHANGELOG),
        "license" => Some(LICENSE),
        _ => None,
    }
}

/// 打开「关于」页的某个链接。只接受上面表里的名字。
#[tauri::command]
pub fn about_open(target: String) -> AppResult<()> {
    let url = url_for(&target).ok_or_else(|| AppError::Msg(format!("未知链接: {target}")))?;
    crate::win32::window::shell_open(url).map_err(AppError::Msg)
}

/// 界面要显示地址时用（比如「官网」那一行的小字），保证和实际打开的完全一致。
#[tauri::command]
pub fn about_links() -> Vec<(&'static str, &'static str)> {
    vec![
        ("site", SITE),
        ("repo", REPO),
        ("releases", RELEASES),
        ("issues", ISSUES),
        ("changelog", CHANGELOG),
        ("license", LICENSE),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 表里的地址必须是 https —— 免得哪天手滑写成 http 或者带空格。
    #[test]
    fn all_links_are_https() {
        for (name, url) in about_links() {
            assert!(url.starts_with("https://"), "{name} 不是 https: {url}");
            assert!(!url.contains(' '), "{name} 含空格: {url}");
        }
    }

    #[test]
    fn unknown_target_rejected() {
        assert!(url_for("site").is_some());
        assert!(url_for("https://evil.example").is_none());
        assert!(url_for("").is_none());
    }
}
