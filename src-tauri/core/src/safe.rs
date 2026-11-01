//! 路径与文件名的消毒。
//!
//! 这一层之所以**必须**存在：`dir` / `file_name` 都是前端（或插件 JSON、
//! 或远端图床配置）传进来的字符串。`Path::join("..\\..\\Windows\\Temp")`
//! 会老老实实拼到根目录外面去 —— 本地图床因此变成"任意位置写文件"。
//!
//! 放在 core 里而不是主 crate，是因为它是**纯字符串逻辑**，
//! 这样才有真正能跑起来的单测（主 crate 的测试 target 在某些机器上加载不了）。

use std::path::{Component, Path, PathBuf};

/// 把用户提供的相对子路径安全地拼到根目录下。
///
/// 按路径段逐个校验，只放行普通名字；`..`、盘符、根路径一律拒绝。
/// 目标已存在时再做一次 `canonicalize` 比对，挡住符号链接 / junction 跳出根目录。
pub fn safe_join(root: &str, sub: &str) -> Result<PathBuf, String> {
    let root = root.trim();
    if root.is_empty() {
        return Err("目标目录为空".into());
    }
    let root_path = Path::new(root);
    let mut out = root_path.to_path_buf();

    let raw_sub = sub.trim();
    // UNC（\\server\share）必须先判，不能让它走到下面的"裁掉前导斜杠"里：
    // 那样会被静默改写成 `<root>/server/share`，用户以为写到了网络共享上，
    // 实际写在了本地目标目录里 —— 不报错但完全不是他要的。
    if raw_sub.starts_with(r"\\") {
        return Err(format!("不支持 UNC 网络路径：{raw_sub}"));
    }
    let sub = raw_sub.trim_matches(&['/', '\\'][..]);
    if !sub.is_empty() {
        for comp in Path::new(sub).components() {
            match comp {
                Component::Normal(part) => out.push(part),
                // "./a" 这种无意义但无害的前缀直接忽略
                Component::CurDir => {}
                _ => return Err(format!("路径不合法：{sub}")),
            }
        }
    }

    // 只有两边都存在时才能做真实路径比对（新建目录不必因此失败）
    if let (Ok(real_root), Ok(real_target)) = (root_path.canonicalize(), out.canonicalize()) {
        if !real_target.starts_with(&real_root) {
            return Err(format!("路径越界：{sub}"));
        }
    }
    Ok(out)
}

/// 只接受"纯文件名"：不含分隔符、不是 `.` / `..`，去掉 Windows 非法字符。
pub fn safe_file_name(name: &str) -> Result<String, String> {
    let raw = name.trim();
    if raw.is_empty() {
        return Err("文件名不能为空".into());
    }
    let cleaned: String = raw
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 32 => '_',
            c => c,
        })
        .collect();
    let trimmed = cleaned.trim().trim_matches('.').trim().to_string();
    if trimmed.is_empty() {
        return Err(format!("文件名不合法：{name}"));
    }
    Ok(trimmed.chars().take(180).collect())
}

/// 拼装文件名：{yyyy}{MM}{dd}{HH}{mm}{ss}{rand}{type}
pub fn render_file_name(template: &str, ext: &str) -> String {
    let now = chrono::Local::now();
    let rand: String = {
        use std::time::{SystemTime, UNIX_EPOCH};
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0);
        format!("{:04x}", (nanos ^ (std::process::id() << 8)) & 0xffff)
    };

    let mut out = template.to_string();
    for (key, value) in [
        ("{yyyy}", now.format("%Y").to_string()),
        ("{MM}", now.format("%m").to_string()),
        ("{dd}", now.format("%d").to_string()),
        ("{HH}", now.format("%H").to_string()),
        ("{mm}", now.format("%M").to_string()),
        ("{ss}", now.format("%S").to_string()),
        ("{rand}", rand),
        ("{type}", ext.to_string()),
    ] {
        out = out.replace(key, &value);
    }

    // Windows 文件名非法字符一律替换掉，别让保存莫名其妙失败
    let sanitized: String = out
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 32 => '_',
            c => c,
        })
        .collect();

    let trimmed = sanitized.trim().trim_matches('.').to_string();
    if trimmed.is_empty() {
        format!("SnipForge_{}", chrono::Local::now().format("%Y%m%d_%H%M%S"))
    } else {
        trimmed.chars().take(180).collect()
    }
}

/// 展开目录模板：{yyyy} {MM} {dd} {HH} {mm} {ss}，并去掉首尾斜杠、合并多余斜杠。
pub fn render_dir(template: &str) -> String {
    let now = chrono::Local::now();
    let mut out = template.trim().to_string();
    for (key, value) in [
        ("{yyyy}", now.format("%Y").to_string()),
        ("{yy}", now.format("%y").to_string()),
        ("{MM}", now.format("%m").to_string()),
        ("{dd}", now.format("%d").to_string()),
        ("{HH}", now.format("%H").to_string()),
        ("{mm}", now.format("%M").to_string()),
        ("{ss}", now.format("%S").to_string()),
    ] {
        out = out.replace(key, &value);
    }
    // 干掉空段（a//b → a/b），别让远端出现奇怪的路径
    let parts: Vec<&str> = out
        .split('/')
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn join_keeps_inside_root() {
        let root = std::env::temp_dir().join("snipforge-safe-join");
        let _ = std::fs::create_dir_all(&root);
        let r = root.to_string_lossy().to_string();

        let ok = safe_join(&r, "2026/11/shot.png").expect("普通子路径应当放行");
        assert!(ok.starts_with(&root));

        // 前导斜杠要当成"相对"处理，不是绝对路径
        let ok2 = safe_join(&r, "/2026/11").expect("前导斜杠应当被裁掉");
        assert!(ok2.starts_with(&root));
    }

    #[test]
    fn join_rejects_traversal() {
        let root = std::env::temp_dir().join("snipforge-safe-join2");
        let _ = std::fs::create_dir_all(&root);
        let r = root.to_string_lossy().to_string();

        assert!(safe_join(&r, "../outside").is_err(), ".. 必须拒绝");
        assert!(safe_join(&r, "..\\..\\Windows\\Temp").is_err(), "反斜杠 .. 必须拒绝");
        assert!(safe_join(&r, "a/../../b").is_err(), "中间夹 .. 必须拒绝");
        assert!(safe_join("", "x").is_err(), "空根目录必须报错");

        // Windows 上还要挡住盘符和 UNC 路径 —— 它们都是"根"，能直接跳出目标目录
        #[cfg(windows)]
        for bad in [r"C:\Windows", r"\\server\share", r"..\..\Windows"] {
            assert!(safe_join(&r, bad).is_err(), "应当拒绝: {bad}");
        }
    }

    #[test]
    fn file_name_strips_separators() {
        assert_eq!(safe_file_name("a/b.png").unwrap(), "a_b.png");
        assert_eq!(safe_file_name("..\\evil.png").unwrap(), "_evil.png");
        assert!(safe_file_name("   ").is_err());
        // 只有点 = 没有名字
        assert!(safe_file_name("...").is_err());
    }

    #[test]
    fn file_name_caps_length() {
        let long = "x".repeat(500);
        assert_eq!(safe_file_name(&long).unwrap().chars().count(), 180);
    }

    #[test]
    fn template_expands_placeholders() {
        let name = render_file_name("{yyyy}-{MM}-{dd}_{type}", "png");
        assert!(name.ends_with(".png") || name.ends_with("_png"), "得到 {name}");
        assert!(!name.contains('{'), "占位符必须全部替换：{name}");
        assert!(!name.contains('/'), "文件名里不能有分隔符：{name}");
    }

    #[test]
    fn empty_template_falls_back() {
        // 模板被清空时不能生成一个空文件名，那会直接创建失败
        let name = render_file_name("   ", "png");
        assert!(name.starts_with("SnipForge_"), "得到 {name}");
    }

    #[test]
    fn dir_template_drops_empty_segments() {
        let d = render_dir("blog//{yyyy}/");
        assert!(!d.contains("//"), "得到 {d}");
        assert!(!d.starts_with('/') && !d.ends_with('/'), "得到 {d}");
        assert!(d.starts_with("blog/"));
    }
}
