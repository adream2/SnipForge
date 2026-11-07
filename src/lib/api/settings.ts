/** 设置 / 应用信息 / 手势识别：所有"改配置"的入口。 */

import { invoke } from '@tauri-apps/api/core'
import type {
  AppInfo,
  AppSettings,
  FeatureInfo,
  GestureDefaults
} from '../types'

export const settingsApi = {
  /* 设置 */
  /** 全部可被手势/托盘触发的功能。名字和 id 由后端 `features.rs` 定 */
  featuresList: () => invoke<FeatureInfo[]>('features_list'),
  /** 出厂手势默认（通用 + 程序专用），"恢复默认"用 */
  gestureDefaults: () => invoke<GestureDefaults>('gesture_defaults'),
  settingsGet: () => invoke<{ settings: AppSettings }>('settings_get'),
  /** 注册表里是否真的登记了开机自启（和配置值对比，能看出写没写成功） */
  autostartStatus: () => invoke<boolean>('autostart_status'),
  settingsPatch: (patch: unknown) => invoke<{ settings: AppSettings }>('settings_patch', { patch }),
  settingsReset: () => invoke<{ settings: AppSettings }>('settings_reset'),
  pickDir: (defaultPath?: string) => invoke<string | null>('settings_pick_dir', { defaultPath }),
  /** 把一笔轨迹翻译成方向串（录制框用），和真手势同一份实现 */
  gestureTrace: (points: Array<[number, number]>) =>
    invoke<string>('gesture_trace', { points }),
  /** 试划：这一划会命中哪一条绑定（下标）。和真手势同一套匹配 + 容错 */
  gestureMatch: (pattern: string, profile?: string) =>
    invoke<number | null>('gesture_match', { pattern, profile: profile ?? null }),
  appInfo: () => invoke<AppInfo>('app_info'),
  showMain: (route?: string) => invoke<void>('app_show_main', { route: route ?? null }),
  /** 系统文件选择框。extensions 传 ['7z'] 这类扩展名（不带点） */
  pickFile: (title?: string, extensions?: string[]) =>
    invoke<string | null>('pick_file', { title: title ?? null, extensions: extensions ?? null }),
  openDir: (path: string) => invoke<void>('app_open_dir', { path }),
  openPath: (path: string) => invoke<void>('app_open_path', { path }),
}
