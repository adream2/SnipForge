/** 插件：状态与开关。 */

import { invoke } from '@tauri-apps/api/core'
import type {
  PluginState
} from '../types'

export const pluginApi = {
  /* 插件 */
  pluginsState: () => invoke<PluginState>('plugins_state'),
  pluginsToggle: (id: string, enabled: boolean) =>
    invoke<PluginState>('plugins_toggle', { id, enabled }),
  pluginsReload: () => invoke<PluginState>('plugins_reload'),
  pluginsOpenDir: (which?: 'user' | 'builtin') =>
    invoke<void>('plugins_open_dir', { which })
}
