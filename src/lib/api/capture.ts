/** 截图：抓帧、窗口命中、选区提交。 */

import { invoke } from '@tauri-apps/api/core'
import type {
  CaptureResult,
  FramePayload,
  Rect
} from '../types'

export const captureApi = {
  /* 截图 */
  captureStart: (mode: string) => invoke<void>('capture_start', { mode }),
  captureFrame: () => invoke<FramePayload>('capture_frame'),
  captureWindows: () => invoke<import('../types').NativeWindowInfo[]>('capture_windows'),
  /** 光标下的窗口 / 组件（物理坐标）。whole=true 只认整个窗口 */
  captureElement: (x: number, y: number, whole: boolean) =>
    invoke<import('../types').NativeElement | null>('capture_element', { x, y, whole }),
  cursorPos: () => invoke<[number, number]>('cursor_pos'),
  captureCommit: (displayId: number, rect: Rect) =>
    invoke<CaptureResult>('capture_commit', { displayId, rect }),
  captureCancel: () => invoke<void>('capture_cancel'),
  capturePending: () => invoke<CaptureResult | null>('capture_pending'),
  /** 框选完直接贴到屏幕上（手势 ↓→↑←） */
  capturePin: (displayId: number, rect: Rect) =>
    invoke<CaptureResult>('capture_pin', { displayId, rect }),
  /** 框选完直接取字，结果弹独立小窗（手势 →↓←↑） */
  captureOcr: (displayId: number, rect: Rect) =>
    invoke<CaptureResult>('capture_ocr', { displayId, rect }),
  captureScroll: (displayId: number, rect: Rect) =>
    invoke<CaptureResult>('capture_scroll', { displayId, rect }),
  captureAbortScroll: () => invoke<void>('capture_abort_scroll'),
  closeEditor: () => invoke<void>('capture_close_editor'),
}
