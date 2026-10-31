import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import type {
  CaptureResult,
  ClipboardItem,
  ClipboardStats,
  OcrDownloadProgress,
  PinInit,
  RecordClickEvt,
  RecordCursorEvt,
  RecordLensEvt,
  RecordResult,
} from '../types'

/** 事件订阅的薄封装：每个 on* 都返回取消函数 */
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
  onPinInit: (handler: (p: PinInit) => void) =>
    listen<PinInit>('pin://init', (e) => handler(e.payload)),
  onSelectorDisplay: (handler: (id: number) => void) =>
    listen<number>('selector://display', (e) => handler(e.payload)),
  /** 取字窗口收到新图（手势框选完直接推过来） */
  onOcrImage: (handler: (dataUrl: string) => void) =>
    listen<string>('ocr://image', (e) => handler(e.payload)),
  /** 后端把手势排除列表改了（画一下排除程序之后） */
  onGestureChanged: (handler: () => void) =>
    listen<Record<string, never>>('app://gesture-changed', () => handler()),
  onOcrDownload: (handler: (p: OcrDownloadProgress) => void) =>
    listen<OcrDownloadProgress>('ocr://download', (e) => handler(e.payload)),
  onRecordCursor: (handler: (p: RecordCursorEvt) => void) =>
    listen<RecordCursorEvt>('record://cursor', (e) => handler(e.payload)),
  onRecordClick: (handler: (p: RecordClickEvt) => void) =>
    listen<RecordClickEvt>('record://click', (e) => handler(e.payload)),
  onRecordLens: (handler: (p: RecordLensEvt) => void) =>
    listen<RecordLensEvt>('record://lens', (e) => handler(e.payload)),
  onRecordStop: (handler: (p: RecordResult) => void) =>
    listen<RecordResult>('record://stopped', (e) => handler(e.payload)),
  /** 面板选中后要粘到 SnipForge 自己的输入框（Chromium 不吃模拟的 Ctrl+V） */
  onPasteSelf: (handler: (text: string) => void) =>
    listen<string>('clipboard://paste-self', (e) => handler(e.payload))
}
