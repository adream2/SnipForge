/** 剪贴板：历史、面板、密钥收藏。 */

import { invoke } from '@tauri-apps/api/core'
import type {
  ClipboardItem,
  ClipboardQuery,
  ClipboardStats
} from '../types'

export const clipboardApi = {
  /* 剪贴板 */
  clipboardList: (query: ClipboardQuery = {}) =>
    invoke<ClipboardItem[]>('clipboard_list', { query }),
  clipboardThumb: (id: string) => invoke<string | null>('clipboard_thumb', { id }),
  clipboardImageData: (id?: string) =>
    invoke<string>('clipboard_image_data', { id: id ?? null }),
  clipboardStats: () => invoke<ClipboardStats>('clipboard_stats'),
  clipboardPaste: (id: string, autoPaste = false, plain = false) =>
    invoke<string>('clipboard_paste', { id, autoPaste, plain }),
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
  clipboardPanelPaste: (id: string, copyOnly = false, plain = false) =>
    invoke<string>('clipboard_panel_paste', { id, copyOnly, plain }),
  clipboardPanelHide: () => invoke<void>('clipboard_panel_hide'),
  /* 密钥/密码收藏 */
  secretsList: () => invoke<import('../types').Secret[]>('secrets_list'),
  secretsSave: (p: { id?: string; name: string; value: string; note?: string }) =>
    invoke<import('../types').Secret>('secrets_save', {
      id: p.id ?? null,
      name: p.name,
      value: p.value,
      note: p.note ?? null
    }),
  /** 只改名字，不影响排序（不会跳到列表最前） */
  secretsRename: (id: string, name: string) =>
    invoke<import('../types').Secret>('secrets_rename', { id, name }),
  secretsRemove: (id: string) => invoke<boolean>('secrets_remove', { id }),
  secretPaste: (id: string, copyOnly?: boolean) =>
    invoke<void>('secret_paste', { id, copyOnly: copyOnly ?? null }),

  clipboardPanelTarget: () => invoke<string>('clipboard_panel_target'),
  clipboardSystemHistoryGet: () => invoke<boolean>('clipboard_system_history_get'),
  clipboardSystemHistorySet: (enabled: boolean) =>
    invoke<boolean>('clipboard_system_history_set', { enabled }),
}
