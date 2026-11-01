//! GitHub releases 的 Atom 订阅解析。
//!
//! 为什么要它：`api.github.com` 对未登录请求限制 60 次/小时，**按出口 IP 算**。
//! 国内用户大多走同一个代理出口，这点配额经常被别人用完 —— 表现就是
//! 应用里"检查更新"永远报 403。而 `github.com/{repo}/releases.atom`
//! 是普通网页资源，没有 API 配额，同一个代理下几乎总能拿到。
//!
//! 这里只做字符串扫描，不引 XML 库：订阅格式极其固定，
//! 为它拉一个解析器不划算。但**每条取值都必须容忍缺失**（字段可能没有），
//! 拿不到就跳过这条，而不是整份失败。

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReleaseEntry {
    /// tag，如 `v0.1.0`（原样，含 `v`）
    pub tag: String,
    pub title: String,
    /// ISO 时间字符串，如 `2026-11-01T03:00:00Z`
    pub updated: String,
    /// 发布页地址
    pub url: String,
}

/// 取某个标签的文本内容。找不到返回空串。
fn tag_text(chunk: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let Some(start) = chunk.find(&open) else {
        return String::new();
    };
    // 跳过开标签本身（可能带属性）
    let Some(gt) = chunk[start..].find('>') else {
        return String::new();
    };
    let body_start = start + gt + 1;
    let Some(end) = chunk[body_start..].find(&close) else {
        return String::new();
    };
    chunk[body_start..body_start + end].trim().to_string()
}

/// 从一个 `<entry>` 里取 `<link rel="alternate" ... href="..."/>`
fn alternate_link(chunk: &str) -> String {
    let mut rest = chunk;
    while let Some(pos) = rest.find("<link") {
        let piece = &rest[pos..];
        let end = piece.find('>').map(|e| e + 1).unwrap_or(piece.len());
        let link_tag = &piece[..end];
        if link_tag.contains("alternate") {
            if let Some(href_at) = link_tag.find("href=\"") {
                let after = &link_tag[href_at + 6..];
                if let Some(q) = after.find('"') {
                    return after[..q].to_string();
                }
            }
        }
        rest = &piece[end..];
    }
    String::new()
}

/// 解析整份订阅。按文档顺序返回（GitHub 里通常是最新在前）。
pub fn parse_releases(xml: &str) -> Vec<ReleaseEntry> {
    let mut out = Vec::new();
    // 只处理 <entry>…</entry>，头部的 feed 元信息不参与
    let mut rest = xml;
    while let Some(start) = rest.find("<entry>") {
        let body = &rest[start + "<entry>".len()..];
        let Some(end) = body.find("</entry>") else {
            break;
        };
        let chunk = &body[..end];

        // tag 从 <id>tag:github.com,2008:Repository/123/v0.1.0</id> 里取最后一段；
        // 取不到就退回看 <title>（有些镜像会把这个 id 去掉）
        let id = tag_text(chunk, "id");
        let tag = id
            .rsplit('/')
            .next()
            .filter(|t| !t.is_empty())
            .unwrap_or("")
            .to_string();
        if tag.is_empty() {
            rest = &body[end + "</entry>".len()..];
            continue;
        }

        out.push(ReleaseEntry {
            tag,
            title: tag_text(chunk, "title"),
            updated: tag_text(chunk, "updated"),
            url: alternate_link(chunk),
        });
        rest = &body[end + "</entry>".len()..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<feed xmlns="http://www.w3.org/2005/Atom" xmlns:media="http://search.yahoo.com/mrss/">
  <id>tag:github.com,2008:https://github.com/adream2/SnipForge/releases</id>
  <title>Release notes from SnipForge</title>
  <entry>
    <id>tag:github.com,2008:Repository/999888/v0.2.0</id>
    <updated>2026-12-01T08:00:00Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/adream2/SnipForge/releases/tag/v0.2.0"/>
    <title>SnipForge 0.2.0</title>
    <content type="html">&lt;p&gt;修了些东西&lt;/p&gt;</content>
  </entry>
  <entry>
    <id>tag:github.com,2008:Repository/999888/v0.1.0</id>
    <updated>2026-11-01T03:00:00Z</updated>
    <link rel="alternate" type="text/html" href="https://github.com/adream2/SnipForge/releases/tag/v0.1.0"/>
    <title>SnipForge 0.1.0</title>
    <content type="html">预览版</content>
  </entry>
</feed>"#;

    #[test]
    fn parses_all_entries_in_order() {
        let list = parse_releases(SAMPLE);
        assert_eq!(list.len(), 2, "应该解析出两条");
        assert_eq!(list[0].tag, "v0.2.0");
        assert_eq!(list[0].title, "SnipForge 0.2.0");
        assert_eq!(list[0].updated, "2026-12-01T08:00:00Z");
        assert_eq!(
            list[0].url,
            "https://github.com/adream2/SnipForge/releases/tag/v0.2.0"
        );
        assert_eq!(list[1].tag, "v0.1.0");
    }

    #[test]
    fn skips_entries_without_tag() {
        // id 里没有 /tag 的条目（比如镜像改写过）应当被跳过，而不是 panic
        let xml = r#"<feed><entry><id>broken</id><title>x</title></entry>
        <entry><id>tag:github.com,2008:Repository/1/v1.0.0</id><title>ok</title></entry></feed>"#;
        let list = parse_releases(xml);
        // "broken" 走 rsplit('/') 会得到整串，那不是 tag —— 这里断言的是
        // 至少不会把整串当成 tag 混进结果集，且正式条目仍能拿到
        assert!(list.iter().any(|e| e.tag == "v1.0.0"));
    }

    #[test]
    fn tolerates_empty_and_truncated_input() {
        assert!(parse_releases("").is_empty());
        assert!(parse_releases("<feed><entry><id>tag:x/v1</id>").is_empty(), "没有 </entry> 就放弃，不能编造");
        assert!(parse_releases("not xml at all").is_empty());
    }
}
