//! 截图编排层：状态机 + 裁剪 + 长截图入口。

pub mod screen;
pub mod scroll;

use crate::error::{AppError, AppResult};
use crate::state::{AppState, CaptureSession};
use crate::types::{CaptureResult, FramePayload, Rect};

/// 开始一次截图：抓帧 → 存会话 → 返回给 selector 的载荷。
///
/// 注意调用方必须先把自家窗口藏起来（见 commands::capture::start_capture），
/// 否则会把 SnipForge 自己截进去。
pub fn begin(state: &AppState, mode: &str) -> AppResult<FramePayload> {
    let frame = screen::capture_frame()?;
    let payload = frame.payload();
    *state.capture.lock() = Some(CaptureSession {
        frame,
        mode: mode.to_string(),
        aborted: false,
    });
    Ok(payload)
}

/// 提交框选结果，裁出图片。
/// display_id 由 selector 指定（它知道自己画在哪块屏上）。
pub fn commit(state: &AppState, display_id: u32, rect: Rect) -> AppResult<CaptureResult> {
    let guard = state.capture.lock();
    let session = guard
        .as_ref()
        .ok_or_else(|| AppError::Msg("截图会话已失效，请重新截图".into()))?;

    let display = session
        .frame
        .find(display_id)
        .ok_or_else(|| AppError::Msg("找不到对应的显示器".into()))?;

    screen::crop_display(display, rect)
}


/// 取出当前会话的显示器预览（selector 打开时拉一次）
pub fn payload_of(state: &AppState) -> Option<FramePayload> {
    state
        .capture
        .lock()
        .as_ref()
        .map(|s| s.frame.payload())
}

pub fn abort(state: &AppState) {
    if let Some(session) = state.capture.lock().as_mut() {
        session.aborted = true;
    }
}
