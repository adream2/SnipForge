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

/** 图床里已有的一张图 */
export interface RemoteImage {
  name: string
  /** 图床里的完整路径 / 对象键 */
  path: string
  url: string
  size: number
  /** GitHub contents API 不返回时间，可能为空 */
  modifiedAt?: number
  sha?: string
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
  /** 能否列出已有图片 */
  canListImages?: boolean
}

/** 手势能做的事。和 Rust 的 `GestureAction` 一一对应（serde tag = type）。 */
export type GestureAction =
  /** 发送按键，如 "Ctrl+Shift+T"；多组用逗号分隔 */
  | { type: 'keys'; keys: string }
  /** 当前窗口操作 */
  | { type: 'window'; op: string }
  /** 打开程序 / 文件 / 文件夹 */
  | { type: 'app'; path: string }
  /** 打开网址 */
  | { type: 'url'; url: string }
  /** 运行命令 */
  | { type: 'cmd'; cmd: string }
  /** 媒体与系统键 */
  | { type: 'media'; key: string }
  /** 输入文字 */
  | { type: 'text'; text: string }
  /** 滚动 */
  | { type: 'scroll'; dir: string; amount: number }
  /** 本工具的功能 */
  | { type: 'snip'; id: string }

export interface GestureBinding {
  /** 方向串（U/D/L/R，如 "DR"）、形状字符（"S"）；自定义手势是空串 */
  pattern: string
  /**
   * 用户自己画的手势模板（32 点，等比归一化）。
   * 非空就是"自定义手势"，按形状匹配，不看方向串。
   */
  points?: number[][]
  action: GestureAction
  enabled: boolean
}

export interface GestureConfig {
  enabled: boolean
  /** right / middle / x1 */
  trigger: string
  /** 移动超过多少像素才算手势 */
  threshold: number
  /** 判一次转向所需位移 */
  step: number
  showTrail: boolean
  /** 轨迹颜色 #RRGGBB */
  trailColor: string
  /** 前台窗口全屏时自动暂停 */
  pauseFullscreen: boolean
  /** 把触发键的按下也吞掉（默认开；关掉退回只吞抬起） */
  swallowDown: boolean
  /** 不响应手势的程序（exe 文件名，小写） */
  excluded: string[]
  /** 在这个程序里划一下就能加进排除列表（默认 RLR = Z 形） */
  excludePattern: string
  bindings: GestureBinding[]
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
  gestures: GestureConfig
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

export interface ClipboardItem {
  id: string
  kind: 'text' | 'link' | 'image' | 'files' | 'html' | 'color'
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

/** 一条密钥/密码收藏 */
export interface Secret {
  id: string
  /** 名称，例如「GitHub Token」 */
  name: string
  /** 真正要粘出去的内容 */
  value: string
  /** 备注：用在哪、什么权限，可空 */
  note: string
  createdAt: number
  updatedAt: number
}

/** 一个已录好的文件 */
export interface RecordFile {
  name: string
  path: string
  bytes: number
  /** 毫秒时间戳 */
  createdAt: number
  /** mp4 | gif */
  format: string
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
