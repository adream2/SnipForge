/** 录屏：MP4 / GIF。 */

import { invoke } from '@tauri-apps/api/core'
import type {
  Rect,
  RecordFormat,
  RecordResult,
  RecordStatus
} from '../types'

export const recordApi = {
  /* ---------------- 录屏 ---------------- */
  recordStart: (rect?: Rect | null, format?: RecordFormat) =>
    invoke<RecordStatus>('record_start', { rect: rect ?? null, format: format ?? null }),
  /** 主窗口里的"录制区域…"：收起主窗口 → 进选区 → 框完自动开录 */
  recordPickRegion: (format?: RecordFormat) =>
    invoke<void>('record_pick_region', { format: format ?? null }),
  /** 主窗口里的"录制全屏" */
  recordStartFullscreen: (format?: RecordFormat) =>
    invoke<void>('record_start_fullscreen', { format: format ?? null }),
  /** 选区框完后直接开录（先关选区再采集） */
  recordStartRegion: (rect: Rect, format?: RecordFormat) =>
    invoke<void>('record_start_region', { rect, format: format ?? null }),
  recordBegin: () => invoke<void>('record_begin'),
  recordPause: () => invoke<void>('record_pause'),
  recordResume: () => invoke<void>('record_resume'),
  recordTogglePause: () => invoke<boolean>('record_toggle_pause'),
  recordStop: () => invoke<RecordResult>('record_stop'),
  recordCancel: () => invoke<void>('record_cancel'),
  recordStatus: () => invoke<RecordStatus | null>('record_status'),
  recordRecent: (limit?: number) =>
    invoke<import('../types').RecordFile[]>('record_recent', { limit: limit ?? null }),
  recordSetLens: (on: boolean) => invoke<void>('record_set_lens', { on }),
  /** 标注模式：覆盖层开始接收鼠标 */
  recordSetInteractive: (interactive: boolean) =>
    invoke<void>('record_set_interactive', { interactive }),
  /** 列表加载完了，通知后端解除"预热"（预热期攒下的回车会在后端补做） */
  clipboardPanelReady: (topId: string | null) =>
    invoke<void>('clipboard_panel_ready', { topId }),
  /** 记住的目标窗口进程名，用来显示「粘贴到 xxx」 */
}
