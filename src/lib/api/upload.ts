/** 图床：供应商、上传、历史。 */

import { invoke } from '@tauri-apps/api/core'
import type {
  ProviderMeta,
  UploadOutcome
} from '../types'

export const uploadApi = {
  /* 图床 */
  uploadProviders: () => invoke<ProviderMeta[]>('upload_providers'),
  uploadProviderSave: (providerId: string, values: Record<string, unknown>) =>
    invoke<void>('upload_provider_save', { providerId, values }),
  uploadSetActive: (providerId: string) => invoke<void>('upload_set_active', { providerId }),
  uploadImage: (
    dataUrl: string,
    providerId?: string,
    fileName?: string,
    dir?: string,
    origBytes?: number
  ) =>
    invoke<UploadOutcome>('upload_image', {
      dataUrl,
      providerId,
      fileName,
      dir,
      origBytes: origBytes ?? null
    }),
  uploadHistoryList: (keyword?: string, limit?: number) =>
    invoke<import('../types').UploadRecord[]>('upload_history_list', {
      keyword: keyword ?? null,
      limit: limit ?? null
    }),
  uploadHistoryRemove: (id: string) => invoke<boolean>('upload_history_remove', { id }),
  uploadHistoryClear: () => invoke<void>('upload_history_clear'),
  /** 本地图片的缩略图（后端缩小转 base64） */
  uploadThumb: (path: string, max?: number) =>
    invoke<string>('upload_thumb', { path, max: max ?? null }),
  uploadListDirs: (providerId: string, path?: string) =>
    invoke<import('../types').DirEntry[]>('upload_list_dirs', { providerId, path: path ?? null }),
  uploadListImages: (providerId: string, path?: string, limit?: number) =>
    invoke<import('../types').RemoteImage[]>('upload_list_images', {
      providerId,
      path: path ?? null,
      limit: limit ?? null
    }),
  uploadTest: (providerId: string) => invoke<UploadOutcome>('upload_test', { providerId }),
}
