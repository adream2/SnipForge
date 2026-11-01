/** 主密码保险箱：状态与操作。 */

import { invoke } from '@tauri-apps/api/core'
import type { VaultStatus } from '../types'

export const vaultApi = {
  /** 读当前状态（启动后是 enabled=true / unlocked=false） */
  vaultStatus: () => invoke<VaultStatus>('vault_status'),
  /** 首次设置主密码：把已有明文凭据搬进保险箱 */
  vaultEnable: (password: string) => invoke<VaultStatus>('vault_enable', { password }),
  vaultUnlock: (password: string) => invoke<VaultStatus>('vault_unlock', { password }),
  vaultLock: () => invoke<VaultStatus>('vault_lock'),
  /** 改密码（需先解锁） */
  vaultChangePassword: (password: string) =>
    invoke<VaultStatus>('vault_change_password', { password }),
  /** 关闭主密码：写回明文并删除保险箱（需先解锁） */
  vaultDisable: () => invoke<VaultStatus>('vault_disable'),
  /** 忘记密码的兜底：清空保险箱，里面的数据全部丢弃 */
  vaultReset: () => invoke<VaultStatus>('vault_reset')
}
