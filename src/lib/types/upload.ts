/** 图床：供应商描述、上传结果与历史。 */

import type { ConfigField } from './common'
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
