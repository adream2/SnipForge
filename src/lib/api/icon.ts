import { invoke } from '@tauri-apps/api/core'
import type { IconAsset, IconBundle } from '../types'

export const iconApi = {
  /** 从文件里提取图标。`path` 必须是磁盘上的绝对路径 */
  iconExtract: (path: string) => invoke<IconBundle>('icon_extract', { path }),
  /** 把选中的若干图标打包成一个多尺寸 .ico */
  iconBuildIco: (icons: IconAsset[], outPath: string) =>
    invoke<string>('icon_build_ico', { icons, outPath })
}
