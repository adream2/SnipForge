//! hosts 编辑器的 IPC。
//!
//! 保存会弹 UAC、等用户点确认，是**秒级**的阻塞操作，所以走
//! `spawn_blocking` —— 同步命令在主线程上跑，卡住的就是整个界面。

use crate::error::{AppError, AppResult};
use crate::hosts::{HostsBackup, HostsInfo, SaveOutcome};
use crate::state::AppState;
use std::sync::Arc;

#[tauri::command]
pub fn hosts_info() -> AppResult<HostsInfo> {
    crate::hosts::read_info()
}

#[tauri::command]
pub async fn hosts_save(
    state: tauri::State<'_, Arc<AppState>>,
    text: String,
) -> AppResult<SaveOutcome> {
    let dir = state.paths.data.clone();
    tauri::async_runtime::spawn_blocking(move || crate::hosts::save(&dir, &text))
        .await
        .map_err(|e| AppError::Msg(format!("保存任务失败：{e}")))?
}

#[tauri::command]
pub fn hosts_backups(state: tauri::State<'_, Arc<AppState>>) -> Vec<HostsBackup> {
    crate::hosts::list_backups(&state.paths.data)
}

#[tauri::command]
pub async fn hosts_restore(
    state: tauri::State<'_, Arc<AppState>>,
    name: String,
) -> AppResult<SaveOutcome> {
    let dir = state.paths.data.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let text = crate::hosts::read_backup(&dir, &name)?;
        crate::hosts::save(&dir, &text)
    })
    .await
    .map_err(|e| AppError::Msg(format!("恢复任务失败：{e}")))?
}

/// 在资源管理器里定位 hosts（选中它，而不是只打开目录 —— 目录里文件太多）
#[tauri::command]
pub fn hosts_reveal() -> AppResult<()> {
    let p = crate::hosts::path();
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", p.display()))
        .spawn()
        .map_err(|e| AppError::Msg(format!("打开资源管理器失败：{e}")))?;
    Ok(())
}

/// 备份文件所在目录（找不到就打开应用数据目录）
#[tauri::command]
pub fn hosts_open_backup_dir(state: tauri::State<'_, Arc<AppState>>) -> AppResult<()> {
    let dir = state.paths.data.join("hosts-backups");
    let _ = std::fs::create_dir_all(&dir);
    crate::commands::app_open_path_inner(&dir)
}
