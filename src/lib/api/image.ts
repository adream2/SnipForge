/** 图片：保存、复制、贴图、取字、翻译。 */

import { invoke } from '@tauri-apps/api/core'
import type {
  OcrResult
} from '../types'

export const imageApi = {
  /* 图片 */
  imageLoadFile: (path: string) =>
    invoke<import('../types').LoadedImageFile>('image_load_file', { path }),
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
  /** 翻译一段文本（取字窗口用） */
  translateText: (text: string, to?: string) =>
    invoke<string>('translate_text', { text, to: to ?? null }),
  /** 设置页的"试一下"：用当前配置翻一句固定示例 */
  translateTest: (to?: string) => invoke<string>('translate_test', { to: to ?? null }),
  /** 取字窗口关掉自己 */
  closeOcr: () => invoke<void>('ocr_close'),
  /** 取走待识别的图（窗口刚建时事件会丢，所以主动取一次） */
  ocrTakeImage: () => invoke<string | null>('ocr_take_image'),
  ocrEngines: () => invoke<import('../types').OcrEngineInfo[]>('ocr_engines'),
  ocrOpenEngineDir: () => invoke<string>('ocr_open_engine_dir'),
  ocrDownloadPaddle: () => invoke<string>('ocr_download_paddle'),
  imageInfo: (dataUrl: string) =>
    invoke<{ width: number; height: number; format: string; bytes: number }>('image_info', {
      dataUrl
    }),
  openImageFolder: (path: string) => invoke<void>('image_open_folder', { path }),
}
