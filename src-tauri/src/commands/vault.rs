//! 主密码相关命令。
//!
//! 全部丢进 `spawn_blocking`：Argon2id 按 19MiB 内存代价跑一次是几十毫秒，
//! 同步命令跑在主线程上，用户会看到界面卡一下。

use crate::error::{AppError, AppResult};
use crate::state::AppState;
use crate::vault::VaultStatus;
use std::sync::Arc;

/// 主密码最短长度。
///
/// 6 位对本地工具够用，但这是**唯一**挡在密文前面的一道门 ——
/// Argon2 只是让暴力破解变贵，不解决"123456"。取 8。
const MIN_LEN: usize = 8;

fn check_strength(pw: &str) -> AppResult<()> {
    if pw.chars().count() < MIN_LEN {
        return Err(AppError::Msg(format!("主密码至少 {MIN_LEN} 个字符")));
    }
    Ok(())
}

#[tauri::command]
pub fn vault_status(state: tauri::State<'_, Arc<AppState>>) -> AppResult<VaultStatus> {
    Ok(state.vault.status())
}

#[tauri::command]
pub async fn vault_enable(
    state: tauri::State<'_, Arc<AppState>>,
    password: String,
) -> AppResult<VaultStatus> {
    check_strength(&password)?;
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || st.enable_vault(&password))
        .await
        .map_err(|e| AppError::Msg(format!("加密任务失败: {e}")))??;
    Ok(state.vault.status())
}

#[tauri::command]
pub async fn vault_unlock(
    state: tauri::State<'_, Arc<AppState>>,
    password: String,
) -> AppResult<VaultStatus> {
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || st.unlock_vault(&password))
        .await
        .map_err(|e| AppError::Msg(format!("解锁任务失败: {e}")))??;
    Ok(state.vault.status())
}

#[tauri::command]
pub fn vault_lock(state: tauri::State<'_, Arc<AppState>>) -> AppResult<VaultStatus> {
    state.lock_vault();
    Ok(state.vault.status())
}

#[tauri::command]
pub async fn vault_change_password(
    state: tauri::State<'_, Arc<AppState>>,
    password: String,
) -> AppResult<VaultStatus> {
    if !state.vault.is_unlocked() {
        return Err(AppError::Msg("请先解锁再修改主密码".into()));
    }
    check_strength(&password)?;
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || st.change_vault_password(&password))
        .await
        .map_err(|e| AppError::Msg(format!("改密任务失败: {e}")))??;
    Ok(state.vault.status())
}

/// 关掉主密码。必须已解锁 —— 否则等于"不用密码就能把加密撤掉"。
#[tauri::command]
pub async fn vault_disable(
    state: tauri::State<'_, Arc<AppState>>,
) -> AppResult<VaultStatus> {
    if !state.vault.is_unlocked() {
        return Err(AppError::Msg("请先解锁再关闭主密码".into()));
    }
    let st = state.inner().clone();
    tauri::async_runtime::spawn_blocking(move || st.disable_vault())
        .await
        .map_err(|e| AppError::Msg(format!("关闭任务失败: {e}")))??;
    Ok(state.vault.status())
}

/// 忘记密码的兜底：清空保险箱（**里面的数据全部丢弃**）。
///
/// 不需要密码 —— 需要密码就没意义了。这是"把自己锁死了怎么办"的唯一出口，
/// 所以界面上必须写清楚"数据会丢"，并且要做二次确认。
#[tauri::command]
pub fn vault_reset(state: tauri::State<'_, Arc<AppState>>) -> AppResult<VaultStatus> {
    state.reset_vault()?;
    Ok(state.vault.status())
}
