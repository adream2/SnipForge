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

export interface ProviderMeta {
  id: string
  name: string
  origin: 'builtin' | 'plugin' | 'declarative'
  description?: string
  fields: ConfigField[]
  requiresNetwork: boolean
  /** 能否给单次上传指定目录 */
  supportsDir?: boolean
  /** 能否列出已有目录 */
  canBrowse?: boolean
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
    /** 上传前压缩（canvas 做，能出有损 WebP） */
    compress: boolean
    /** keep | webp | jpeg | png */
    compressFormat: string
    compressQuality: number
    /** 最长边上限，0 = 不限 */
    compressMaxEdge: number
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
    cursorHighlight: boolean
    magnifier: boolean
    annotate: boolean
    gifFps: number
    /** GIF 输出最大宽度，0 = 原尺寸 */
    gifWidth: number
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
  /** 配置字段名（regionCapture 等），匹配用这个，别用 action 文案 */
  id: string
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

/** 一条上传记录（落盘，重启后还在） */
export interface UploadRecord {
  id: string
  url: string
  deleteUrl?: string
  name: string
  /** 实际传上去的体积 */
  bytes: number
  /** 压缩前的体积，0 = 未知 */
  origBytes: number
  format: string
  providerId: string
  providerName: string
  dir?: string
  /** 毫秒时间戳 */
  createdAt: number
  /** 本地目录图床的真实磁盘路径 */
  localPath?: string
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

/* ---------------- 录屏 ---------------- */

/** 录制格式：跟着录制动作走，不是全局设置 */
export type RecordFormat = 'mp4' | 'gif'

export interface RecordStatus {
  running: boolean
  paused: boolean
  /** 倒计时结束后才为 true */
  started: boolean
  lens: boolean
  elapsedMs: number
  frames: number
  fps: number
  path: string
  width: number
  height: number
  /** 这一路是否在同时写 GIF */
  gif: boolean
  /** 是否在写 MP4 */
  mp4: boolean
}

export interface RecordResult {
  /** 主产物：有 MP4 就是 MP4，只出 GIF 时是 GIF */
  path: string
  durationMs: number
  frames: number
  bytes: number
  width: number
  height: number
  /** 同时产出的 GIF，没开就是 null */
  gifPath: string | null
  gifBytes: number
  gifFrames: number
  gifWidth: number
  gifHeight: number
}

/** 后端从磁盘读出来的图片（带 data URL，前端直接显示） */
export interface LoadedImageFile {
  dataUrl: string
  width: number
  height: number
  format: string
  bytes: number
}

export interface RecordCursorEvt {
  /** 区域相对坐标（物理像素） */
  x: number
  y: number
  recording: boolean
}

export interface RecordClickEvt {
  x: number
  y: number
  button: string
}

export interface RecordLensEvt {
  url: string
  x: number
  y: number
}
