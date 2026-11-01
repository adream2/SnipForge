//! 版本号解析与比较。
//!
//! 放在 core 里是因为它**纯粹是字符串逻辑**，而"有没有新版本"这件事
//! 一旦判断错，用户看到的就是"点了更新却说已是最新"或者"天天提示更新"。
//! 这种逻辑必须有断言兜着 —— 主 crate 的测试跑不起来，所以放这儿。
//!
//! 只实现 semver 里我们真正需要的部分：
//! - 可选的前导 `v`（tag 都是 `v0.1.0` 这种写法）
//! - `major.minor.patch`
//! - 可选的 `-prerelease`（按 semver 规则：有 prerelease 的**小于**同名正式版）
//! - 忽略 `+build` 元数据（semver 规定它不参与比较）

use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// 预发布标识，空 = 正式版
    pub pre: Vec<String>,
}

impl Version {
    /// 解析失败返回 None。**宁可返回 None 也不要瞎猜** ——
    /// 猜错的结果是"永远显示有新版本"。
    pub fn parse(text: &str) -> Option<Self> {
        let s = text.trim();
        let s = s.strip_prefix('v').or_else(|| s.strip_prefix('V')).unwrap_or(s);
        // 丢掉 build 元数据
        let s = s.split('+').next().unwrap_or(s);
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, p.split('.').map(|x| x.to_string()).collect()),
            None => (s, Vec::new()),
        };

        let mut it = core.split('.');
        let major = it.next()?.parse().ok()?;
        let minor = it.next()?.parse().ok()?;
        let patch = it.next()?.parse().ok()?;
        // 多出来的段（0.1.0.0）不接受：那说明不是 semver，比较结果没意义
        if it.next().is_some() {
            return None;
        }
        Some(Self {
            major,
            minor,
            patch,
            pre,
        })
    }

    fn triple(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        // 先比数字段；再比预发布标识
        self.triple()
            .cmp(&other.triple())
            .then_with(|| match (self.pre.is_empty(), other.pre.is_empty()) {
                // 正式版 > 预发布版
                (true, false) => Ordering::Greater,
                (false, true) => Ordering::Less,
                (true, true) => Ordering::Equal,
                (false, false) => cmp_pre(&self.pre, &other.pre),
            })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// prerelease 段比较：逐段比，数字段按数值、其它按字典序，
/// 前缀相同则**短的小**（`alpha` < `alpha.1`）。
fn cmp_pre(a: &[String], b: &[String]) -> Ordering {
    for (x, y) in a.iter().zip(b.iter()) {
        let xn = x.parse::<u64>();
        let yn = y.parse::<u64>();
        let ord = match (xn, yn) {
            (Ok(p), Ok(q)) => p.cmp(&q),
            // 数字段 < 字母段（semver 规定）
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => x.cmp(y),
        };
        if ord != Ordering::Equal {
            return ord;
        }
    }
    a.len().cmp(&b.len())
}

/// 远端版本是否比当前版本新。
///
/// 解析不出来时一律返回 `false` —— 宁可漏一次提示，
/// 也不要因为一个怪 tag 就天天弹"有新版本"。
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (Version::parse(latest), Version::parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_tag_shapes() {
        assert!(Version::parse("0.1.0").is_some());
        assert!(Version::parse("v0.1.0").is_some(), "tag 带 v 前缀是常态");
        assert!(Version::parse(" V1.2.3 ").is_some(), "空格和大写 V 也要认");
        let v = Version::parse("0.1.0-preview.2").unwrap();
        assert_eq!(v.pre, vec!["preview", "2"]);
        // build 元数据不参与比较，解析时应被丢弃
        let v = Version::parse("0.1.0+build.5").unwrap();
        assert!(v.pre.is_empty());
        // 不是 semver 就不要硬猜
        assert!(Version::parse("0.1").is_none());
        assert!(Version::parse("latest").is_none());
        assert!(Version::parse("0.1.0.0").is_none());
    }

    #[test]
    fn numeric_order() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("0.1.10", "0.1.9"), "按数值比，不是按字符串");
        assert!(is_newer("1.0.0", "0.99.99"));
        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("0.1.0", "0.2.0"));
    }

    #[test]
    fn stable_beats_prerelease_of_same_version() {
        // 1.0.0 正式版比 1.0.0-rc.1 新 —— 从 rc 升到正式版必须提示
        assert!(is_newer("1.0.0", "1.0.0-rc.1"));
        assert!(!is_newer("1.0.0-rc.1", "1.0.0"), "已经是正式版就别往回退");
    }

    #[test]
    fn prerelease_ordering() {
        assert!(is_newer("0.1.0-preview.2", "0.1.0-preview.1"));
        assert!(!is_newer("0.1.0-preview.1", "0.1.0-preview.2"));
        // semver：alpha < alpha.1
        assert!(is_newer("1.0.0-alpha.1", "1.0.0-alpha"));
        // 数字段 < 字母段
        assert!(is_newer("1.0.0-alpha", "1.0.0-1"));
    }

    #[test]
    fn garbage_never_reports_newer() {
        // 这条是防"天天弹更新"的：远端 tag 解析不出来时不许提示
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(!is_newer("", "0.1.0"));
        assert!(!is_newer("0.1.0", "weird"));
    }

    #[test]
    fn v_prefix_is_not_a_downgrade() {
        assert!(!is_newer("v0.1.0", "0.1.0"), "同一个版本只是写法不同");
        assert!(is_newer("v0.1.1", "0.1.0"));
    }
}
