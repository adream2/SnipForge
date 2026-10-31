//! 翻译服务。
//!
//! 为什么默认不是谷歌：`translate.googleapis.com` 在国内基本连不上，
//! 打开取字窗口点翻译只能看到"服务不可达"。默认换成**腾讯交互翻译**
//! （transmart 的网页接口）：免 key、国内直连、自带语言识别。
//!
//! 四个供应商：
//!   - tencent：transmart 的 `api/imt`，免 key（默认）
//!   - google：免 key 的 `translate_a/single`（国内多半要走代理）
//!   - deepl：需要 key，`:fx` 结尾是免费版，走另一个域名
//!   - custom：用户自己填 URL / method / headers / body，结果用点路径取
//!
//! 语言码按供应商各自映射（腾讯用 `zh`、DeepL 用 `ZH`、谷歌用 `zh-CN`），
//! 前端只认一套规范码，转换都在这里做。

use crate::config::TranslateConfig;
use crate::error::{AppError, AppResult};
use std::time::Duration;

/// 目标语言（规范码 → 界面上显示的名字）。
pub const LANGS: &[(&str, &str)] = &[
    ("zh-CN", "简体中文"),
    ("zh-TW", "繁體中文"),
    ("en", "English"),
    ("ja", "日本語"),
    ("ko", "한국어"),
    ("fr", "Français"),
    ("de", "Deutsch"),
    ("es", "Español"),
    ("ru", "Русский"),
    ("pt", "Português"),
    ("it", "Italiano"),
    ("ar", "العربية"),
    ("th", "ไทย"),
    ("vi", "Tiếng Việt"),
];

/// 供应商显示名（设置页和取字窗口都用它）
pub fn provider_label(id: &str) -> &'static str {
    match id {
        "google" => "谷歌",
        "deepl" => "DeepL",
        "custom" => "自定义",
        _ => "腾讯",
    }
}

fn client() -> AppResult<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(12))
        .user_agent("Mozilla/5.0 (Windows NT 10.0; Win64; x64) SnipForge")
        .build()
        .map_err(|e| AppError::Msg(format!("创建请求失败: {e}")))
}

pub async fn translate(cfg: &TranslateConfig, text: &str, to: Option<&str>) -> AppResult<String> {
    let text = text.trim();
    if text.is_empty() {
        return Err(AppError::Msg("没有可翻译的内容".into()));
    }
    // 端点普遍限长，超了直接拒绝，别发出一个注定失败的请求
    if text.chars().count() > 4000 {
        return Err(AppError::Msg("文本太长，先选一段再翻译".into()));
    }
    let target = normalize(to.filter(|s| !s.trim().is_empty()).unwrap_or(&cfg.target));
    match cfg.provider.as_str() {
        "google" => google(&target, text).await,
        "deepl" => deepl(cfg, &target, text).await,
        "custom" => custom(cfg, &target, text).await,
        _ => tencent(&target, text).await,
    }
}

fn normalize(code: &str) -> String {
    let c = code.trim();
    if c.is_empty() {
        "zh-CN".to_string()
    } else {
        c.to_string()
    }
}

/// 规范码 → 腾讯的语言码
fn tc_lang(code: &str) -> &str {
    match code {
        "zh-CN" => "zh",
        "zh-TW" => "zh-TW",
        other => other,
    }
}

/// 规范码 → DeepL 的语言码
fn deepl_lang(code: &str) -> &str {
    match code {
        "zh-CN" => "ZH",
        "zh-TW" => "ZH-HANT",
        "en" => "EN",
        "ja" => "JA",
        "ko" => "KO",
        "fr" => "FR",
        "de" => "DE",
        "es" => "ES",
        "ru" => "RU",
        "pt" => "PT-BR",
        "it" => "IT",
        "ar" => "AR",
        other => other,
    }
}

/// 腾讯交互翻译：transmart 网页用的接口，免 key、国内直连。
///
/// 一次能传多行（`text_list`），按行翻再拼回去 —— 段落结构能保住，
/// 比整段丢进去翻出来一团要好读。
async fn tencent(target: &str, text: &str) -> AppResult<String> {
    let lines: Vec<&str> = text.lines().collect();
    let body = serde_json::json!({
        "header": {
            "fn": "auto_translation",
            "client_key": "browser-chrome-120.0.0.0-20240101-1-1"
        },
        "type": "plain",
        "model_category": "normal",
        "source": { "lang": "auto", "text_list": lines },
        "target": { "lang": tc_lang(target) }
    });

    let resp = client()?
        .post("https://transmart.qq.com/api/imt")
        .header("Referer", "https://transmart.qq.com/zh-CN/index")
        .json(&body)
        .send()
        .await
        .map_err(|e| AppError::Msg(format!("腾讯翻译不可达: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Msg(format!("腾讯翻译返回 {}", resp.status())));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Msg(format!("腾讯翻译结果解析失败: {e}")))?;

    let code = v
        .get("header")
        .and_then(|h| h.get("ret_code"))
        .and_then(|c| c.as_str())
        .unwrap_or("");
    if !code.is_empty() && code != "succ" {
        return Err(AppError::Msg(format!("腾讯翻译失败: {code}")));
    }
    let out = v
        .get("auto_translation")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|x| x.as_str())
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();
    if out.trim().is_empty() {
        return Err(AppError::Msg("腾讯翻译没返回内容".into()));
    }
    Ok(out)
}

/// 谷歌：免 key 端点（国内通常连不上）
async fn google(target: &str, text: &str) -> AppResult<String> {
    let url = format!(
        "https://translate.googleapis.com/translate_a/single?client=gtx&sl=auto&tl={target}&dt=t&q={}",
        percent_encode(text)
    );
    let resp = client()?
        .get(&url)
        .send()
        .await
        .map_err(|e| {
            AppError::Msg(format!("谷歌翻译不可达（国内一般需要代理）: {e}"))
        })?;
    if !resp.status().is_success() {
        return Err(AppError::Msg(format!("谷歌翻译返回 {}", resp.status())));
    }
    let body: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Msg(format!("谷歌翻译结果解析失败: {e}")))?;
    // [[["译文","原文",...], ...], ...]
    let mut out = String::new();
    if let Some(segments) = body.get(0).and_then(|v| v.as_array()) {
        for seg in segments {
            if let Some(t) = seg.get(0).and_then(|v| v.as_str()) {
                out.push_str(t);
            }
        }
    }
    if out.is_empty() {
        return Err(AppError::Msg("谷歌翻译没返回内容".into()));
    }
    Ok(out)
}

/// DeepL：`:fx` 结尾的 key 是免费版，域名不同
async fn deepl(cfg: &TranslateConfig, target: &str, text: &str) -> AppResult<String> {
    let key = cfg.deepl_key.trim();
    if key.is_empty() {
        return Err(AppError::Msg("先在设置里填 DeepL 的 key".into()));
    }
    let url = if key.ends_with(":fx") {
        "https://api-free.deepl.com/v2/translate"
    } else {
        "https://api.deepl.com/v2/translate"
    };
    let body = format!(
        "text={}&target_lang={}",
        percent_encode(text),
        deepl_lang(target)
    );
    let resp = client()?
        .post(url)
        .header("Authorization", format!("DeepL-Auth-Key {key}"))
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| AppError::Msg(format!("DeepL 不可达: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Msg(format!("DeepL 返回 {}", resp.status())));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| AppError::Msg(format!("DeepL 结果解析失败: {e}")))?;
    v.get("translations")
        .and_then(|t| t.get(0))
        .and_then(|t| t.get("text"))
        .and_then(|t| t.as_str())
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::Msg("DeepL 没返回内容".into()))
}

/// 自定义 HTTP：URL / body 里用 `{text}`、`{to}` 占位，结果按点路径取。
async fn custom(cfg: &TranslateConfig, target: &str, text: &str) -> AppResult<String> {
    let url_tpl = cfg.custom_url.trim();
    if url_tpl.is_empty() {
        return Err(AppError::Msg("先在设置里填自定义翻译的 URL".into()));
    }
    let url = url_tpl
        .replace("{text}", &percent_encode(text))
        .replace("{to}", target);
    let body = cfg
        .custom_body
        .replace("{text}", text)
        .replace("{to}", target);

    let mut req = if cfg.custom_method.eq_ignore_ascii_case("post") {
        client()?.post(&url)
    } else {
        client()?.get(&url)
    };
    for line in cfg.custom_headers.lines() {
        if let Some((k, v)) = line.split_once(':') {
            let k = k.trim();
            if !k.is_empty() {
                req = req.header(k, v.trim());
            }
        }
    }
    if cfg.custom_method.eq_ignore_ascii_case("post") {
        if !cfg.custom_headers.to_lowercase().contains("content-type") {
            req = req.header("Content-Type", "application/json");
        }
        req = req.body(body);
    }

    let resp = req
        .send()
        .await
        .map_err(|e| AppError::Msg(format!("自定义翻译不可达: {e}")))?;
    let status = resp.status();
    let raw = resp
        .text()
        .await
        .map_err(|e| AppError::Msg(format!("自定义翻译读取失败: {e}")))?;
    if !status.is_success() {
        let brief: String = raw.chars().take(160).collect();
        return Err(AppError::Msg(format!("自定义翻译返回 {status}: {brief}")));
    }

    // 有 json 就按点路径取，取不到再退回原文
    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) {
        if !cfg.custom_result_path.trim().is_empty() {
            let picked = pick_path(&v, cfg.custom_result_path.trim());
            if let Some(s) = picked.as_ref().and_then(|x| x.as_str()) {
                if !s.is_empty() {
                    return Ok(s.to_string());
                }
            }
            return Err(AppError::Msg(format!(
                "按路径 {} 没取到译文，返回内容：{}",
                cfg.custom_result_path.trim(),
                raw.chars().take(160).collect::<String>()
            )));
        }
        // 没填路径：字符串就直接用，否则整个 json 当文本
        if let Some(s) = v.as_str() {
            return Ok(s.to_string());
        }
    }
    if raw.trim().is_empty() {
        return Err(AppError::Msg("自定义翻译没返回内容".into()));
    }
    Ok(raw.trim().to_string())
}

/// 点路径取值：`data.translations.0.text`
fn pick_path<'a>(v: &'a serde_json::Value, path: &str) -> Option<&'a serde_json::Value> {
    let mut cur = v;
    for seg in path.split('.') {
        let seg = seg.trim();
        if seg.is_empty() {
            continue;
        }
        cur = match cur {
            serde_json::Value::Object(map) => map.get(seg)?,
            serde_json::Value::Array(arr) => arr.get(seg.parse::<usize>().ok()?)?,
            _ => return None,
        };
    }
    Some(cur)
}

/// 最小可用的 URL 编码：只保留 unreserved 字符，其余按 UTF-8 字节转 %XX。
fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.as_bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(*b as char)
            }
            b' ' => out.push_str("%20"),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真联网跑一次（`cargo test --lib translate -- --ignored --nocapture`）。
    /// 用来确认默认供应商在你这台机器上是通的 —— 谷歌换腾讯就是因为国内连不上。
    #[tokio::test]
    #[ignore]
    async fn live_default_provider() {
        let cfg = TranslateConfig::default();
        let out = translate(&cfg, "hello world
second line", None)
            .await
            .expect("翻译失败");
        println!("译文: {out}");
        assert!(!out.trim().is_empty());
    }

    #[test]
    fn language_codes() {
        assert_eq!(tc_lang("zh-CN"), "zh");
        assert_eq!(tc_lang("zh-TW"), "zh-TW");
        assert_eq!(tc_lang("en"), "en");
        assert_eq!(deepl_lang("zh-CN"), "ZH");
        assert_eq!(deepl_lang("pt"), "PT-BR");
    }

    #[test]
    fn url_encoding() {
        assert_eq!(percent_encode("a b&c"), "a%20b%26c");
        assert_eq!(percent_encode("中文"), "%E4%B8%AD%E6%96%87");
        assert_eq!(percent_encode("a-_.~"), "a-_.~");
    }

    #[test]
    fn pick_json_path() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"data":{"translations":[{"text":"你好"}]}}"#).unwrap();
        assert_eq!(
            pick_path(&v, "data.translations.0.text")
                .and_then(|x| x.as_str()),
            Some("你好")
        );
        assert!(pick_path(&v, "data.nope").is_none());
    }
}
