use crate::capture::{self, scroll};
use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::types::{CaptureResult, FramePayload, NativeWindowInfo, Rect};
use crate::windows as win;
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager};

/// 快捷键 / 托盘触发的入口（同步上下文，内部起异步任务）
pub fn trigger(app: &AppHandle, mode: &str) {
    let handle = app.clone();
    let mode = mode.to_string();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = run_capture(&handle, &mode).await {
            log::error!("截图失败: {e}");
            let _ = handle.emit("app://toast", serde_json::json!({
                "type": "error",
                "message": format!("截图失败：{e}")
            }));
        }
    });
}

pub fn toggle_main(app: &AppHandle) {
    win::toggle_main(app);
}

async fn run_capture(app: &AppHandle, mode: &str) -> AppResult<()> {
    let state = app
        .try_state::<Arc<AppState>>()
        .ok_or_else(|| AppError::Msg("应用状态未初始化".into()))?
        .inner()
        .clone();

    // 钩子：插件可在截图前介入（比如强制某模式、弹提示）
    let (cancelled, _) = state.hooks.run(
        crate::hooks::names::CAPTURE_BEFORE,
        serde_json::json!({ "mode": mode }),
    );
    if cancelled {
        return Ok(());
    }

    // 先藏起自家窗口，否则截到自己
    win::hide_main(app);
    win::close_selector(app);
    std::thread::sleep(std::time::Duration::from_millis(150));

    match mode {
        "fullscreen" => {
            let payload = capture::begin(&state, mode)?;
            let _ = payload;
            let result = capture::commit_fullscreen(&state)?;
            finish(app, &state, result)?;
        }
        // 框选完不打开编辑器，直接贴图 / 直接取字（手势用）
        "pin-region" | "ocr-region" => {
            let payload = capture::begin(&state, "region")?;
            let first = payload
                .displays
                .first()
                .ok_or_else(|| AppError::Msg("没有可用显示器".into()))?;
            let cursor = app.cursor_position().unwrap_or_default();
            let (cx, cy) = (cursor.x as i32, cursor.y as i32);
            let target = payload
                .displays
                .iter()
                .find(|d| crate::windows::rect_contains(&d.bounds, cx, cy))
                .unwrap_or(first);
            let after = if mode == "pin-region" { "pin" } else { "ocr" };
            win::open_selector_after(app, target.bounds, target.id, after)?;
        }
        _ => {
            // region / scroll：抓帧后交给覆盖层
            let payload = capture::begin(&state, mode)?;
            let first = payload
                .displays
                .first()
                .ok_or_else(|| AppError::Msg("没有可用显示器".into()))?;

            // 在鼠标所在那块屏上开覆盖层
            let cursor = app.cursor_position().unwrap_or_default();
            let cx = cursor.x as i32;
            let cy = cursor.y as i32;
            let target = payload
                .displays
                .iter()
                .find(|d| crate::windows::rect_contains(&d.bounds, cx, cy))
                .unwrap_or(first);

            win::open_selector(app, target.bounds, target.id, None)?;
        }
    }
    Ok(())
}

fn finish(app: &AppHandle, state: &Arc<AppState>, result: CaptureResult) -> AppResult<()> {
    // 钩子：插件可改写图片（加水印、压缩等）
    let (cancelled, payload) = state.hooks.run(
        crate::hooks::names::CAPTURE_AFTER,
        serde_json::json!({
            "dataUrl": result.data_url,
            "width": result.width,
            "height": result.height
        }),
    );
    if cancelled {
        return Ok(());
    }

    let mut final_result = result;
    if let Some(new_url) = payload["dataUrl"].as_str() {
        if new_url != final_result.data_url {
            final_result.data_url = new_url.to_string();
        }
    }

    *state.pending.lock() = Some(final_result.clone());
    state.release_capture();
    win::open_editor(app, final_result)?;
    Ok(())
}

/* ------------------------- 命令 ------------------------- */

#[tauri::command]
pub fn capture_start(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    mode: String,
) -> AppResult<()> {
    let inner = state.inner().clone();
    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        win::hide_main(&handle);
        std::thread::sleep(std::time::Duration::from_millis(120));
        match capture::begin(&inner, &mode) {
            Ok(_) => {
                if let Some(payload) = capture::payload_of(&inner) {
                    if let Some(first) = payload.displays.first() {
                        let _ = win::open_selector(&handle, first.bounds, first.id, None);
                    }
                }
            }
            Err(e) => log::error!("启动截图失败: {e}"),
        }
    });
    Ok(())
}

/// selector 打开后拉取预览帧
#[tauri::command]
pub fn capture_frame(state: tauri::State<'_, Arc<AppState>>) -> AppResult<FramePayload> {
    capture::payload_of(&state).ok_or_else(|| AppError::Msg("截图会话已失效".into()))
}

/// selector 拉取窗口列表（悬停高亮用）
#[tauri::command]
pub fn capture_windows() -> AppResult<Vec<NativeWindowInfo>> {
    Ok(crate::win32::window::list_top_level_windows(std::process::id()))
}

/// 当前光标位置（物理像素）。
///
/// 选区覆盖层刚打开时用它把光标下的窗口立刻高亮出来 —— 否则要等用户
/// 抖一下鼠标才有反馈，看起来像"识别不到窗口"。
///
/// 必须是 `async`：`cursor_position()` 内部要把调用投递回主线程，
/// 同步命令本来就在主线程上跑，会自锁（钉图冻死应用就是踩的这条）。
#[tauri::command]
pub async fn cursor_pos(app: tauri::AppHandle) -> AppResult<(i32, i32)> {
    let p = app
        .cursor_position()
        .map_err(|e| AppError::Msg(format!("读取鼠标位置失败: {e}")))?;
    Ok((p.x as i32, p.y as i32))
}

/// selector 提交框选。
///
/// 注意：覆盖层窗口自己要关掉，如果先关再 return，发起 invoke 的 webview 已经被销毁，
/// 响应送不回去，前端会收到一个莫名其妙的 reject。所以先返回结果，再异步收尾。
#[tauri::command]
pub fn capture_commit(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    display_id: u32,
    rect: Rect,
) -> AppResult<CaptureResult> {
    let result = capture::commit(&state, display_id, rect)?;
    let handle = app.clone();
    let carried = result.clone();
    tauri::async_runtime::spawn(async move {
        win::close_selector(&handle);
        if let Some(st) = handle.try_state::<Arc<AppState>>() {
            if let Err(e) = finish(&handle, st.inner(), carried) {
                log::error!("打开编辑器失败: {e}");
            }
        }
    });
    Ok(result)
}

/// 框选完直接贴到屏幕上（手势 ↓→↑← 用的那条路）。
///
/// 和普通截图共用抓帧 + 选区，区别只在收尾：不进编辑器，直接钉在桌面。
#[tauri::command]
pub async fn capture_pin(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    display_id: u32,
    rect: Rect,
) -> AppResult<CaptureResult> {
    let result = capture::commit(&state, display_id, rect)?;
    let handle = app.clone();
    let inner = state.inner().clone();
    let data = result.data_url.clone();
    tauri::async_runtime::spawn(async move {
        // 先关覆盖层再贴图，否则遮罩会闪在贴图上
        win::close_selector(&handle);
        inner.release_capture();
        if let Err(e) = crate::commands::image::pin_data_url(&handle, &inner, data).await {
            log::warn!("框选贴图失败: {e}");
        }
    });
    Ok(result)
}

/// 框选完直接取字（手势 →↓←↑），结果弹在一个独立小窗里。
#[tauri::command]
pub async fn capture_ocr(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    display_id: u32,
    rect: Rect,
) -> AppResult<CaptureResult> {
    let result = capture::commit(&state, display_id, rect)?;
    let handle = app.clone();
    let inner = state.inner().clone();
    let data = result.data_url.clone();
    tauri::async_runtime::spawn(async move {
        win::close_selector(&handle);
        inner.release_capture();
        if let Err(e) = crate::windows::open_ocr(&handle, data) {
            log::warn!("打开取字窗口失败: {e}");
        }
    });
    Ok(result)
}

/// 光标底下的窗口 / 组件（区域截图悬停高亮用）。
///
/// 坐标是物理像素；`whole = true` 时只认整个顶层窗口（按住 Ctrl 的用户意图）。
/// 前端每次移动都会调，所以这里只做纯查询，不碰截图会话。
#[tauri::command]
pub async fn capture_element(x: i32, y: i32, whole: bool) -> Option<crate::types::NativeElement> {
    // async + spawn_blocking：同步命令跑在主线程上，一次枚举几百个窗口
    // 会卡住 webview 的 IPC，鼠标划过窗口时高亮就会一顿一顿的。
    tauri::async_runtime::spawn_blocking(move || crate::win32::window::element_at(x, y, whole))
        .await
        .ok()
        .flatten()
}

/// selector 取消
#[tauri::command]
pub fn capture_cancel(app: AppHandle, state: tauri::State<'_, Arc<AppState>>) {
    win::close_selector(&app);
    state.release_capture();
}

/// 编辑器取走当前待编辑截图
#[tauri::command]
pub fn capture_pending(state: tauri::State<'_, Arc<AppState>>) -> Option<CaptureResult> {
    state.pending.lock().clone()
}

/// 长截图：selector 框好后调用
#[tauri::command]
pub async fn capture_scroll(
    app: AppHandle,
    state: tauri::State<'_, Arc<AppState>>,
    display_id: u32,
    rect: Rect,
) -> AppResult<CaptureResult> {
    let state = state.inner().clone();
    let app2 = app.clone();

    // 覆盖层必须让开，否则会把自己滚进截图里
    win::close_selector(&app2);
    std::thread::sleep(std::time::Duration::from_millis(120));

    // spawn_blocking 的闭包是 move 的，先把 Arc 再克隆一份给任务，
    // 否则外面的 state 会被移走，后面的 release_capture 就用不了
    let task_state = state.clone();
    let result = tauri::async_runtime::spawn_blocking(move || -> AppResult<CaptureResult> {
        let display = {
            let guard = task_state.capture.lock();
            let session = guard
                .as_ref()
                .ok_or_else(|| AppError::Msg("截图会话已失效".into()))?;
            session
                .frame
                .find(display_id)
                .ok_or_else(|| AppError::Msg("找不到显示器".into()))?
                .physical
        };

        // 滚轮落点：区域中心偏下，避开吸顶头部
        let wheel_x = rect.x + rect.width as i32 / 2;
        let wheel_y = rect.y + (rect.height as f64 * 0.7) as i32;

        let options = scroll::ScrollOptions {
            max_frames: 24,
            settle_ms: 420,
            wheel_x,
            wheel_y,
            wheel_delta: -360,
        };

        let app_inner = app2.clone();
        let capture_once = |region: Rect| -> AppResult<image::RgbaImage> {
            // 每帧重新抓：目标窗口在滚动，缓存的首帧早就过期了
            let fresh = capture::screen::capture_frame()?;
            let d = fresh
                .displays
                .iter()
                .find(|d| d.physical.x == display.x && d.physical.y == display.y)
                .or_else(|| fresh.displays.first())
                .ok_or_else(|| AppError::Msg("捕获失败".into()))?;
            scroll::extract_region(d, region)
                .ok_or_else(|| AppError::Msg("区域裁剪失败".into()))
        };

        let aborted = {
            let st = task_state.clone();
            move || {
                st.capture
                    .lock()
                    .as_ref()
                    .map(|s| s.aborted)
                    .unwrap_or(true)
            }
        };

        let progress = move |p: scroll::ScrollProgress| {
            let _ = app_inner.emit("capture://scroll-progress", p);
        };

        scroll::run(rect, options, capture_once, progress, &aborted)
    })
    .await
    .map_err(|e| AppError::Msg(format!("长截图任务失败: {e}")))??;

    // 长截图结束后会话里的首帧已经没用了
    state.release_capture();

    *state.pending.lock() = Some(result.clone());
    win::open_editor(&app, result.clone())?;
    Ok(result)
}

#[tauri::command]
pub fn capture_abort_scroll(state: tauri::State<'_, Arc<AppState>>) {
    capture::abort(&state);
}

#[tauri::command]
pub fn capture_close_editor(app: AppHandle) {
    win::close_editor(&app);
}
