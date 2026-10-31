/** 截图 / 取字：帧数据、窗口命中、OCR 结果。 */

import type { Rect } from './common'
export interface CaptureResult {
  dataUrl: string
  width: number
  height: number
  scaleFactor: number
  source: string
  timestamp: number
}

export interface DisplayPreview {
  id: number
  bounds: Rect
  scaleFactor: number
  preview: string
}

export interface FramePayload {
  union: Rect
  displays: DisplayPreview[]
}

export interface NativeWindowInfo {
  hwnd: number
  title: string
  className: string
  rect: Rect
  exe: string
  ignored: boolean
}

/**
 * 光标底下的窗口 / 组件（区域截图高亮用）。
 *
 * `kind = "component"` 表示钻进了子窗口（按钮、编辑框、页面渲染区……），
 * 这种时候点一下选的是这个组件，按住 Ctrl 才退回到整个窗口。
 */

export interface NativeElement {
  hwnd: number
  rect: Rect
  kind: 'window' | 'component'
  title: string
  className: string
  windowTitle: string
  exe: string
}

export interface OcrWord {
  text: string
  x: number
  y: number
  width: number
  height: number
}

export interface OcrLine {
  text: string
  words: OcrWord[]
}

export interface OcrEngineInfo {
  id: string
  name: string
  available: boolean
  detail: string
  languages: string[]
  builtin: boolean
}

export interface OcrDownloadProgress {
  stage: 'resolving' | 'downloading' | 'extracting' | 'done'
  downloaded: number
  total: number
  message: string
}

export interface OcrResult {
  text: string
  lines: OcrLine[]
  language: string
  durationMs: number
}

/** 后端从磁盘读出来的图片（带 data URL，前端直接显示） */
export interface LoadedImageFile {
  dataUrl: string
  width: number
  height: number
  format: string
  bytes: number
}
