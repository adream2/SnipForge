//! 版本检查命令。

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::update::UpdateInfo;
use std::sync::Arc;

/// 手动检查更新。走后台线程，网络慢也不会卡住界面。
#[tauri::command]
pub async fn update_check(state: tauri::State<'_, Arc<AppState>>) -> AppResult<UpdateInfo> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || crate::update::check_blocking(&st))
        .await
        .map_err(|e| AppError::Msg(format!("检查任务失败: {e}")))?
}

/// 启动时那次自动检查的结果。没有就是没查过（或没网）。
///
/// 界面进「关于」时先读它，用户不用为了看一眼版本再等一次网络往返。
#[tauri::command]
pub fn update_cached(state: tauri::State<'_, Arc<AppState>>) -> Option<UpdateInfo> {
    state.update.read().clone()
}

/// 打开发布页（只接受 GitHub 链接，见 `update::open_release`）
#[tauri::command]
pub fn update_open(url: String) -> AppResult<()> {
    crate::update::open_release(&url)
}
