//! 内置图床实现。
//!
//! 全部用 reqwest::blocking —— 上传是低频操作，用不着为它铺一层 async 状态机；
//! 命令层会把它丢进 spawn_blocking，不会卡住 UI 线程。

use super::{cfg_str, safe_file_name, safe_join, Cfg, Provider, UploadFile};
use crate::error::{AppError, AppResult};
use crate::types::{
    ConfigField, DirEntry, FieldOption, ProviderMeta, RemoteImage, UploadResult, VisibleWhen,
};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::time::Duration;

type HmacSha256 = Hmac<Sha256>;

fn f(key: &str, label: &str, ty: &str) -> ConfigField {
    ConfigField {
        key: key.into(),
        label: label.into(),
        field_type: ty.into(),
        placeholder: None,
        description: None,
        required: false,
        default: None,
        options: None,
        visible_when: None,
    }
}

fn required(mut field: ConfigField) -> ConfigField {
    field.required = true;
    field
}

fn hint(mut field: ConfigField, text: &str) -> ConfigField {
    field.description = Some(text.into());
    field
}

/// 只在另一个字段取某个值时才显示这个字段。
/// 「自定义域名」这种只在特定选项下才有意义的字段，一直摆着只会让人困惑。
fn visible_when(mut field: ConfigField, key: &str, value: serde_json::Value) -> ConfigField {
    field.visible_when = Some(VisibleWhen {
        key: key.into(),
        equals: value,
    });
    field
}

fn with_default(mut field: ConfigField, value: serde_json::Value) -> ConfigField {
    field.default = Some(value);
    field
}

fn options(mut field: ConfigField, opts: &[(&str, &str)]) -> ConfigField {
    field.options = Some(
        opts.iter()
            .map(|(label, value)| FieldOption {
                label: (*label).into(),
                value: (*value).into(),
            })
            .collect(),
    );
    field
}

fn client(timeout_secs: u64) -> AppResult<reqwest::blocking::Client> {
    // 图床多半在墙外（GitHub / S3 / SM.MS），同样要跟着系统代理走
    crate::net::with_blocking_proxy(
        reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(timeout_secs))
            .user_agent("SnipForge/0.1"),
    )
    .build()
    .map_err(|e| AppError::Msg(format!("HTTP 客户端创建失败: {e}")))
}

fn http_err(status: u16, body: &str) -> AppError {
    AppError::Msg(format!("HTTP {status}: {}", body.chars().take(300).collect::<String>()))
}

/// 是不是图片文件名（按扩展名判断，不读内容——浏览列表不该为此下载整张图）
fn is_image_name(name: &str) -> bool {
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    matches!(
        ext.as_deref(),
        Some("png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" | "svg" | "avif" | "ico")
    )
}

fn ms_from_system(t: std::time::SystemTime) -> i64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/* ============================ GitHub ============================ */

pub struct GithubProvider;

impl Provider for GithubProvider {
    fn meta(&self) -> ProviderMeta {
        ProviderMeta {
            id: "github".into(),
            name: "GitHub 仓库".into(),
            origin: "builtin".into(),
            description: Some("图片提交到 GitHub 仓库，用 jsDelivr 或 raw 当 CDN".into()),
            requires_network: true,
            supports_dir: true,
            can_browse: true,
            can_list_images: true,
            fields: vec![
                hint(
                    required(f("token", "Personal Access Token", "password")),
                    "需要 repo 权限，或 fine-grained token 的 Contents: Read & Write",
                ),
                hint(
                    required(f("owner", "用户名 / 组织", "string")),
                    "仓库所在的账号，例如 github.com/adream2/xxx 里的 adream2",
                ),
                hint(
                    required(f("repo", "仓库名", "string")),
                    "只填仓库名，不带账号前缀",
                ),
                hint(
                    with_default(f("branch", "分支", "string"), serde_json::json!("main")),
                    "仓库的默认分支。老仓库多是 master，新建的默认 main",
                ),
                hint(
                    with_default(f("path", "存储路径", "string"), serde_json::json!("snipforge")),
                    "文件放在仓库里的哪个目录。留空则放在仓库根目录",
                ),
                hint(
                    with_default(
                        options(f("cdn", "访问方式", "select"), &[
                            ("jsDelivr CDN", "jsdelivr"),
                            ("Statically", "statically"),
                            ("GitHub Raw", "raw"),
                            ("gh-proxy（国内加速）", "ghproxy"),
                            ("自定义加速链接", "custom"),
                        ]),
                        serde_json::json!("jsdelivr"),
                    ),
                    "决定上传后给你什么链接。jsDelivr 国内可用但缓存难刷；Statically 次选；
raw 实时但国内常被墙；gh-proxy 是国内公共反代，随时可能换域名或失效",
                ),
                hint(
                    visible_when(
                        f("customDomain", "自定义加速链接", "string"),
                        "cdn",
                        serde_json::json!("custom"),
                    ),
                    "两种写法：
① 只填前缀（如 https://img.example.com），后面自动拼 /存储路径/文件名
② 填完整模板，支持 {owner} {repo} {branch} {path} {file} 五个占位符",
                ),
            ],
        }
    }

    fn upload(&self, file: &UploadFile, cfg: &Cfg) -> AppResult<UploadResult> {
        let token = cfg_str(cfg, "token");
        let owner = cfg_str(cfg, "owner");
        let repo = cfg_str(cfg, "repo");
        if token.is_empty() || owner.is_empty() || repo.is_empty() {
            return Err(AppError::Msg("GitHub 图床配置不完整".into()));
        }

        let branch = {
            let b = cfg_str(cfg, "branch");
            if b.is_empty() { "main".to_string() } else { b }
        };
        // 单次上传可以带目录（在「图床」页选的或新建的）；没带就用配置里的
        let dir = file
            .dir
            .clone()
            .unwrap_or_else(|| cfg_str(cfg, "path"))
            .trim_matches('/')
            .to_string();
        let key = if dir.is_empty() {
            file.file_name.clone()
        } else {
            format!("{dir}/{}", file.file_name)
        };

        let api = format!("https://api.github.com/repos/{owner}/{repo}/contents/{key}");

        // GitHub 要求 base64，且不接受 data URL 前缀
        use base64::Engine;
        let content = base64::engine::general_purpose::STANDARD.encode(&file.buffer);

        let body = serde_json::json!({
            "message": format!("upload: {}", file.file_name),
            "content": content,
            "branch": branch
        });

        let res = client(40)?
            .put(&api)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .json(&body)
            .send()
            .map_err(|e| AppError::Msg(format!("GitHub 请求失败: {e}")))?;

        let status = res.status().as_u16();
        let text = res.text().unwrap_or_default();
        if !(200..300).contains(&status) {
            let hint = serde_json::from_str::<serde_json::Value>(&text)
                .ok()
                .and_then(|v| v["message"].as_str().map(|s| s.to_string()))
                .unwrap_or_else(|| text.chars().take(200).collect());
            return Err(AppError::Msg(format!("GitHub 上传失败 ({status}): {hint}")));
        }

        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let url = github_public_url(cfg, &key)?;

        Ok(UploadResult {
            url,
            delete_url: parsed["content"]["url"].as_str().map(|s| s.to_string()),
            thumb_url: None,
            meta: Some(serde_json::json!({
                "sha": parsed["content"]["sha"],
                "path": key
            })),
        })
    }

    /// 列仓库里某个目录下的子目录。
    ///
    /// 目录不存在时 GitHub 返回 404 —— 这时候不该报错：用户很可能就是要
    /// 新建这个目录（GitHub 上传文件时会自动建），所以当成"空目录"返回。
    fn list_dirs(&self, cfg: &Cfg, path: &str) -> AppResult<Vec<DirEntry>> {
        let token = cfg_str(cfg, "token");
        let owner = cfg_str(cfg, "owner");
        let repo = cfg_str(cfg, "repo");
        if token.is_empty() || owner.is_empty() || repo.is_empty() {
            return Err(AppError::Msg("GitHub 图床配置不完整".into()));
        }
        let branch = {
            let b = cfg_str(cfg, "branch");
            if b.is_empty() { "main".to_string() } else { b }
        };
        let dir = path.trim_matches('/');
        let api = if dir.is_empty() {
            format!("https://api.github.com/repos/{owner}/{repo}/contents?ref={branch}")
        } else {
            format!("https://api.github.com/repos/{owner}/{repo}/contents/{dir}?ref={branch}")
        };

        let res = client(25)?
            .get(&api)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .map_err(|e| AppError::Msg(format!("GitHub 请求失败: {e}")))?;

        let status = res.status().as_u16();
        if status == 404 {
            return Ok(Vec::new());
        }
        let text = res.text().unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(http_err(status, &text));
        }

        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let mut out = Vec::new();
        if let Some(items) = parsed.as_array() {
            for it in items {
                if it["type"].as_str() != Some("dir") {
                    continue;
                }
                let name = it["name"].as_str().unwrap_or_default().to_string();
                let p = it["path"].as_str().unwrap_or(&name).to_string();
                if !name.is_empty() {
                    out.push(DirEntry { name, path: p });
                }
            }
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// 列仓库里某个目录下的图片。
    ///
    /// contents API 一次最多返回 1000 项、按名称升序，且**不返回修改时间**，
    /// 所以没法按时间排。这里的取舍是按名称倒序——图片文件名基本都带日期，
    /// 倒序更可能把最近的排在前面。
    fn list_images(&self, cfg: &Cfg, dir: &str, limit: usize) -> AppResult<Vec<RemoteImage>> {
        let token = cfg_str(cfg, "token");
        let owner = cfg_str(cfg, "owner");
        let repo = cfg_str(cfg, "repo");
        if token.is_empty() || owner.is_empty() || repo.is_empty() {
            return Err(AppError::Msg("GitHub 图床配置不完整".into()));
        }
        let branch = {
            let b = cfg_str(cfg, "branch");
            if b.is_empty() { "main".to_string() } else { b }
        };
        let sub = dir.trim_matches('/');
        let api = if sub.is_empty() {
            format!("https://api.github.com/repos/{owner}/{repo}/contents?ref={branch}")
        } else {
            format!("https://api.github.com/repos/{owner}/{repo}/contents/{sub}?ref={branch}")
        };

        let res = client(30)?
            .get(&api)
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/vnd.github+json")
            .header("X-GitHub-Api-Version", "2022-11-28")
            .send()
            .map_err(|e| AppError::Msg(format!("GitHub 请求失败: {e}")))?;

        let status = res.status().as_u16();
        // 目录还不存在：当成空列表，比报错友好（用户可能刚配好还没传过）
        if status == 404 {
            return Ok(Vec::new());
        }
        let text = res.text().unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(http_err(status, &text));
        }

        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or_default();
        let mut out = Vec::new();
        if let Some(items) = parsed.as_array() {
            for it in items {
                if it["type"].as_str() != Some("file") {
                    continue;
                }
                let name = it["name"].as_str().unwrap_or_default().to_string();
                if name.is_empty() || !is_image_name(&name) {
                    continue;
                }
                let path = it["path"].as_str().unwrap_or(&name).to_string();
                out.push(RemoteImage {
                    url: github_public_url(cfg, &path)?,
                    name,
                    path,
                    size: it["size"].as_u64().unwrap_or(0),
                    modified_at: None,
                    sha: it["sha"].as_str().map(|s| s.to_string()),
                });
            }
        }
        out.sort_by(|a, b| b.name.cmp(&a.name));
        out.truncate(limit.max(1));
        Ok(out)
    }
}

/// 仓库里的文件怎么访问。上传和浏览都要用，抽出来免得两处逻辑漂移。
fn github_public_url(cfg: &Cfg, key: &str) -> AppResult<String> {
    let owner = cfg_str(cfg, "owner");
    let repo = cfg_str(cfg, "repo");
    let branch = {
        let b = cfg_str(cfg, "branch");
        if b.is_empty() { "main".to_string() } else { b }
    };
    let raw = format!("https://raw.githubusercontent.com/{owner}/{repo}/{branch}/{key}");

    Ok(match cfg_str(cfg, "cdn").as_str() {
        "raw" => raw,
        // 第三方代理：把 raw 地址整个套进它们的转发前缀
        "ghproxy" => format!("https://gh-proxy.com/{raw}"),
        "statically" => format!("https://cdn.statically.io/gh/{owner}/{repo}/{branch}/{key}"),
        "custom" => {
            let tpl = cfg_str(cfg, "customDomain");
            let tpl = tpl.trim();
            if tpl.is_empty() {
                return Err(AppError::Msg("选了自定义加速链接，但没填地址".into()));
            }
            if tpl.contains('{') {
                // 模板模式：{owner} {repo} {branch} {path} {file}
                // 用它可以把任意加速源的 URL 规则搬过来，也能只替换掉其中一段。
                let file = key.rsplit('/').next().unwrap_or(key);
                tpl.replace("{owner}", &owner)
                    .replace("{repo}", &repo)
                    .replace("{branch}", &branch)
                    .replace("{path}", key)
                    .replace("{file}", file)
            } else {
                // 纯前缀模式：老配置就是这个写法（填域名、后面自动拼路径），保持兼容
                format!("{}/{}", tpl.trim_end_matches('/'), key)
            }
        }
        _ => format!("https://cdn.jsdelivr.net/gh/{owner}/{repo}@{branch}/{key}"),
    })
}

/* ============================ S3 兼容 ============================ */

pub struct S3Provider;

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    format!("{:x}", h.finalize())
}

fn hmac(key: &[u8], data: &[u8]) -> Vec<u8> {
    let mut mac = HmacSha256::new_from_slice(key).expect("HMAC 密钥长度任意");
    mac.update(data);
    mac.finalize().into_bytes().to_vec()
}

/// SigV4 要求的 URI 编码：保留 '/'，其余按 RFC3986
fn uri_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 2);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' | b'/' => {
                out.push(byte as char)
            }
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

impl Provider for S3Provider {
    fn meta(&self) -> ProviderMeta {
        ProviderMeta {
            id: "s3".into(),
            name: "S3 兼容存储".into(),
            origin: "builtin".into(),
            description: Some("AWS S3 / Cloudflare R2 / MinIO / B2 / 七牛 / OSS / COS 的 S3 端点".into()),
            requires_network: true,
            supports_dir: true,
            can_browse: false,
            can_list_images: false,
            fields: vec![
                hint(
                    required(f("endpoint", "Endpoint", "string")),
                    "不带桶名，例如 https://<account>.r2.cloudflarestorage.com",
                ),
                hint(required(f("region", "Region", "string")), "R2 填 auto，AWS 填真实区域"),
                required(f("bucket", "Bucket", "string")),
                required(f("accessKeyId", "Access Key ID", "string")),
                required(f("secretAccessKey", "Secret Access Key", "password")),
                with_default(f("pathPrefix", "路径前缀", "string"), serde_json::json!("snipforge")),
                hint(
                    required(f("publicBaseUrl", "公开访问域名", "string")),
                    "上传成功后的访问前缀，末尾不要斜杠",
                ),
                with_default(f("acl", "对象 ACL", "select"), serde_json::json!("none"))
                    .pipe_options(&[("不设置（用桶策略）", "none"), ("public-read", "public-read")]),
            ],
        }
    }

    fn upload(&self, file: &UploadFile, cfg: &Cfg) -> AppResult<UploadResult> {
        let endpoint = cfg_str(cfg, "endpoint").trim_end_matches('/').to_string();
        let bucket = cfg_str(cfg, "bucket");
        let region = cfg_str(cfg, "region");
        let ak = cfg_str(cfg, "accessKeyId");
        let sk = cfg_str(cfg, "secretAccessKey");

        if endpoint.is_empty() || bucket.is_empty() || ak.is_empty() || sk.is_empty() {
            return Err(AppError::Msg("S3 配置不完整".into()));
        }

        let prefix = file
            .dir
            .clone()
            .unwrap_or_else(|| cfg_str(cfg, "pathPrefix"))
            .trim_matches('/')
            .to_string();
        // 没指定目录时才按年月自动分层（S3 没有真目录，前缀就是目录）
        let date_dir = if prefix.is_empty() && file.dir.is_none() {
            chrono::Utc::now().format("%Y/%m").to_string()
        } else {
            String::new()
        };
        let key = match (prefix.is_empty(), date_dir.is_empty()) {
            (true, true) => file.file_name.clone(),
            (true, false) => format!("{date_dir}/{}", file.file_name),
            (false, true) => format!("{prefix}/{}", file.file_name),
            (false, false) => format!("{prefix}/{date_dir}/{}", file.file_name),
        };

        let host = endpoint
            .trim_start_matches("https://")
            .trim_start_matches("http://")
            .split('/')
            .next()
            .unwrap_or_default()
            .to_string();

        // 统一走 path-style：MinIO/七牛/R2 都认，AWS 也支持
        let canonical_uri = format!("/{}/{}", bucket, uri_encode(&key));

        let now = chrono::Utc::now();
        let amz_date = now.format("%Y%m%dT%H%M%SZ").to_string();
        let date_stamp = now.format("%Y%m%d").to_string();
        let payload_hash = sha256_hex(&file.buffer);

        let mut headers: Vec<(String, String)> = vec![
            ("host".into(), host.clone()),
            ("x-amz-content-sha256".into(), payload_hash.clone()),
            ("x-amz-date".into(), amz_date.clone()),
            ("content-type".into(), file.mime_type.clone()),
        ];
        let acl = cfg_str(cfg, "acl");
        if acl != "none" && !acl.is_empty() {
            headers.push(("x-amz-acl".into(), acl));
        }
        headers.sort_by(|a, b| a.0.cmp(&b.0));

        let signed_headers = headers
            .iter()
            .map(|(k, _)| k.as_str())
            .collect::<Vec<_>>()
            .join(";");
        let canonical_headers = headers
            .iter()
            .map(|(k, v)| format!("{k}:{}\n", v.trim()))
            .collect::<String>();

        let canonical_request = format!(
            "PUT\n{canonical_uri}\n\n{canonical_headers}\n{signed_headers}\n{payload_hash}"
        );

        let scope = format!("{date_stamp}/{region}/s3/aws4_request");
        let string_to_sign = format!(
            "AWS4-HMAC-SHA256\n{amz_date}\n{scope}\n{}",
            sha256_hex(canonical_request.as_bytes())
        );

        let k_date = hmac(format!("AWS4{sk}").as_bytes(), date_stamp.as_bytes());
        let k_region = hmac(&k_date, region.as_bytes());
        let k_service = hmac(&k_region, b"s3");
        let k_signing = hmac(&k_service, b"aws4_request");
        let signature = hex(&hmac(&k_signing, string_to_sign.as_bytes()));

        let authorization = format!(
            "AWS4-HMAC-SHA256 Credential={ak}/{scope}, SignedHeaders={signed_headers}, Signature={signature}"
        );

        let url = format!("{endpoint}{canonical_uri}");
        let res = client(60)?
            .put(&url)
            .header("Authorization", authorization)
            .header("x-amz-content-sha256", &payload_hash)
            .header("x-amz-date", &amz_date)
            .header("Content-Type", &file.mime_type)
            .header("Host", &host)
            .body(file.buffer.clone())
            .send()
            .map_err(|e| AppError::Msg(format!("S3 请求失败: {e}")))?;

        let status = res.status().as_u16();
        if !(200..300).contains(&status) {
            let body = res.text().unwrap_or_default();
            return Err(http_err(status, &body));
        }

        let base = {
            let b = cfg_str(cfg, "publicBaseUrl");
            if b.is_empty() { endpoint } else { b }
        };

        Ok(UploadResult {
            url: format!("{}/{key}", base.trim_end_matches('/')),
            delete_url: None,
            thumb_url: None,
            meta: Some(serde_json::json!({ "bucket": bucket, "key": key })),
        })
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// 小工具：给 select 字段补 options（链式调用用）
trait PipeOptions {
    fn pipe_options(self, opts: &[(&str, &str)]) -> Self;
}

impl PipeOptions for ConfigField {
    fn pipe_options(self, opts: &[(&str, &str)]) -> Self {
        options(self, opts)
    }
}

/* ============================ SM.MS ============================ */

pub struct SmmsProvider;

impl Provider for SmmsProvider {
    fn meta(&self) -> ProviderMeta {
        ProviderMeta {
            id: "smms".into(),
            name: "SM.MS".into(),
            origin: "builtin".into(),
            description: Some("免费公共图床，注册后在 API Token 页面生成令牌".into()),
            requires_network: true,
            supports_dir: false,
            can_browse: false,
            can_list_images: false,
            fields: vec![hint(
                required(f("token", "API Token", "password")),
                "在 sm.ms/home/apitoken 获取",
            )],
        }
    }

    fn upload(&self, file: &UploadFile, cfg: &Cfg) -> AppResult<UploadResult> {
        let token = cfg_str(cfg, "token");
        if token.is_empty() {
            return Err(AppError::Msg("请先填写 SM.MS API Token".into()));
        }

        let part = reqwest::blocking::multipart::Part::bytes(file.buffer.clone())
            .file_name(file.file_name.clone())
            .mime_str(&file.mime_type)
            .map_err(|e| AppError::Msg(e.to_string()))?;
        let form = reqwest::blocking::multipart::Form::new().part("smfile", part);

        let res = client(40)?
            .post("https://sm.ms/api/v2/upload")
            .header("Authorization", token)
            .multipart(form)
            .send()
            .map_err(|e| AppError::Msg(format!("SM.MS 请求失败: {e}")))?;

        let text = res.text().unwrap_or_default();
        let parsed: serde_json::Value = serde_json::from_str(&text)
            .map_err(|_| AppError::Msg(format!("SM.MS 返回非 JSON: {}", text.chars().take(150).collect::<String>())))?;

        // 同一张图重复上传会返回 code=image_repeated
        if parsed["code"].as_str() == Some("image_repeated") {
            if let Some(url) = parsed["images"].as_str() {
                return Ok(UploadResult {
                    url: url.to_string(),
                    delete_url: None,
                    thumb_url: None,
                    meta: None,
                });
            }
        }

        if parsed["success"].as_bool() != Some(true) {
            let msg = parsed["message"].as_str().unwrap_or("未知错误");
            return Err(AppError::Msg(format!("SM.MS 上传失败: {msg}")));
        }

        Ok(UploadResult {
            url: parsed["data"]["url"].as_str().unwrap_or_default().to_string(),
            delete_url: parsed["data"]["delete"].as_str().map(|s| s.to_string()),
            thumb_url: None,
            meta: Some(parsed["data"].clone()),
        })
    }
}

/* ============================ 自定义 HTTP ============================ */

pub struct CustomHttpProvider;

impl Provider for CustomHttpProvider {
    fn meta(&self) -> ProviderMeta {
        ProviderMeta {
            id: "custom-http".into(),
            name: "自定义 HTTP 接口".into(),
            origin: "builtin".into(),
            description: Some("对接自己的上传接口，支持 multipart / JSON-Base64".into()),
            requires_network: true,
            supports_dir: false,
            can_browse: false,
            can_list_images: false,
            fields: vec![
                required(f("url", "上传地址", "string")),
                with_default(
                    options(f("mode", "提交方式", "select"), &[
                        ("multipart/form-data", "multipart"),
                        ("JSON + Base64", "json"),
                    ]),
                    serde_json::json!("multipart"),
                ),
                with_default(f("fieldName", "文件字段名", "string"), serde_json::json!("file")),
                hint(f("extraHeaders", "额外请求头 (JSON)", "textarea"), r#"例如 {"X-Api-Key":"xxx"}"#),
                f("extraFields", "额外表单字段 (JSON)", "textarea"),
                with_default(f("responsePath", "URL 取值路径", "string"), serde_json::json!("data.url")),
                hint(f("urlPrefix", "URL 前缀", "string"), "接口只返回相对路径时使用"),
            ],
        }
    }

    fn upload(&self, file: &UploadFile, cfg: &Cfg) -> AppResult<UploadResult> {
        let url = cfg_str(cfg, "url");
        if url.is_empty() {
            return Err(AppError::Msg("请填写上传地址".into()));
        }

        let field_name = {
            let n = cfg_str(cfg, "fieldName");
            if n.is_empty() { "file".to_string() } else { n }
        };
        let extra_headers = parse_json_object(cfg.get("extraHeaders"));
        let extra_fields = parse_json_object(cfg.get("extraFields"));

        let client = client(50)?;
        let mut req = if cfg_str(cfg, "mode") == "json" {
            let mut body = serde_json::Map::new();
            for (k, v) in &extra_fields {
                body.insert(k.clone(), v.clone());
            }
            body.insert(
                field_name,
                serde_json::Value::String({
                    use base64::Engine;
                    base64::engine::general_purpose::STANDARD.encode(&file.buffer)
                }),
            );
            body.insert("fileName".into(), serde_json::json!(file.file_name));
            body.insert("mimeType".into(), serde_json::json!(file.mime_type));
            client.post(&url).json(&serde_json::Value::Object(body))
        } else {
            let mut form = reqwest::blocking::multipart::Form::new();
            for (k, v) in &extra_fields {
                form = form.text(k.clone(), v.as_str().unwrap_or_default().to_string());
            }
            let part = reqwest::blocking::multipart::Part::bytes(file.buffer.clone())
                .file_name(file.file_name.clone())
                .mime_str(&file.mime_type)
                .map_err(|e| AppError::Msg(e.to_string()))?;
            client.post(&url).multipart(form.part(field_name, part))
        };

        for (k, v) in extra_headers {
            req = req.header(k, v.as_str().unwrap_or_default().to_string());
        }

        let res = req.send().map_err(|e| AppError::Msg(format!("请求失败: {e}")))?;
        let status = res.status().as_u16();
        let text = res.text().unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(http_err(status, &text));
        }

        // 后端直接吐 URL 的情况
        let trimmed = text.trim();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return Ok(UploadResult {
                url: trimmed.to_string(),
                delete_url: None,
                thumb_url: None,
                meta: None,
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(trimmed)
            .map_err(|_| AppError::Msg(format!("响应无法解析: {}", trimmed.chars().take(200).collect::<String>())))?;

        let path = cfg_str(cfg, "responsePath");
        let mut final_url = if path.is_empty() {
            None
        } else {
            pick_path(&parsed, &path)
        };
        if final_url.is_none() {
            final_url = find_first_url(&parsed);
        }
        let mut final_url = final_url
            .ok_or_else(|| AppError::Msg("未能从响应中取到 URL，请检查「URL 取值路径」".into()))?;

        let prefix = cfg_str(cfg, "urlPrefix");
        if !prefix.is_empty() && !final_url.starts_with("http") {
            final_url = format!(
                "{}/{}",
                prefix.trim_end_matches('/'),
                final_url.trim_start_matches('/')
            );
        }

        Ok(UploadResult {
            url: final_url,
            delete_url: None,
            thumb_url: None,
            meta: Some(parsed),
        })
    }
}

fn parse_json_object(value: Option<&serde_json::Value>) -> Vec<(String, serde_json::Value)> {
    match value {
        Some(serde_json::Value::Object(map)) => map.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
        Some(serde_json::Value::String(s)) if !s.trim().is_empty() => {
            serde_json::from_str::<serde_json::Value>(s)
                .ok()
                .and_then(|v| v.as_object().cloned())
                .map(|m| m.into_iter().collect())
                .unwrap_or_default()
        }
        _ => Vec::new(),
    }
}

fn pick_path(root: &serde_json::Value, path: &str) -> Option<String> {
    let mut cur = root;
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        // 支持 arr[0] 形式
        if let Some((key, idx)) = segment.split_once('[') {
            let index: usize = idx.trim_end_matches(']').parse().ok()?;
            cur = cur.get(key)?.get(index)?;
        } else {
            cur = cur.get(segment)?;
        }
    }
    cur.as_str().map(|s| s.to_string())
}

fn find_first_url(node: &serde_json::Value) -> Option<String> {
    match node {
        serde_json::Value::String(s) if s.starts_with("http") => Some(s.clone()),
        serde_json::Value::Array(items) => items.iter().find_map(find_first_url),
        serde_json::Value::Object(map) => map.values().find_map(find_first_url),
        _ => None,
    }
}

/* ============================ 本地目录 ============================ */

pub struct LocalFolderProvider;

impl Provider for LocalFolderProvider {
    fn meta(&self) -> ProviderMeta {
        ProviderMeta {
            id: "local-folder".into(),
            name: "本地目录".into(),
            origin: "builtin".into(),
            description: Some("存到指定文件夹，返回 file:// 链接（配同步盘可当私有图床）".into()),
            requires_network: false,
            supports_dir: true,
            can_browse: true,
            can_list_images: true,
            fields: vec![
                required(f("dir", "目标目录", "string")),
                hint(f("publicBaseUrl", "对外访问前缀", "string"), "留空则返回本地 file:// 路径"),
            ],
        }
    }

    fn upload(&self, file: &UploadFile, cfg: &Cfg) -> AppResult<UploadResult> {
        let root = cfg_str(cfg, "dir");
        if root.is_empty() {
            return Err(AppError::Msg("请设置目标目录".into()));
        }
        // 子目录和文件名都来自前端，必须走安全拼接：
        // 否则 `..\..\Windows\Temp` 能把文件写到任意位置。
        let dir = safe_join(&root, file.dir.as_deref().unwrap_or(""))?;
        let name = safe_file_name(&file.file_name)?;
        std::fs::create_dir_all(&dir)?;
        let target = dir.join(&name);
        std::fs::write(&target, &file.buffer)?;

        let base = cfg_str(cfg, "publicBaseUrl");
        let url = if base.is_empty() {
            format!("file:///{}", target.to_string_lossy().replace('\\', "/"))
        } else {
            format!(
                "{}/{}",
                base.trim_end_matches('/'),
                urlencoding::encode(&name)
            )
        };

        Ok(UploadResult {
            url,
            delete_url: None,
            thumb_url: None,
            meta: Some(serde_json::json!({ "path": target.to_string_lossy() })),
        })
    }

    /// 列根目录下某个子目录里的目录。`path` 为空 = 列根目录本身。
    fn list_dirs(&self, cfg: &Cfg, path: &str) -> AppResult<Vec<DirEntry>> {
        let root = cfg_str(cfg, "dir");
        if root.is_empty() {
            return Err(AppError::Msg("请先设置目标目录".into()));
        }
        let sub_raw = path.trim().replace('\\', "/");
        let sub = sub_raw.trim_matches('/');
        let abs = safe_join(&root, sub)?;
        if !abs.exists() {
            // 目录还不存在 = 用户想新建，返回空列表而不是报错
            return Ok(Vec::new());
        }

        let mut out = Vec::new();
        for entry in std::fs::read_dir(&abs)?.flatten() {
            if !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let rel = if sub.is_empty() {
                name.clone()
            } else {
                format!("{sub}/{name}")
            };
            out.push(DirEntry { name, path: rel });
        }
        out.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(out)
    }

    /// 列目录下的图片。
    ///
    /// 本地图床最实用的就是它：不用开资源管理器翻，直接在列表里看缩略图、
    /// 复制链接。按修改时间倒序 —— 本地文件有真实 mtime，新的就该在最前面。
    fn list_images(&self, cfg: &Cfg, dir: &str, limit: usize) -> AppResult<Vec<RemoteImage>> {
        let root = cfg_str(cfg, "dir");
        if root.is_empty() {
            return Err(AppError::Msg("请先设置目标目录".into()));
        }
        let sub_raw = dir.trim().replace('\\', "/");
        let sub = sub_raw.trim_matches('/');
        let abs = safe_join(&root, sub)?;
        if !abs.exists() {
            return Ok(Vec::new());
        }

        let base = cfg_str(cfg, "publicBaseUrl");
        let mut out = Vec::new();
        for entry in std::fs::read_dir(&abs)?.flatten() {
            if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') || !is_image_name(&name) {
                continue;
            }
            let full = entry.path();
            let meta = entry.metadata().ok();
            let rel = if sub.is_empty() {
                name.clone()
            } else {
                format!("{sub}/{name}")
            };
            // 配了对外前缀就拼 URL（同步盘场景），否则给 file:// 本地路径
            let url = if base.is_empty() {
                format!("file:///{}", full.to_string_lossy().replace('\\', "/"))
            } else {
                let encoded: Vec<String> = rel
                    .split('/')
                    .map(|seg| urlencoding::encode(seg).to_string())
                    .collect();
                format!("{}/{}", base.trim_end_matches('/'), encoded.join("/"))
            };
            out.push(RemoteImage {
                name,
                path: rel,
                url,
                size: meta.as_ref().map(|m| m.len()).unwrap_or(0),
                modified_at: meta
                    .as_ref()
                    .and_then(|m| m.modified().ok())
                    .map(ms_from_system),
                sha: None,
            });
        }
        out.sort_by(|a, b| b.modified_at.unwrap_or(0).cmp(&a.modified_at.unwrap_or(0)));
        out.truncate(limit.max(1));
        Ok(out)
    }
}

/* ============================ 声明式 provider ============================ */

/// 从 JSON 定义加载的图床。
///
/// 这是给第三方扩展留的"零代码"通道：用户在 plugins/ 里放一个
/// `xxx.uploader.json`，声明 URL、字段名、响应取值路径，就能接入任意 HTTP 图床。
/// 不需要编译、不需要写 Rust，也不需要信任第三方二进制。
pub struct DeclarativeProvider {
    pub meta: ProviderMeta,
    /// 完整定义（含模板）
    pub spec: serde_json::Value,
}

impl Provider for DeclarativeProvider {
    fn meta(&self) -> ProviderMeta {
        self.meta.clone()
    }

    fn upload(&self, file: &UploadFile, cfg: &Cfg) -> AppResult<UploadResult> {
        let spec = &self.spec;
        let url = spec["url"].as_str().unwrap_or_default();
        if url.is_empty() {
            return Err(AppError::Msg("声明式图床缺少 url".into()));
        }

        let method = spec["method"].as_str().unwrap_or("POST").to_uppercase();
        let field_name = spec["fieldName"].as_str().unwrap_or("file");
        let response_path = spec["responsePath"].as_str().unwrap_or("");
        let prefix = spec["urlPrefix"].as_str().unwrap_or("");

        let client = client(50)?;
        let mut req = match method.as_str() {
            "PUT" => client.put(url),
            _ => client.post(url),
        };

        // header 里可以用 {{token}} 引用配置值
        if let Some(headers) = spec["headers"].as_object() {
            for (k, v) in headers {
                let raw = v.as_str().unwrap_or_default();
                req = req.header(k, interpolate(raw, cfg));
            }
        }

        req = req.multipart({
            let part = reqwest::blocking::multipart::Part::bytes(file.buffer.clone())
                .file_name(file.file_name.clone())
                .mime_str(&file.mime_type)
                .map_err(|e| AppError::Msg(e.to_string()))?;
            let mut form = reqwest::blocking::multipart::Form::new();
            if let Some(fields) = spec["fields"].as_object() {
                for (k, v) in fields {
                    form = form.text(k.clone(), interpolate(v.as_str().unwrap_or_default(), cfg));
                }
            }
            form.part(field_name.to_string(), part)
        });

        let res = req.send().map_err(|e| AppError::Msg(format!("请求失败: {e}")))?;
        let status = res.status().as_u16();
        let text = res.text().unwrap_or_default();
        if !(200..300).contains(&status) {
            return Err(http_err(status, &text));
        }

        // 有些服务（0x0.st、Catbox）直接回一行 URL 而不是 JSON
        let trimmed = text.trim();
        if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
            return Ok(UploadResult {
                url: trimmed.to_string(),
                delete_url: None,
                thumb_url: None,
                meta: None,
            });
        }

        let parsed: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::Null);
        let mut final_url = if response_path.is_empty() {
            None
        } else {
            pick_path(&parsed, response_path)
        };
        if final_url.is_none() {
            final_url = find_first_url(&parsed);
        }
        let final_url = final_url
            .ok_or_else(|| AppError::Msg("响应里没有可用的 URL".into()))?;

        let url = if !prefix.is_empty() && !final_url.starts_with("http") {
            format!("{}/{}", prefix.trim_end_matches('/'), final_url.trim_start_matches('/'))
        } else {
            final_url
        };

        Ok(UploadResult {
            url,
            delete_url: None,
            thumb_url: None,
            meta: Some(parsed),
        })
    }
}

/// 把 {{key}} 替换成配置值
fn interpolate(template: &str, cfg: &Cfg) -> String {
    let mut out = template.to_string();
    for (k, v) in cfg {
        let token = format!("{{{{{k}}}}}");
        if out.contains(&token) {
            let value = v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string());
            out = out.replace(&token, &value);
        }
    }
    out
}

#[cfg(test)]
mod url_tests {
    use super::*;
    use serde_json::json;

    fn cfg(pairs: &[(&str, &str)]) -> Cfg {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), json!(v)))
            .collect()
    }

    /// 一套典型配置：owner / repo / branch 都有，额外项按需覆盖
    fn base(extra: &[(&str, &str)]) -> Cfg {
        let mut pairs = vec![("owner", "adream2"), ("repo", "pics"), ("branch", "master")];
        pairs.extend_from_slice(extra);
        cfg(&pairs)
    }

    const KEY: &str = "snipforge/a.png";

    #[test]
    fn default_is_jsdelivr() {
        assert_eq!(
            github_public_url(&base(&[]), KEY).unwrap(),
            "https://cdn.jsdelivr.net/gh/adream2/pics@master/snipforge/a.png"
        );
    }

    #[test]
    fn raw_and_mirrors() {
        assert_eq!(
            github_public_url(&base(&[("cdn", "raw")]), KEY).unwrap(),
            "https://raw.githubusercontent.com/adream2/pics/master/snipforge/a.png"
        );
        assert_eq!(
            github_public_url(&base(&[("cdn", "statically")]), KEY).unwrap(),
            "https://cdn.statically.io/gh/adream2/pics/master/snipforge/a.png"
        );
        assert_eq!(
            github_public_url(&base(&[("cdn", "ghproxy")]), KEY).unwrap(),
            "https://gh-proxy.com/https://raw.githubusercontent.com/adream2/pics/master/snipforge/a.png"
        );
    }

    /// 老配置填的是纯前缀，行为不能变
    #[test]
    fn custom_prefix_keeps_old_behaviour() {
        let c = base(&[("cdn", "custom"), ("customDomain", "https://img.example.com/")]);
        assert_eq!(
            github_public_url(&c, KEY).unwrap(),
            "https://img.example.com/snipforge/a.png"
        );
    }

    #[test]
    fn custom_template_substitutes_placeholders() {
        let c = base(&[
            ("cdn", "custom"),
            ("customDomain", "https://x.com/{owner}/{repo}@{branch}/{path}"),
        ]);
        assert_eq!(
            github_public_url(&c, KEY).unwrap(),
            "https://x.com/adream2/pics@master/snipforge/a.png"
        );

        let c2 = base(&[("cdn", "custom"), ("customDomain", "https://x.com/img/{file}")]);
        assert_eq!(
            github_public_url(&c2, KEY).unwrap(),
            "https://x.com/img/a.png"
        );
    }

    #[test]
    fn custom_without_value_errors() {
        let c = base(&[("cdn", "custom"), ("customDomain", "   ")]);
        assert!(github_public_url(&c, KEY).is_err());
    }
}
