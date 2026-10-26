import { invoke } from '@tauri-apps/api/core'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  AppInfo,
  AppSettings,
  ClipboardItem,
  ClipboardQuery,
  ClipboardStats,
  CaptureResult,
  FramePayload,
  OcrResult,
  PluginState,
  ProviderMeta,
  Rect,
  RecordFormat,
  RecordResult,
  RecordStatus,
  UploadOutcome
} from './types'

/** 所有后端能力的唯一入口。组件里不直接写 invoke 字符串。 */
export const api = {
  /* 设置 */
  settingsGet: () => invoke<{ settings: AppSettings }>('settings_get'),
  settingsPatch: (patch: unknown) => invoke<{ settings: AppSettings }>('settings_patch', { patch }),
  settingsReset: () => invoke<{ settings: AppSettings }>('settings_reset'),
  pickDir: (defaultPath?: string) => invoke<string | null>('settings_pick_dir', { defaultPath }),
  appInfo: () => invoke<AppInfo>('app_info'),
  showMain: (route?: string) => invoke<void>('app_show_main', { route: route ?? null }),
  openDir: (path: string) => invoke<void>('app_open_dir', { path }),
  openPath: (path: string) => invoke<void>('app_open_path', { path }),

  /* 截图 */
  captureStart: (mode: string) => invoke<void>('capture_start', { mode }),
  captureFrame: () => invoke<FramePayload>('capture_frame'),
  captureWindows: () => invoke<import('./types').NativeWindowInfo[]>('capture_windows'),
  cursorPos: () => invoke<[number, number]>('cursor_pos'),
  captureCommit: (displayId: number, rect: Rect) =>
    invoke<CaptureResult>('capture_commit', { displayId, rect }),
  captureCancel: () => invoke<void>('capture_cancel'),
  capturePending: () => invoke<CaptureResult | null>('capture_pending'),
  captureScroll: (displayId: number, rect: Rect) =>
    invoke<CaptureResult>('capture_scroll', { displayId, rect }),
  captureAbortScroll: () => invoke<void>('capture_abort_scroll'),
  closeEditor: () => invoke<void>('capture_close_editor'),

  /* 图片 */
  imageLoadFile: (path: string) =>
    invoke<import('./types').LoadedImageFile>('image_load_file', { path }),
  imageSave: (dataUrl: string, fileName?: string, openFolder?: boolean) =>
    invoke<{ path: string; bytes: number; format: string }>('image_save', {
      dataUrl,
      fileName,
      openFolder
    }),
  imageSaveAs: (dataUrl: string, suggested?: string) =>
    invoke<string | null>('image_save_as', { dataUrl, suggested }),
  imageCopy: (dataUrl: string) => invoke<void>('image_copy', { dataUrl }),
  /** 把前端异常写进后端日志文件 */
  log: (level: 'info' | 'warn' | 'error', message: string) =>
    invoke<void>('app_log', { level, message }),
  /** 钉图：把图片钉在屏幕最上层 */
  imagePin: (dataUrl: string) => invoke<void>('image_pin', { dataUrl }),
  imageOcr: (dataUrl: string) => invoke<OcrResult>('image_ocr', { dataUrl }),
  ocrEngines: () => invoke<import('./types').OcrEngineInfo[]>('ocr_engines'),
  ocrOpenEngineDir: () => invoke<string>('ocr_open_engine_dir'),
  ocrDownloadPaddle: () => invoke<string>('ocr_download_paddle'),
  imageInfo: (dataUrl: string) =>
    invoke<{ width: number; height: number; format: string; bytes: number }>('image_info', {
      dataUrl
    }),
  openImageFolder: (path: string) => invoke<void>('image_open_folder', { path }),

  /* 剪贴板 */
  clipboardList: (query: ClipboardQuery = {}) =>
    invoke<ClipboardItem[]>('clipboard_list', { query }),
  clipboardThumb: (id: string) => invoke<string | null>('clipboard_thumb', { id }),
  clipboardImageData: (id?: string) =>
    invoke<string>('clipboard_image_data', { id: id ?? null }),
  clipboardStats: () => invoke<ClipboardStats>('clipboard_stats'),
  clipboardPaste: (id: string, autoPaste = false) =>
    invoke<string>('clipboard_paste', { id, autoPaste }),
  clipboardTogglePin: (id: string) => invoke<ClipboardItem | null>('clipboard_toggle_pin', { id }),
  clipboardRemove: (id: string) => invoke<boolean>('clipboard_remove', { id }),
  clipboardClear: (keepPinned: boolean) => invoke<void>('clipboard_clear', { keepPinned }),
  clipboardCopyText: (text: string) => invoke<void>('clipboard_copy_text', { text }),
  clipboardOpenFile: (id: string) => invoke<void>('clipboard_open_file', { id }),
  clipboardPin: (id: string) => invoke<void>('clipboard_pin', { id }),
  clipboardSetTakeoverWinV: (enabled: boolean) =>
    invoke<boolean>('clipboard_set_takeover_win_v', { enabled }),
  clipboardTakeoverWinVStatus: () => invoke<boolean>('clipboard_takeover_win_v_status'),
  /** 面板里选中一条：写剪贴板 → 收起面板 → 还原焦点 → Ctrl+V */
  clipboardPanelPaste: (id: string, copyOnly = false) =>
    invoke<string>('clipboard_panel_paste', { id, copyOnly }),
  clipboardPanelHide: () => invoke<void>('clipboard_panel_hide'),
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
  recordSetLens: (on: boolean) => invoke<void>('record_set_lens', { on }),
  /** 标注模式：覆盖层开始接收鼠标 */
  recordSetInteractive: (interactive: boolean) =>
    invoke<void>('record_set_interactive', { interactive }),
  /** 列表加载完了，通知后端解除"预热"（预热期攒下的回车会在后端补做） */
  clipboardPanelReady: (topId: string | null) =>
    invoke<void>('clipboard_panel_ready', { topId }),
  /** 记住的目标窗口进程名，用来显示「粘贴到 xxx」 */
  clipboardPanelTarget: () => invoke<string>('clipboard_panel_target'),
  clipboardSystemHistoryGet: () => invoke<boolean>('clipboard_system_history_get'),
  clipboardSystemHistorySet: (enabled: boolean) =>
    invoke<boolean>('clipboard_system_history_set', { enabled }),

  /* 图床 */
  uploadProviders: () => invoke<ProviderMeta[]>('upload_providers'),
  uploadProviderSave: (providerId: string, values: Record<string, unknown>) =>
    invoke<void>('upload_provider_save', { providerId, values }),
  uploadSetActive: (providerId: string) => invoke<void>('upload_set_active', { providerId }),
  uploadImage: (dataUrl: string, providerId?: string, fileName?: string) =>
    invoke<UploadOutcome>('upload_image', { dataUrl, providerId, fileName }),
  uploadTest: (providerId: string) => invoke<UploadOutcome>('upload_test', { providerId }),

  /* 插件 */
  pluginsState: () => invoke<PluginState>('plugins_state'),
  pluginsToggle: (id: string, enabled: boolean) =>
    invoke<PluginState>('plugins_toggle', { id, enabled }),
  pluginsReload: () => invoke<PluginState>('plugins_reload'),
  pluginsOpenDir: (which?: 'user' | 'builtin') =>
    invoke<void>('plugins_open_dir', { which })
}

/** 事件订阅的薄封装，统一返回取消函数 */
export const events = {
  on: <T>(name: string, handler: (payload: T) => void): Promise<UnlistenFn> =>
    listen<T>(name, (e) => handler(e.payload)),
  onNavigate: (handler: (route: string) => void) => listen<string>('app://navigate', (e) => handler(e.payload)),
  onToast: (handler: (t: { type: string; message: string }) => void) =>
    listen<{ type: string; message: string }>('app://toast', (e) => handler(e.payload)),
  onClipboardNew: (handler: (item: ClipboardItem) => void) =>
    listen<ClipboardItem>('clipboard://new', (e) => handler(e.payload)),
  onPanelOpen: (handler: () => void) => listen('panel://open', () => handler()),
  onClipboardStats: (handler: (s: ClipboardStats) => void) =>
    listen<ClipboardStats>('clipboard://stats', (e) => handler(e.payload)),
  onEditorCapture: (handler: (c: CaptureResult) => void) =>
    listen<CaptureResult>('editor://capture', (e) => handler(e.payload)),
  onScrollProgress: (handler: (p: unknown) => void) =>
    listen('capture://scroll-progress', (e) => handler(e.payload)),
  onPinInit: (handler: (p: import('./types').PinInit) => void) =>
    listen<import('./types').PinInit>('pin://init', (e) => handler(e.payload)),
  onSelectorDisplay: (handler: (id: number) => void) =>
    listen<number>('selector://display', (e) => handler(e.payload)),
  onOcrDownload: (handler: (p: import('./types').OcrDownloadProgress) => void) =>
    listen<import('./types').OcrDownloadProgress>('ocr://download', (e) => handler(e.payload)),
  onRecordCursor: (handler: (p: import('./types').RecordCursorEvt) => void) =>
    listen<import('./types').RecordCursorEvt>('record://cursor', (e) => handler(e.payload)),
  onRecordClick: (handler: (p: import('./types').RecordClickEvt) => void) =>
    listen<import('./types').RecordClickEvt>('record://click', (e) => handler(e.payload)),
  onRecordLens: (handler: (p: import('./types').RecordLensEvt) => void) =>
    listen<import('./types').RecordLensEvt>('record://lens', (e) => handler(e.payload)),
  onRecordStop: (handler: (p: import('./types').RecordResult) => void) =>
    listen<import('./types').RecordResult>('record://stopped', (e) => handler(e.payload))
}
