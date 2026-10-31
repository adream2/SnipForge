/** hosts 编辑：读写系统 hosts 文件（写会提权） */

import { invoke } from '@tauri-apps/api/core'
import type { HostsBackup, HostsInfo, HostsSaveOutcome } from '../types'

export const hostsApi = {
  hostsInfo: () => invoke<HostsInfo>('hosts_info'),
  /** 保存：没有管理员权限时会弹 UAC；用户取消会返回错误，不会假装成功 */
  hostsSave: (text: string) => invoke<HostsSaveOutcome>('hosts_save', { text }),
  hostsBackups: () => invoke<HostsBackup[]>('hosts_backups'),
  hostsRestore: (name: string) => invoke<HostsSaveOutcome>('hosts_restore', { name }),
  /** 在资源管理器里选中 hosts */
  hostsReveal: () => invoke<void>('hosts_reveal'),
  hostsOpenBackupDir: () => invoke<void>('hosts_open_backup_dir'),
}
