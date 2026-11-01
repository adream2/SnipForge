//! PaddleOCR 外部引擎接入。
//!
//! 走 [PaddleOCR-json](https://github.com/hiroi-sora/PaddleOCR-json) 这个项目的
//! 可执行文件 —— 它把 PaddleOCR 打包成一个"喂图片路径、吐 JSON"的小程序，
//! 不用我们自己去接 ONNX runtime 和三层模型文件。
//!
//! 调用方式是**一次性命令行**而不是常驻进程：OCR 是低频操作，
//! 为此维护一个常驻子进程的生命周期（僵死、内存泄漏、版本不匹配）不值当。
//! 代价是每次多 ~300ms 启动，用户感知不到。

use crate::error::{AppError, AppResult};
use crate::types::{OcrLine, OcrResult, OcrWord};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

/// 不弹黑窗
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 在几个固定位置找引擎可执行文件。
///
/// 顺序：用户显式配置 > 用户数据目录（推荐放置处）> 程序目录旁边（便携用法）
pub fn find_exe(configured: &str) -> Option<PathBuf> {
    if !configured.trim().is_empty() {
        let p = PathBuf::from(configured.trim());
        if p.is_file() {
            return Some(p);
        }
    }

    let mut candidates = Vec::new();

    if let Some(data) = dirs::data_dir() {
        let root = data.join("fun.lxzt.snipforge").join("ocr-engines");
        candidates.push(root.join("PaddleOCR-json.exe"));
        candidates.push(root.join("PaddleOCR-json").join("PaddleOCR-json.exe"));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("ocr-engines").join("PaddleOCR-json.exe"));
            candidates.push(dir.join("PaddleOCR-json.exe"));
        }
    }

    // 开发模式：仓库根目录
    if cfg!(debug_assertions) {
        let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Some(parent) = repo.parent() {
            candidates.push(parent.join("ocr-engines").join("PaddleOCR-json.exe"));
        }
    }

    candidates.into_iter().find(|p| p.is_file())
}

fn write_temp_png(png: &[u8]) -> AppResult<PathBuf> {
    let dir = std::env::temp_dir().join("snipforge").join("ocr");
    std::fs::create_dir_all(&dir)?;
    let file = dir.join(format!(
        "ocr-{}-{}.png",
        chrono::Utc::now().timestamp_millis(),
        std::process::id()
    ));
    std::fs::write(&file, png)?;
    Ok(file)
}

pub fn recognize(png: &[u8], exe: &Path, timeout_secs: u64) -> AppResult<OcrResult> {
    let started = std::time::Instant::now();
    let image_path = write_temp_png(png)?;

    let result = run_engine(exe, &image_path, timeout_secs);

    // 临时文件一定要删，OCR 跑得勤的话这里会攒出几百 MB
    let _ = std::fs::remove_file(&image_path);
    let raw = result?;

    // 不同版本的 PaddleOCR-json 字段略有出入（box 可能带 score 也可能不带），
    // 用宽松的 JSON 取值而不是强类型结构体，省得换个版本就解析失败
    let parsed: serde_json::Value = serde_json::from_str(&raw).map_err(|e| {
        AppError::Msg(format!(
            "PaddleOCR 返回无法解析: {e}；原始输出: {}",
            raw.chars().take(200).collect::<String>()
        ))
    })?;

    let code = parsed["code"].as_i64().unwrap_or(-1);
    // 100 = 识别到内容，101 = 图中无文字，其它为错误码
    if code != 100 && code != 101 {
        return Err(AppError::Msg(format!(
            "PaddleOCR 返回错误码 {code}：{}",
            raw.chars().take(200).collect::<String>()
        )));
    }

    let empty = Vec::new();
    let data = parsed["data"].as_array().unwrap_or(&empty);

    let mut lines: Vec<OcrLine> = Vec::with_capacity(data.len());
    for item in data {
        let text = item["text"].as_str().unwrap_or("").to_string();
        if text.trim().is_empty() {
            continue;
        }
        let bbox = item["box"]
            .as_array()
            .and_then(|arr| {
                let pts: Vec<[f32; 2]> = arr
                    .iter()
                    .filter_map(|p| {
                        let pair = p.as_array()?;
                        Some([pair.first()?.as_f64()? as f32, pair.get(1)?.as_f64()? as f32])
                    })
                    .collect();
                bbox_of(&pts)
            });

        lines.push(OcrLine {
            text: text.clone(),
            words: bbox
                .map(|(x, y, w, h)| {
                    vec![OcrWord {
                        text,
                        x,
                        y,
                        width: w,
                        height: h,
                    }]
                })
                .unwrap_or_default(),
        });
    }

    // Paddle 是按检测框返回的，按从上到下、从左到右排一下更像人读的顺序
    lines.sort_by(|a, b| {
        let ay = a.words.first().map(|w| w.y).unwrap_or(0.0);
        let by = b.words.first().map(|w| w.y).unwrap_or(0.0);
        ay.partial_cmp(&by).unwrap_or(std::cmp::Ordering::Equal)
    });

    Ok(OcrResult {
        text: lines
            .iter()
            .map(|l| l.text.as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        lines,
        language: "paddle".into(),
        duration_ms: started.elapsed().as_millis() as u64,
    })
}

fn bbox_of(points: &[[f32; 2]]) -> Option<(f32, f32, f32, f32)> {
    if points.is_empty() {
        return None;
    }
    let xs: Vec<f32> = points.iter().map(|p| p[0]).collect();
    let ys: Vec<f32> = points.iter().map(|p| p[1]).collect();
    let min_x = xs.iter().cloned().fold(f32::MAX, f32::min);
    let max_x = xs.iter().cloned().fold(f32::MIN, f32::max);
    let min_y = ys.iter().cloned().fold(f32::MAX, f32::min);
    let max_y = ys.iter().cloned().fold(f32::MIN, f32::max);
    Some((min_x, min_y, max_x - min_x, max_y - min_y))
}

fn run_engine(exe: &Path, image_path: &Path, timeout_secs: u64) -> AppResult<String> {
    use std::io::Read;

    let mut cmd = Command::new(exe);
    cmd.arg(format!("-image_path={}", image_path.to_string_lossy()))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());

    #[cfg(windows)]
    cmd.creation_flags(CREATE_NO_WINDOW);

    let mut child = cmd
        .spawn()
        .map_err(|e| AppError::Msg(format!("启动 PaddleOCR 失败: {e}")))?;

    // **必须一边等一边读管道**。
    //
    // 以前是「先 try_wait 轮询，退出后再 wait_with_output」，这有个必死的坑：
    // 子进程输出超过管道缓冲（Windows 约 64KB）就会阻塞在 write 上，
    // 于是它永远不退出、我们永远等不到，最后只能按"超时"杀掉。
    // 文字多的截图 JSON 轻易超过 64KB —— 也就是说图越大越必然失败。
    let mut out_pipe = child
        .stdout
        .take()
        .ok_or_else(|| AppError::Msg("无法读取 PaddleOCR 输出".into()))?;
    let mut err_pipe = child
        .stderr
        .take()
        .ok_or_else(|| AppError::Msg("无法读取 PaddleOCR 错误输出".into()))?;

    let out_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = out_pipe.read_to_end(&mut buf);
        buf
    });
    let err_reader = std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = err_pipe.read_to_end(&mut buf);
        buf
    });

    // 模型加载冷启动要几秒，第一次跑给足时间；卡死时也不能把应用拖住
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs.max(5));
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(AppError::Msg(format!(
                        "PaddleOCR 超过 {timeout_secs} 秒未返回，已终止"
                    )));
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            Err(e) => return Err(AppError::Msg(format!("等待 PaddleOCR 失败: {e}"))),
        }
    }

    // 进程退出 → 管道 EOF → 两个读线程自然结束
    let stdout = String::from_utf8_lossy(&out_reader.join().unwrap_or_default()).to_string();
    if stdout.trim().is_empty() {
        let stderr = String::from_utf8_lossy(&err_reader.join().unwrap_or_default()).to_string();
        return Err(AppError::Msg(format!(
            "PaddleOCR 没有输出。stderr: {}",
            stderr.chars().take(300).collect::<String>()
        )));
    }
    Ok(stdout)
}


/* ============================ 一键下载安装 ============================ */

/// 下载进度事件载荷
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DownloadProgress {
    /// resolving | downloading | extracting | done
    pub stage: String,
    pub downloaded: u64,
    pub total: u64,
    pub message: String,
}

const RELEASE_API: &str = "https://api.github.com/repos/hiroi-sora/PaddleOCR-json/releases/latest";

/// 内置加速镜像（"前缀 + 原 URL"形式）。
///
/// 国内直连 GitHub Releases 经常几 KB/s 甚至断开，而这包有 88MB。
/// 这里按可用性排序，**任何一个失败都会自动跳到下一个**，用户不用管。
/// 这类公益镜像会失效，所以：失败了能继续fallback，而且允许用户自己填。
pub const MIRROR_PREFIXES: &[&str] = &[
    "https://ghfast.top/",
    "https://gh-proxy.com/",
    "https://ghproxy.net/",
];

/// 下载源策略（配置里的 `ocr.downloadMode`）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceMode {
    /// 镜像优先，最后回落 GitHub 直连
    Auto,
    /// 只走 GitHub 直连
    Direct,
    /// 先试用户自定义前缀，再回落内置镜像与直连
    Custom,
}

impl SourceMode {
    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "direct" => SourceMode::Direct,
            "custom" => SourceMode::Custom,
            _ => SourceMode::Auto,
        }
    }
}

/// 拼镜像 URL。镜像都是 `https://xxx/` + `https://github.com/...` 的形式。
pub fn mirror_url(prefix: &str, url: &str) -> String {
    let p = prefix.trim();
    if p.is_empty() {
        return url.to_string();
    }
    // 已经是镜像形式（用户可能把完整地址粘进来）就别再套一层
    if p.contains(url) {
        return p.to_string();
    }
    let base = if p.ends_with('/') { p.to_string() } else { format!("{p}/") };
    format!("{base}{url}")
}

fn http_client() -> AppResult<reqwest::blocking::Client> {
    // OCR 包在 GitHub 上，国内基本必须走代理。系统代理开着就跟着走，
    // 这也是"下载困难"最常见的原因（浏览器能下、这里不行）。
    crate::net::with_blocking_proxy(reqwest::blocking::Client::builder()
        // 单次尝试的上限。镜像挂了要尽快跳到下一个，
        // 而不是让用户盯着"正在下载 0%"等半小时。
        .timeout(Duration::from_secs(600))
        // 连不上（DNS/端口不通）是最常见的失败，8 秒足够判定
        .connect_timeout(Duration::from_secs(8))
        .user_agent("SnipForge/0.1 (screenshot tool)"))
    .build()
    .map_err(|e| AppError::Msg(format!("HTTP 客户端创建失败: {e}")))
}

/// 在目录里递归找 PaddleOCR-json.exe
fn find_exe_in(dir: &Path) -> Option<PathBuf> {
    let direct = dir.join("PaddleOCR-json.exe");
    if direct.is_file() {
        return Some(direct);
    }
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_exe_in(&path) {
                return Some(found);
            }
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|n| n.eq_ignore_ascii_case("PaddleOCR-json.exe"))
            .unwrap_or(false)
        {
            return Some(path);
        }
    }
    None
}

/// 从 release JSON 里挑出该下载哪个包
struct Asset {
    url: String,
    name: String,
    /// GitHub 给的文件大小，0 = 未知
    size: u64,
    /// 新版 release API 会给 `sha256:...`，有就校验
    sha256: Option<String>,
}

fn parse_asset(release: &serde_json::Value) -> AppResult<Asset> {
    let assets = release["assets"]
        .as_array()
        .ok_or_else(|| AppError::Msg("版本信息里没有可下载文件".into()))?;

    // 优先 64 位 Windows 包，避开 dev / 带 mkl 的变体
    let asset = assets
        .iter()
        .filter(|a| {
            let name = a["name"].as_str().unwrap_or("").to_lowercase();
            name.contains("windows") && name.ends_with(".7z")
        })
        .find(|a| {
            let name = a["name"].as_str().unwrap_or("").to_lowercase();
            name.contains("x64") || name.contains("x86-64")
        })
        .or_else(|| {
            assets.iter().find(|a| {
                a["name"]
                    .as_str()
                    .map(|n| n.to_lowercase().contains("windows"))
                    .unwrap_or(false)
            })
        })
        .ok_or_else(|| AppError::Msg("该版本没有 Windows 包".into()))?;

    let url = asset["browser_download_url"]
        .as_str()
        .ok_or_else(|| AppError::Msg("下载地址缺失".into()))?
        .to_string();
    let name = asset["name"].as_str().unwrap_or("paddleocr.7z").to_string();
    let size = asset["size"].as_u64().unwrap_or(0);
    let sha256 = asset["digest"]
        .as_str()
        .and_then(|d| d.strip_prefix("sha256:"))
        .map(|s| s.to_ascii_lowercase());

    Ok(Asset {
        url,
        name,
        size,
        sha256,
    })
}

/// 解析版本信息时的候选地址。
///
/// **直连排第一**，这点和下载相反：加速镜像基本只代理 `github.com` 的
/// 下载地址，不代理 `api.github.com`。把镜像排在前面只会让用户白等
/// 三次超时。API 全部失败时走 `fallback_asset()` 的内置版本。
fn api_candidates(mode: SourceMode, custom: &str) -> Vec<String> {
    let mut out: Vec<String> = vec![RELEASE_API.to_string()];
    if mode == SourceMode::Direct {
        return out;
    }
    let push_prefix = |p: &str, out: &mut Vec<String>| {
        let u = mirror_url(p, RELEASE_API);
        if !out.contains(&u) {
            out.push(u);
        }
    };
    if mode == SourceMode::Custom && !custom.trim().is_empty() {
        push_prefix(custom, &mut out);
    }
    for p in MIRROR_PREFIXES {
        push_prefix(p, &mut out);
    }
    out
}

/// 内置兜底版本。
///
/// 为什么需要它：`api.github.com` 在国内时通时不通，而镜像又不代理 API ——
/// "API 挂了但镜像能用"是很常见的状态。只有 API 一条路的话，用户会卡在
/// "查询最新版本失败"，明明下面的下载地址是通的。
/// 这里固定一个已验证可用的版本，API 失败时直接拿它的地址走镜像。
fn fallback_asset() -> Asset {
    Asset {
        url: "https://github.com/hiroi-sora/PaddleOCR-json/releases/download/v1.4.1/PaddleOCR-json_v1.4.1_windows_x64.7z".into(),
        name: "PaddleOCR-json_v1.4.1_windows_x64.7z".into(),
        size: 92_736_768,
        sha256: None,
    }
}

/// 下载时的候选地址（镜像优先，直连兜底）
fn download_candidates(mode: SourceMode, custom: &str, asset_url: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let push_prefix = |p: &str, out: &mut Vec<String>| {
        let u = mirror_url(p, asset_url);
        if !out.contains(&u) {
            out.push(u);
        }
    };
    match mode {
        SourceMode::Direct => {}
        SourceMode::Custom => {
            if !custom.trim().is_empty() {
                push_prefix(custom, &mut out);
            }
            for p in MIRROR_PREFIXES {
                push_prefix(p, &mut out);
            }
        }
        SourceMode::Auto => {
            for p in MIRROR_PREFIXES {
                push_prefix(p, &mut out);
            }
        }
    }
    out.push(asset_url.to_string());
    out
}

fn sha256_of(path: &Path) -> AppResult<String> {
    use sha2::{Digest, Sha256};
    use std::io::Read;

    let mut hasher = Sha256::new();
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

/// 把一个地址下到 `dest`，边下边报进度，结束后校验体积。
fn fetch_to_file(
    client: &reqwest::blocking::Client,
    url: &str,
    dest: &Path,
    expect_size: u64,
    on_progress: &mut impl FnMut(DownloadProgress),
) -> AppResult<()> {
    use std::io::{Read, Write};

    let mut resp = client
        .get(url)
        .send()
        .map_err(|e| AppError::Msg(format!("连接失败: {e}")))?;
    if !resp.status().is_success() {
        return Err(AppError::Msg(format!("HTTP {}", resp.status())));
    }

    let total = if expect_size > 0 {
        expect_size
    } else {
        resp.content_length().unwrap_or(0)
    };

    let mut file = std::fs::File::create(dest)?;
    let mut buf = vec![0u8; 128 * 1024];
    let mut downloaded: u64 = 0;
    let mut last_report = std::time::Instant::now();

    loop {
        let n = resp
            .read(&mut buf)
            .map_err(|e| AppError::Msg(format!("下载中断: {e}")))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])?;
        downloaded += n as u64;

        // 每 200ms 报一次，太频繁反而拖慢下载
        if last_report.elapsed() >= Duration::from_millis(200) {
            last_report = std::time::Instant::now();
            on_progress(DownloadProgress {
                stage: "downloading".into(),
                downloaded,
                total,
                message: format!(
                    "正在下载 {:.1} / {:.1} MB",
                    downloaded as f64 / 1_048_576.0,
                    total as f64 / 1_048_576.0
                ),
            });
        }
    }
    file.flush()?;
    drop(file);

    // 体积对不上 = 这个源给的可能是截断文件（镜像限流时很常见），换下一个
    if expect_size > 0 && downloaded != expect_size {
        return Err(AppError::Msg(format!(
            "文件不完整：期望 {expect_size} 字节，实际 {downloaded} 字节"
        )));
    }
    Ok(())
}

/// 解包 + 找 exe。自动下载和"手动导入"共用这一半。
fn extract_and_finish(
    archive: &Path,
    engine_dir: &Path,
    on_progress: &mut impl FnMut(DownloadProgress),
) -> AppResult<PathBuf> {
    on_progress(DownloadProgress {
        stage: "extracting".into(),
        downloaded: 0,
        total: 0,
        message: "正在解压（约 150MB，视磁盘速度需要几十秒）…".into(),
    });

    sevenz_rust::decompress_file(archive, engine_dir)
        .map_err(|e| AppError::Msg(format!("7z 解压失败: {e:?}")))?;

    let exe = find_exe_in(engine_dir).ok_or_else(|| {
        AppError::Msg(format!(
            "解压完成但没找到 PaddleOCR-json.exe，请检查 {}",
            engine_dir.display()
        ))
    })?;

    on_progress(DownloadProgress {
        stage: "done".into(),
        downloaded: 0,
        total: 0,
        message: "安装完成".into(),
    });
    Ok(exe)
}

/// 下载并安装。
///
/// 为什么自己实现而不是让用户手动下载？
/// 官方发布的是 88MB 的 7z，而 Windows 资源管理器不支持 7z ——
/// 让普通用户「去 GitHub 下 7z，再装个解压软件，解到指定目录」，
/// 这一步就劝退绝大多数人。7z 解压用纯 Rust 实现，不依赖外部工具。
///
/// 先解析版本信息（这一步同样可能被墙），再按 `mode` 挨个源试下载。
/// 任何一个源成功就停 —— 用户不需要知道最后用的是哪个镜像。
pub fn download_and_install(
    engine_dir: &Path,
    mode: SourceMode,
    custom_prefix: &str,
    mut on_progress: impl FnMut(DownloadProgress),
) -> AppResult<PathBuf> {
    std::fs::create_dir_all(engine_dir)?;
    let client = http_client()?;

    // ---- 1. 解析最新版本 ----
    on_progress(DownloadProgress {
        stage: "resolving".into(),
        downloaded: 0,
        total: 0,
        message: "正在查询最新版本…".into(),
    });

    let mut release: Option<serde_json::Value> = None;
    let mut last_err = String::new();
    for url in api_candidates(mode, custom_prefix) {
        match client.get(&url).send().and_then(|r| r.error_for_status()) {
            Ok(resp) => match resp.json::<serde_json::Value>() {
                Ok(v) => {
                    log::info!("版本信息已获取: {url}");
                    release = Some(v);
                    break;
                }
                Err(e) => {
                    log::warn!("版本信息解析失败（{url}）: {e}");
                    last_err = e.to_string();
                }
            },
            Err(e) => {
                log::warn!("版本信息获取失败（{url}）: {e}");
                last_err = e.to_string();
            }
        }
        on_progress(DownloadProgress {
            stage: "resolving".into(),
            downloaded: 0,
            total: 0,
            message: "换一个源再试…".into(),
        });
    }
    // 版本信息拿不到不是死路：下面还有内置版本可以试
    let asset = match release {
        Some(v) => parse_asset(&v)?,
        None => {
            log::warn!("拿不到最新版本信息（{last_err}），改用内置版本 v1.4.1");
            on_progress(DownloadProgress {
                stage: "resolving".into(),
                downloaded: 0,
                total: 0,
                message: "拿不到最新版本，改用内置版本 v1.4.1".into(),
            });
            fallback_asset()
        }
    };

    // ---- 2. 下载 ----
    let tmp_dir = std::env::temp_dir().join("snipforge").join("ocr-download");
    std::fs::create_dir_all(&tmp_dir)?;
    let archive = tmp_dir.join(&asset.name);

    let urls = download_candidates(mode, custom_prefix, &asset.url);
    let mut done = false;
    let mut last_err = String::new();
    for (i, url) in urls.iter().enumerate() {
        on_progress(DownloadProgress {
            stage: "downloading".into(),
            downloaded: 0,
            total: asset.size,
            message: format!("正在下载（源 {}/{}）…", i + 1, urls.len()),
        });
        match fetch_to_file(&client, url, &archive, asset.size, &mut on_progress) {
            Ok(()) => {
                log::info!("下载完成（第 {} 个源）: {url}", i + 1);
                done = true;
                break;
            }
            Err(e) => {
                log::warn!("下载源失败，换下一个（{url}）: {e}");
                last_err = e.to_string();
                let _ = std::fs::remove_file(&archive);
            }
        }
    }
    if !done {
        return Err(AppError::Msg(format!(
            "所有下载源都失败了：{last_err}\n可以手动下载 .7z 后用「我已有安装包」导入，或在设置里换一个加速镜像"
        )));
    }

    // ---- 3. 校验（新版 API 会给 sha256，有就一定要验）----
    if let Some(want) = &asset.sha256 {
        on_progress(DownloadProgress {
            stage: "extracting".into(),
            downloaded: asset.size,
            total: asset.size,
            message: "正在校验文件…".into(),
        });
        let got = sha256_of(&archive)?;
        if &got != want {
            let _ = std::fs::remove_file(&archive);
            return Err(AppError::Msg(format!(
                "校验失败：期望 {want}，实际 {got}。文件可能被改动过，已删除。"
            )));
        }
        log::info!("sha256 校验通过");
    }

    // ---- 4. 解压 ----
    let exe = extract_and_finish(&archive, engine_dir, &mut on_progress)?;
    let _ = std::fs::remove_file(&archive);

    on_progress(DownloadProgress {
        stage: "done".into(),
        downloaded: asset.size,
        total: asset.size,
        message: "安装完成".into(),
    });
    Ok(exe)
}

/// 用户自己下好的压缩包：只解压，不联网。
///
/// 这是"镜像全挂了 / 公司网络出不去"时的兜底 —— 有它就不会出现
/// "功能明明支持，但我就是装不上"。
pub fn install_from_archive(
    archive: &Path,
    engine_dir: &Path,
    mut on_progress: impl FnMut(DownloadProgress),
) -> AppResult<PathBuf> {
    if !archive.is_file() {
        return Err(AppError::Msg("找不到这个文件".into()));
    }
    std::fs::create_dir_all(engine_dir)?;
    extract_and_finish(archive, engine_dir, &mut on_progress)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_url_prefixing() {
        let gh = "https://github.com/a/b/releases/download/v1/x.7z";
        assert_eq!(
            mirror_url("https://ghfast.top/", gh),
            format!("https://ghfast.top/{gh}")
        );
        // 没写结尾斜杠也要能拼对
        assert_eq!(
            mirror_url("https://ghfast.top", gh),
            format!("https://ghfast.top/{gh}")
        );
        // 空前缀 = 直连
        assert_eq!(mirror_url("   ", gh), gh);
        // 已经把完整地址粘进来时不要再套一层
        let full = format!("https://ghfast.top/{gh}");
        assert_eq!(mirror_url(&full, gh), full);
    }

    #[test]
    fn candidate_order_puts_direct_last() {
        let urls = download_candidates(SourceMode::Auto, "", "https://github.com/x");
        assert_eq!(urls.last().unwrap(), "https://github.com/x");
        assert!(urls.len() > 1, "自动模式应当包含镜像");

        // 自定义前缀排在最前
        let urls =
            download_candidates(SourceMode::Custom, "https://my.mirror/", "https://github.com/x");
        assert_eq!(urls[0], "https://my.mirror/https://github.com/x");

        // 只在直连模式下才没有镜像
        let urls = download_candidates(SourceMode::Direct, "", "https://github.com/x");
        assert_eq!(urls, vec!["https://github.com/x".to_string()]);
    }

    #[test]
    fn mode_parsing() {        assert_eq!(SourceMode::parse("direct"), SourceMode::Direct);
        assert_eq!(SourceMode::parse("CUSTOM"), SourceMode::Custom);
        assert_eq!(SourceMode::parse(""), SourceMode::Auto);
        assert_eq!(SourceMode::parse("whatever"), SourceMode::Auto);
    }
}
