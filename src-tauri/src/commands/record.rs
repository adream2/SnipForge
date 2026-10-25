//! 录屏相关命令。
//!
//! 状态放在一个全局槽里：同一时刻只允许一个录制。
//! `ActiveRecording` 持有录制线程的 JoinHandle，`stop()` 会消费它 ——
//! 所以取出来必须用 `take()`，不能 clone。

use crate::error::{AppError, AppResult};
use crate::record::{ActiveRecording, RecordOptions, RecordResult, RecordStatus};
use crate::state::AppState;
use crate::types::Rect;
use std::sync::{Arc, Mutex, OnceLock};
use tauri::{Emitter, Manager};

static ACTIVE: OnceLock<Mutex<Option<ActiveRecording>>> = OnceLock::new();

fn slot() -> &'static Mutex<Option<ActiveRecording>> {
    ACTIVE.get_or_init(|| Mutex::new(None))
}

fn lock() -> AppResult<std::sync::MutexGuard<'static, Option<ActiveRecording>>> {
    slot()
        .lock()
        .map_err(|_| AppError::Msg("录制状态锁已污染".into()))
}

/// 没指定区域就录主屏
fn resolve_rect(rect: Option<Rect>) -> AppResult<Rect> {
    if let Some(r) = rect {
        if r.width < 16 || r.height < 16 {
            return Err(AppError::Msg("录制区域太小".into()));
        }
        return Ok(r);
    }

    let monitors =
        xcap::Monitor::all().map_err(|e| AppError::Msg(format!("枚举显示器失败: {e}")))?;
    let pick = monitors
        .iter()
        .find(|m| m.is_primary().unwrap_or(false))
        .or_else(|| monitors.first())
        .ok_or_else(|| AppError::Msg("没有可用显示器".into()))?;

    Ok(Rect {
        x: pick.x().unwrap_or(0),
        y: pick.y().unwrap_or(0),
        width: pick.width().unwrap_or(1920),
        height: pick.height().unwrap_or(1080),
    })
}

/// 开录。
///
/// 注意：这里**只开始采集、不写帧**。真正开写要等前端倒计时走完调 `record_begin`，
/// 这样 DXGI 首帧那几百毫秒的冷启动就被倒计时吃掉了，视频从第 0 帧就是满的。
#[tauri::command]
pub fn record_start(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    rect: Option<Rect>,
) -> AppResult<RecordStatus> {
    start_impl(&app, &state, rect)
}

fn start_impl(
    app: &tauri::AppHandle,
    state: &AppState,
    rect: Option<Rect>,
) -> AppResult<RecordStatus> {
    let mut guard = lock()?;
    if guard.is_some() {
        return Err(AppError::Msg("已经在录制中".into()));
    }

    let rect = resolve_rect(rect)?;
    let cfg = state.settings.read().clone();

    let dir = if cfg.save.dir.trim().is_empty() {
        state.paths.pictures.clone()
    } else {
        std::path::PathBuf::from(&cfg.save.dir)
    };
    std::fs::create_dir_all(&dir)?;

    // 输出格式：mp4 / gif / both。两个文件同名不同扩展，用户一眼能对上。
    let fmt = cfg.record.format.to_ascii_lowercase();
    let want_mp4 = fmt != "gif";
    let want_gif = fmt == "gif" || fmt == "both";
    let stem = crate::uploader::render_file_name(&cfg.save.name_template, "mp4");

    // 必须带扩展名：Media Foundation 靠扩展名决定封装格式，
    // 没有 .mp4 会直接建不出文件（而且报错很含糊）。
    let path = dir.join(format!("{stem}.mp4"));
    let gif_plan = want_gif.then(|| crate::record::gif::GifPlan {
        path: dir.join(format!("{stem}.gif")),
        fps: cfg.record.gif_fps.clamp(2, 30),
        max_width: cfg.record.gif_width,
    });

    let opts = RecordOptions {
        rect,
        fps: cfg.record.fps.clamp(10, 60),
        bitrate: cfg.record.video_bits_per_second.max(1_000_000),
        draw_cursor: cfg.record.show_cursor,
        write_mp4: want_mp4,
    };

    let rec = crate::record::start(Some(app.clone()), opts, path, gif_plan)?;
    rec.set_lens(cfg.record.magnifier);

    // 窗口要在采集起来之后再开，否则覆盖层会先于采集出现，头几帧里没有它
    if let Err(e) = crate::windows::open_record_windows(app, rect) {
        log::warn!("打开录制窗口失败: {e}");
    }

    let status = rec.status();
    *guard = Some(rec);
    log::info!("开始录制区域 {:?} → {}", rect, status.path);
    Ok(status)
}

/// 用户在选区里框完之后走这条。
///
/// 必须先关掉选区覆盖层再开录 —— 选区窗口是半透明深色遮罩，如果它还在屏幕上，
/// 录进去的头几帧就是一整块黑。关窗是异步的，所以要等一小下再开采集。
#[tauri::command]
pub fn record_start_region(
    app: tauri::AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    rect: Rect,
) -> AppResult<()> {
    crate::windows::close_selector(&app);

    let handle = app.clone();
    let state = state.inner().clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(160));
        if let Err(e) = start_impl(&handle, &state, Some(rect)) {
            log::error!("从选区开始录制失败: {e}");
            let _ = handle.emit("record://error", e.to_string());
        }
    });
    Ok(())
}

/// 倒计时结束，正式开始写帧
#[tauri::command]
pub fn record_begin() -> AppResult<()> {
    if let Some(rec) = lock()?.as_ref() {
        rec.begin();
    }
    Ok(())
}

#[tauri::command]
pub fn record_pause() -> AppResult<()> {
    if let Some(rec) = lock()?.as_ref() {
        rec.pause();
    }
    Ok(())
}

#[tauri::command]
pub fn record_resume() -> AppResult<()> {
    if let Some(rec) = lock()?.as_ref() {
        rec.resume();
    }
    Ok(())
}

#[tauri::command]
pub fn record_toggle_pause() -> AppResult<bool> {
    let guard = lock()?;
    match guard.as_ref() {
        Some(rec) => {
            if rec.live.is_paused() {
                rec.resume();
                Ok(false)
            } else {
                rec.pause();
                Ok(true)
            }
        }
        None => Err(AppError::Msg("没有正在进行的录制".into())),
    }
}

#[tauri::command]
pub fn record_set_lens(on: bool) -> AppResult<()> {
    if let Some(rec) = lock()?.as_ref() {
        rec.set_lens(on);
    }
    Ok(())
}

/// 标注模式：覆盖层开始接收鼠标
#[tauri::command]
pub fn record_set_interactive(app: tauri::AppHandle, interactive: bool) -> AppResult<()> {
    crate::windows::set_overlay_interactive(&app, interactive);
    // 覆盖层自己也切一下状态：它要显示工具栏、把画布接上指针事件。
    // 只在后端改窗口的"穿透"是够不到 DOM 的。
    crate::windows::emit_to_record_overlay(&app, "record://anno", interactive);
    Ok(())
}

#[tauri::command]
pub fn record_status() -> AppResult<Option<RecordStatus>> {
    Ok(lock()?.as_ref().map(|r| r.status()))
}

/// 停止并收尾，返回最终文件信息
#[tauri::command]
pub fn record_stop(app: tauri::AppHandle) -> AppResult<RecordResult> {
    // 注意不要在这里先把录制从槽里 take 出来 —— stop_recording 自己会 take。
    // 先 take 的话它取到 None，直接报"没有正在进行的录制"。
    stop_recording(&app)
}

/// 放弃本次录制（删掉半成品文件）
#[tauri::command]
pub fn record_cancel(app: tauri::AppHandle) -> AppResult<()> {
    let rec = lock()?.take();
    crate::windows::close_record_windows(&app);
    if let Some(rec) = rec {
        let path = rec.path.clone();
        let gif = rec.gif_path.clone();
        let _ = rec.stop();
        let _ = std::fs::remove_file(&path);
        // GIF 那一半也得删，否则取消录制会在文件夹里留个残废 gif
        if let Some(g) = gif {
            let _ = std::fs::remove_file(g);
        }
    }
    Ok(())
}

/// 录制是否正在进行（托盘菜单用）
pub fn is_recording() -> bool {
    slot().lock().map(|g| g.is_some()).unwrap_or(false)
}

/// 主窗口/托盘点"开始录制区域"时走这条：
/// 打开选区窗口，用户框完再真正开录。
pub fn trigger_region_record(app: &tauri::AppHandle) -> AppResult<()> {
    if is_recording() {
        return Err(AppError::Msg("已经在录制中".into()));
    }
    let state = app
        .try_state::<Arc<AppState>>()
        .ok_or_else(|| AppError::Msg("状态未初始化".into()))?;

    // 复用截图的取帧流程：裁区域需要一个背景图给用户"框"，顺带把显示器信息拿到
    let payload = crate::capture::begin(&state, "record")?;
    let cursor = app
        .cursor_position()
        .map_err(|e| AppError::Msg(format!("读取鼠标位置失败: {e}")))?;
    let target = payload
        .displays
        .iter()
        .find(|d| crate::windows::rect_contains(&d.bounds, cursor.x as i32, cursor.y as i32))
        .or_else(|| payload.displays.first())
        .ok_or_else(|| AppError::Msg("没有可用显示器".into()))?;

    crate::windows::open_selector(app, target.bounds, target.id, true)
}

/* ------------------ 给托盘 / 快捷键用的公开入口 ------------------ */

/// 录主屏
pub fn start_fullscreen(app: &tauri::AppHandle) -> AppResult<RecordStatus> {
    let state = app
        .try_state::<Arc<AppState>>()
        .ok_or_else(|| AppError::Msg("状态未初始化".into()))?
        .inner()
        .clone();
    start_impl(app, &state, None)
}

/// 停止并收尾（托盘菜单用，行为和命令一致）
pub fn stop_recording(app: &tauri::AppHandle) -> AppResult<RecordResult> {
    let rec = lock()?
        .take()
        .ok_or_else(|| AppError::Msg("没有正在进行的录制".into()))?;
    crate::windows::close_record_windows(app);
    let result = rec.stop()?;
    log::info!(
        "录制完成：{}（{} 帧 / {}ms / {} KB）{}",
        result.path,
        result.frames,
        result.duration_ms,
        result.bytes / 1024,
        match &result.gif_path {
            Some(p) => format!(" + GIF {}（{} KB）", p, result.gif_bytes / 1024),
            None => String::new(),
        }
    );

    // 录完把文件塞进剪贴板：GIF 的下一个动作几乎总是"粘到聊天窗口里"。
    // 走 CF_HDROP，粘出来是文件本身而不是一条路径文本。
    if let Some(state) = app.try_state::<Arc<AppState>>() {
        let cfg = state.settings.read().clone();
        if cfg.save.after_save == "copy" {
            let target = result.gif_path.clone().unwrap_or_else(|| result.path.clone());
            if let Err(e) = crate::win32::clipboard::set_files(&[target]) {
                log::warn!("把录制文件放进剪贴板失败: {e}");
            }
        }
    }

    let _ = app.emit("record://stopped", &result);
    Ok(result)
}

/// 主窗口里点"录制区域…"：先收起主窗口再进选区。
///
/// 不先藏的话，选区背景图（截屏）里会有我们自己的窗口，
/// 用户刚框到的那块区域大概率就是它。
#[tauri::command]
pub fn record_pick_region(app: tauri::AppHandle) -> AppResult<()> {
    crate::windows::hide_main(&app);
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(180));
        if let Err(e) = trigger_region_record(&handle) {
            log::warn!("进入录制选区失败: {e}");
            let _ = handle.emit("record://error", e.to_string());
        }
    });
    Ok(())
}

/// 主窗口里点"录制全屏"
#[tauri::command]
pub fn record_start_fullscreen(app: tauri::AppHandle) -> AppResult<()> {
    crate::windows::hide_main(&app);
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(180));
        if let Err(e) = start_fullscreen(&handle) {
            log::warn!("录制全屏失败: {e}");
            let _ = handle.emit("record://error", e.to_string());
        }
    });
    Ok(())
}

/// 快捷键 / 托盘上的"录屏"：录着就停，没录就进选区
pub fn toggle(app: &tauri::AppHandle) {
    if is_recording() {
        if let Err(e) = stop_recording(app) {
            log::warn!("停止录制失败: {e}");
        }
    } else if let Err(e) = trigger_region_record(app) {
        log::warn!("开始录制失败: {e}");
    }
}
