/** 基础类型：坐标、目录、应用信息 —— 谁都能用，不依赖别的模块。 */

export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

export interface Point {
  x: number
  y: number
}

export interface ConfigField {
  key: string
  label: string
  fieldType: 'string' | 'password' | 'number' | 'boolean' | 'select' | 'textarea'
  placeholder?: string
  description?: string
  required: boolean
  default?: unknown
  options?: Array<{ label: string; value: string }>
  visibleWhen?: { key: string; equals: unknown }
}

/** 图床里的一个目录（浏览用） */
export interface DirEntry {
  name: string
  path: string
}

export interface AppInfo {
  version: string
  platform: string
  dataDir: string
  ocrLanguages: string[]
  hotkeyResults: HotkeyResult[]
}

export interface HotkeyResult {
  /** 配置字段名（regionCapture 等），匹配用这个，别用 action 文案 */
  id: string
  action: string
  accelerator: string
  ok: boolean
  reason?: string
}

/** 贴图窗口的初始化数据 */
export interface PinInit {
  dataUrl: string
  opacity: number
}

/* ---------------- 录屏 ---------------- */
