//! 手势串的解析与匹配。
//!
//! 全是纯函数，住在 core 里才能真的跑单测 —— 主 crate 的 test harness
//! 在部分机器上加载不起来（`STATUS_ENTRYPOINT_NOT_FOUND`），
//! 而匹配规则恰恰是最需要回归保护的地方（"↓↑→↓ 在浏览器里打开主页"就是这么来的）。
//!
//! 命名空间约定：token 用逗号分隔，`"D,R"` = 先下后右，`"DR"` = 一条右下斜线。
//! 不分开的话这两种手势会连成同一个串，语义彻底混掉。

use crate::config::GestureBinding;

/// 方向串切 token：`"D,R"` → `["D", "R"]`。
pub fn tokens(dir: &str) -> Vec<&str> {
    dir.split(',').filter(|t| !t.is_empty()).collect()
}

/// 配置里手写的方向串统一成规范形式。
///
/// 两种写法都认：
///   - **带分隔符**：`"D,R"` = 先下后右，每个部分必须是一个合法 token
///   - **连写**：`"DRUR"` = `DR,UR`（两字符 token 优先）
pub fn normalize(pattern: &str) -> String {
    const TWO: [&str; 4] = ["DR", "DL", "UR", "UL"];
    const ONE: [&str; 4] = ["U", "D", "L", "R"];

    let raw = pattern.trim();
    if raw.contains(',') || raw.contains(' ') {
        return raw
            .split(|c: char| c == ',' || c.is_whitespace())
            .map(|t| t.trim().to_ascii_uppercase())
            .filter(|t| TWO.contains(&t.as_str()) || ONE.contains(&t.as_str()))
            .collect::<Vec<_>>()
            .join(",");
    }

    // 连写：贪心切，两字符优先（认 DR，不拆成 D + R）
    let s: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphabetic())
        .map(|c| c.to_ascii_uppercase())
        .collect();
    let b = s.as_bytes();
    let mut out: Vec<String> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        if i + 2 <= b.len() {
            let two = &s[i..i + 2];
            if TWO.contains(&two) {
                out.push(two.to_string());
                i += 2;
                continue;
            }
        }
        let one = &s[i..i + 1];
        if ONE.contains(&one) {
            out.push(one.to_string());
        }
        i += 1;
    }
    out.join(",")
}

/// 两个 token 序列的编辑距离（插入 / 删除 / 替换各算 1）。
///
/// 手势串越写越长，要求逐字相等太脆 —— 转折处多判或少判一个方向是常态。
pub fn edit_distance(a: &[&str], b: &[&str]) -> usize {
    let (n, m) = (a.len(), b.len());
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut cur = vec![0usize; m + 1];
    for i in 1..=n {
        cur[0] = i;
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            cur[j] = (prev[j] + 1).min(cur[j - 1] + 1).min(prev[j - 1] + cost);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[m]
}

/// 允许差几个 token。
///
/// 串越短越不能容忍：`D,R,D`（主页）只差一个 token 就变成 `D,U,R,D`
/// （编辑 hosts），两个手势会互相吞。四五段以上的长手势在转折处
/// 多判/少判一个方向是常态，差一个还算命中。
pub fn tolerance(n: usize) -> usize {
    if n <= 2 {
        0
    } else {
        1
    }
}

/// 在一组绑定里找命中的那条，返回（距离, 下标）。
///
/// 距离要给调用方用：通用集里精确命中、而专用集只是"差一个"时，得让精确的赢。
pub fn find_in(bindings: &[GestureBinding], dir: &str) -> Option<(usize, usize)> {
    let cur = tokens(dir);
    if cur.is_empty() {
        return None;
    }
    let mut best: Option<(usize, usize)> = None;
    for (i, b) in bindings.iter().enumerate() {
        if !b.enabled {
            continue;
        }
        let tpl_s = normalize(&b.pattern);
        let tpl = tokens(&tpl_s);
        if tpl.is_empty() {
            continue;
        }
        let d = if tpl == cur {
            0
        } else {
            edit_distance(&cur, &tpl)
        };
        // 画得比绑定**短**的直接跳过：3 段不可能是"4 段漏了一段"，更可能是
        // 用户就想画 3 段。只容忍"画得比绑定长"（转折处手抖多切出一个 token）。
        // 不加这条，画 ↓→↓ 会去命中 ↓↑→↓（编辑 hosts）。
        if cur.len() < tpl.len() {
            continue;
        }
        if d > tolerance(tpl.len()) {
            continue;
        }
        if best.map_or(true, |(bd, _)| d < bd) {
            best = Some((d, i));
        }
    }
    best
}

/// 专用集和通用集都命中时选哪个：**先比距离，距离相同才让专用集赢**。
///
/// 只按"专用集优先"会出这种错：通用集里 `D,U,R,D`（编辑 hosts）精确命中，
/// 专用集里 `D,R,D`（主页）只差一个 token —— 结果在浏览器里画 ↓↑→↓ 打开的是主页。
pub fn pick(
    profile: Option<(usize, usize)>,
    general: Option<(usize, usize)>,
) -> Option<(bool, usize)> {
    match (profile, general) {
        (Some((pd, pi)), Some((gd, gi))) => Some(if gd < pd { (false, gi) } else { (true, pi) }),
        (Some((_, pi)), None) => Some((true, pi)),
        (None, Some((_, gi))) => Some((false, gi)),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding(pattern: &str) -> GestureBinding {
        GestureBinding {
            pattern: pattern.into(),
            points: Vec::new(),
            action: None,
            actions: Vec::new(),
            enabled: true,
        }
    }

    #[test]
    fn normalize_keeps_token_boundaries() {
        // 连写：两字符 token 优先
        assert_eq!(normalize("DRUR"), "DR,UR");
        assert_eq!(normalize("drur"), "DR,UR");
        // 显式分隔：D + R 是两个 token，不是一条斜线
        assert_eq!(normalize("D,R"), "D,R");
        assert_eq!(normalize("D R"), "D,R");
        // 单条斜线
        assert_eq!(normalize("DR"), "DR");
        assert_eq!(normalize("dr"), "DR");
        // 非法 token 整个丢掉（"D-X" 不是合法方向，不做部分解析）
        assert_eq!(normalize("D-X,R"), "R");
        assert_eq!(normalize(""), "");
    }

    #[test]
    fn edit_distance_counts_one_token() {
        let a = tokens("DR,UR");
        assert_eq!(edit_distance(&a, &tokens("DR,U,UR")), 1);
        assert_eq!(edit_distance(&a, &tokens("DR,UL")), 1);
        // 换掉两个：2
        assert_eq!(edit_distance(&a, &tokens("U,D")), 2);
        // 完全不相干的长串：不算命中
        assert_eq!(edit_distance(&a, &tokens("U,D,L,R")), 4);
    }

    /// 「↓↑→↓ 在浏览器里打开主页」的回归用例。
    ///
    /// 通用集 `D,U,R,D`（编辑 hosts）精确命中，浏览器专用集 `D,R,D`（主页）
    /// 只差一个 token。距离小的必须赢，否则短手势会被专用集抢走。
    #[test]
    fn exact_beats_fuzzy_across_scopes() {
        let general = vec![binding("D,U,R,D")];
        let profile = vec![binding("D,R,D")];
        let g = find_in(&general, "D,U,R,D");
        let p = find_in(&profile, "D,U,R,D");
        assert_eq!(g.map(|(d, _)| d), Some(0), "通用集应当精确命中");
        assert_eq!(p.map(|(d, _)| d), Some(1), "专用集只该是差一个");
        assert_eq!(pick(p, g), Some((false, 0)), "必须走通用集");
    }

    /// 两段以内的串只认精确：`U` 是复制，画 `U,R` 不该被判成复制。
    #[test]
    fn short_patterns_need_exact() {
        let list = vec![binding("U"), binding("D,R")];
        assert!(find_in(&list, "U,R").is_none(), "U,R 不该命中 U");
        assert!(find_in(&list, "D").is_none(), "单段 D 不该命中 D,R");
        assert_eq!(find_in(&list, "D,R").map(|(d, _)| d), Some(0));
    }

    /// 三段以上容忍"多画了一个 token"（转折处手抖），但**不容忍少画**。
    #[test]
    fn longer_stroke_may_have_one_extra_token() {
        let list = vec![binding("D,R,D")];
        assert_eq!(find_in(&list, "D,U,R,D").map(|(d, _)| d), Some(1));
    }

    /// 反方向不容忍：画得比绑定短就不是这个手势。
    #[test]
    fn shorter_stroke_never_matches_longer_binding() {
        let list = vec![binding("D,U,R,D")];
        assert!(find_in(&list, "D,R,D").is_none(), "3 段不该命中 4 段绑定");
    }
}
