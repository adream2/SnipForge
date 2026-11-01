/** 图标提取：从 exe / dll / lnk / apk / ipa 里取出的图标。 */

export interface IconAsset {
  /** data:image/png;base64,... */
  dataUrl: string
  width: number
  height: number
  /** PNG 字节数（不含 base64 膨胀） */
  bytes: number
  /** 在源文件里的位置（资源 ID / zip 路径） */
  source: string
}

export interface IconBundle {
  /** exe | dll | lnk | apk | ipa | ico | image */
  kind: string
  name: string
  path: string
  icons: IconAsset[]
  /** 提取过程中的说明（含降级原因） */
  notes: string[]
}
