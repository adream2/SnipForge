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

export interface ProviderMeta {
  id: string
  name: string
  origin: 'builtin' | 'plugin' | 'declarative'
  description?: string
  fields: ConfigField[]
  requiresNetwork: boolean
}

export interface AppSettings {
  version: number
  hotkeys: Record<string, string>
  save: { dir: string; nameTemplate: string; afterSave: string }
  clipboard: {
    enabled: boolean
    maxItems: number
    pollIntervalMs: number
    ignoreApps: string[]
    skipSensitive: boolean
    retentionDays: number
    captureImages: boolean
    takeoverWinV: boolean
  }
  uploader: {
    enabled: boolean
    activeProvider: string
    autoUploadOnCapture: boolean
    copyAsMarkdown: boolean
    providers: Record<string, Record<string, unknown>>
  }
  editor: {
    strokeColor: string
    strokeWidth: number
    fontSize: number
    pinOpacity: number
    watermarkText: string
    watermarkEnabled: boolean
  }
  ocr: {
    engine: string
    paddlePath: string
    timeoutSecs: number
  }
  record: {
    audio: boolean
    fps: number
    videoBitsPerSecond: number
    countdown: number
    showCursor: boolean
  }
  general: {
    language: string
    launchAtLogin: boolean
    theme: string
    minimizeToTray: boolean
    showNotifications: boolean
    enablePlugins: boolean
    closeToTray: boolean
  }
  pluginEnabled: Record<string, boolean>
}

export interface HotkeyResult {
  action: string
  accelerator: string
  ok: boolean
  reason?: string
}

export interface AppInfo {
  version: string
  platform: string
  dataDir: string
  ocrLanguages: string[]
  hotkeyResults: HotkeyResult[]
}

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

export interface ClipboardItem {
  id: string
  kind: 'text' | 'image' | 'files' | 'html' | 'color'
  text?: string
  html?: string
  blob?: string
  width?: number
  height?: number
  files?: string[]
  color?: string
  hash: string
  size: number
  /** 来源应用进程名 */
  sourceApp?: string
  pinned: boolean
  tags: string[]
  createdAt: number
  lastUsedAt: number
  hits: number
}

export interface ClipboardQuery {
  keyword?: string
  kind?: string
  pinnedOnly?: boolean
  limit?: number
  offset?: number
}

export interface ClipboardStats {
  total: number
  byKind: Record<string, number>
  dbBytes: number
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

export interface UploadResult {
  url: string
  deleteUrl?: string
  thumbUrl?: string
  meta?: unknown
}

export interface UploadOutcome {
  providerId: string
  providerName: string
  result: UploadResult
  durationMs: number
}

export interface PluginInfo {
  id: string
  name: string
  version: string
  description?: string
  author?: string
  origin: 'builtin' | 'user'
  enabled: boolean
  loaded: boolean
  error?: string
  hooks: string[]
  providers: string[]
  dir: string
}

export interface PluginState {
  plugins: PluginInfo[]
  builtinDir: string
  userDir: string
}

/** 贴图窗口的初始化数据 */
export interface PinInit {
  dataUrl: string
  opacity: number
}
