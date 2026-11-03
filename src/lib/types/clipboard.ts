/** 剪贴板：历史条目、查询、统计、密钥收藏。 */

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
  /** 只看某个来源程序（进程名，带不带 .exe 都认） */
  source?: string
  limit?: number
  offset?: number
}

export interface ClipboardStats {
  total: number
  byKind: Record<string, number>
  dbBytes: number
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
