/** 插件：状态与开关。 */

import { invoke } from '@tauri-apps/api/core'
import type {
  HookPoint,
  PluginState
} from '../types'

export const pluginApi = {
  /* 插件 */
  pluginsState: () => invoke<PluginState>('plugins_state'),
  pluginsToggle: (id: string, enabled: boolean) =>
    invoke<PluginState>('plugins_toggle', { id, enabled }),
  pluginsReload: () => invoke<PluginState>('plugins_reload'),
  /** 当前有哪些钩子点、各挂了几个处理器 */
  pluginsHooks: () => invoke<HookPoint[]>('plugins_hooks'),
  pluginsOpenDir: (which?: 'user' | 'builtin') =>
    invoke<void>('plugins_open_dir', { which })
}
