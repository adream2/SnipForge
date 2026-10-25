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

    // 手动轮询代替 wait()，这样才能做超时。模型加载在冷启动时要几秒，
    // 第一次跑给足时间，但卡死时也不能把整个应用拖住。
    let deadline = std::time::Instant::now() + Duration::from_secs(timeout_secs.max(5));
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {
                if std::time::Instant::now() > deadline {
                    let _ = child.kill();
                    return Err(AppError::Msg(format!(
                        "PaddleOCR 超过 {timeout_secs} 秒未返回，已终止"
                    )));
                }
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(e) => return Err(AppError::Msg(format!("等待 PaddleOCR 失败: {e}"))),
        }
    }

    let output = child
        .wait_with_output()
        .map_err(|e| AppError::Msg(format!("读取 PaddleOCR 输出失败: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if stdout.trim().is_empty() {
        let stderr = String::from_utf8_lossy(&output.stderr);
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

fn http_client() -> AppResult<reqwest::blocking::Client> {
    reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(1800))
        .user_agent("SnipForge/0.1 (screenshot tool)")
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

/// 一键下载并安装 PaddleOCR。
///
/// 为什么自己实现而不是让用户手动下载？
/// 官方发布的是 88MB 的 7z，而 Windows 资源管理器不支持 7z ——
/// 让普通用户「去 GitHub 下 7z，再装个解压软件，解到指定目录」，
/// 这一步就劝退绝大多数人。7z 解压用纯 Rust 实现，不依赖外部工具。
pub fn download_and_install(
    engine_dir: &Path,
    mut on_progress: impl FnMut(DownloadProgress),
) -> AppResult<PathBuf> {
    std::fs::create_dir_all(engine_dir)?;
    let client = http_client()?;

    on_progress(DownloadProgress {
        stage: "resolving".into(),
        downloaded: 0,
        total: 0,
        message: "正在查询最新版本…".into(),
    });

    let release: serde_json::Value = client
        .get(RELEASE_API)
        .send()
        .map_err(|e| AppError::Msg(format!("访问 GitHub 失败（可能需要代理）：{e}")))?
        .json()
        .map_err(|e| AppError::Msg(format!("解析版本信息失败: {e}")))?;

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
    let file_name = asset["name"].as_str().unwrap_or("paddleocr.7z").to_string();
    let total = asset["size"].as_u64().unwrap_or(0);

    let tmp_dir = std::env::temp_dir().join("snipforge").join("ocr-download");
    std::fs::create_dir_all(&tmp_dir)?;
    let archive = tmp_dir.join(&file_name);

    // ---- 下载 ----
    let mut resp = client
        .get(&url)
        .send()
        .map_err(|e| AppError::Msg(format!("下载失败：{e}")))?;

    {
        use std::io::{Read, Write};
        let mut file = std::fs::File::create(&archive)?;
        let mut buf = vec![0u8; 128 * 1024];
        let mut downloaded: u64 = 0;
        let mut last_report = std::time::Instant::now();

        loop {
            let n = resp
                .read(&mut buf)
                .map_err(|e| AppError::Msg(format!("下载中断：{e}")))?;
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
        on_progress(DownloadProgress {
            stage: "downloading".into(),
            downloaded,
            total: total.max(downloaded),
            message: "下载完成".into(),
        });
    }

    // ---- 解压 ----
    on_progress(DownloadProgress {
        stage: "extracting".into(),
        downloaded: total,
        total,
        message: "正在解压（约 150MB，视磁盘速度需要几十秒）…".into(),
    });

    sevenz_rust::decompress_file(&archive, engine_dir)
        .map_err(|e| AppError::Msg(format!("7z 解压失败: {e:?}")))?;

    let _ = std::fs::remove_file(&archive);

    let exe = find_exe_in(engine_dir).ok_or_else(|| {
        AppError::Msg(format!(
            "解压完成但没找到 PaddleOCR-json.exe，请检查 {}",
            engine_dir.display()
        ))
    })?;

    on_progress(DownloadProgress {
        stage: "done".into(),
        downloaded: total,
        total,
        message: "安装完成".into(),
    });

    Ok(exe)
}
