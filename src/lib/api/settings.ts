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
  /** 一笔轨迹是不是形状手势（目前只有 "S"）。识别只有后端一份实现 */
  gestureRecognize: (points: Array<[number, number]>) =>
    invoke<string | null>('gesture_recognize', { points }),
  /** 把手画的一笔变成手势模板（32 点），和真手势同一份实现 */
  gestureNormalize: (points: Array<[number, number]>) =>
    invoke<number[][]>('gesture_normalize', { points }),
  /** 试划：这一划会命中哪一条绑定（下标）。和真手势同一套匹配顺序 */
  gestureMatch: (points: Array<[number, number]>, pattern: string, profile?: string) =>
    invoke<number | null>('gesture_match', { points, pattern, profile: profile ?? null }),
  appInfo: () => invoke<AppInfo>('app_info'),
  showMain: (route?: string) => invoke<void>('app_show_main', { route: route ?? null }),
  openDir: (path: string) => invoke<void>('app_open_dir', { path }),
  openPath: (path: string) => invoke<void>('app_open_path', { path }),
}
