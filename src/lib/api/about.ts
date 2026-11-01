/** 「关于」页的外链。地址在 Rust 侧集中维护，这里只传名字。 */

import { invoke } from '@tauri-apps/api/core'

export type AboutTarget = 'site' | 'repo' | 'releases' | 'issues' | 'changelog' | 'license'

export const aboutApi = {
  /** 用系统默认浏览器打开。后端只认上面这六个名字 */
  aboutOpen: (target: AboutTarget) => invoke<void>('about_open', { target }),
  /** 名字 → 地址。界面要显示地址时用，保证和实际打开的一致 */
  aboutLinks: () => invoke<[AboutTarget, string][]>('about_links')
}
