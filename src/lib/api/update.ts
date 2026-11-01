/** 版本检查。只有三个动作：查一次、读缓存、打开发布页。 */

import { invoke } from '@tauri-apps/api/core'
import type { UpdateInfo } from '../types'

export const updateApi = {
  /** 真去 GitHub 查一次（启动时的自动检查结果见 updateCached） */
  updateCheck: () => invoke<UpdateInfo>('update_check'),
  /** 启动时那次自动检查的结果；没查过/没网就是 null */
  updateCached: () => invoke<UpdateInfo | null>('update_cached'),
  /** 打开发布页。后端只放行 https://github.com/ 开头的地址 */
  updateOpen: (url: string) => invoke<void>('update_open', { url })
}
