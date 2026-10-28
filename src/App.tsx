import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { api, events } from './lib/api'
import type { AppSettings, ClipboardStats } from './lib/types'
import {
  IconClipboard,
  IconImage,
  IconVideo,
  IconUpload,
  IconPlugin,
  IconSettings,
  IconMinus,
  IconSquare,
  IconX
} from './components/Icons'
import ClipboardPage from './pages/ClipboardPage'
import RecordPage from './pages/RecordPage'
import ToolsPage from './pages/ToolsPage'
import UploadPage from './pages/UploadPage'
import PluginsPage from './pages/PluginsPage'
import SettingsPage from './pages/SettingsPage'
import { ToastProvider, useToast } from './components/Toast'

/**
 * 把文本插到输入框的光标处。
 *
 * React 的受控输入框直接改 value 不会触发 onChange（React 记着自己那份值），
 * 必须走原型上的原生 setter，再手动派发 input 事件，React 才会同步状态。
 */
function insertIntoInput(el: HTMLInputElement | HTMLTextAreaElement, text: string) {
  const proto =
    el instanceof HTMLTextAreaElement ? HTMLTextAreaElement.prototype : HTMLInputElement.prototype
  const setter = Object.getOwnPropertyDescriptor(proto, 'value')?.set
  const start = el.selectionStart ?? el.value.length
  const end = el.selectionEnd ?? start
  setter?.call(el, el.value.slice(0, start) + text + el.value.slice(end))
  el.dispatchEvent(new Event('input', { bubbles: true }))
  const pos = start + text.length
  try {
    el.focus()
    el.setSelectionRange(pos, pos)
  } catch {
    /* number / email 这类 input 不支持 setSelectionRange */
  }
}

type Page =
  | 'clipboard'
  | 'record'
  | 'tools'
  | 'upload'
  | 'plugins'
  | 'settings'

const NAV: Array<{ id: Page; label: string; icon: React.ReactNode }> = [
  { id: 'clipboard', label: '剪贴板', icon: <IconClipboard size={16} /> },
  { id: 'record', label: '录屏', icon: <IconVideo size={16} /> },
  { id: 'tools', label: '图片工具', icon: <IconImage size={16} /> },
  { id: 'upload', label: '图床', icon: <IconUpload size={16} /> },
  { id: 'plugins', label: '插件', icon: <IconPlugin size={16} /> },
  { id: 'settings', label: '设置', icon: <IconSettings size={16} /> }
]

function Shell() {
  const [page, setPage] = useState<Page>('clipboard')
  const [stats, setStats] = useState<ClipboardStats | null>(null)
  const [settings, setSettings] = useState<AppSettings | null>(null)
  const toast = useToast()

  /** 最近聚焦过的输入框。面板粘贴时窗口会短暂失焦，document.activeElement 可能已经跑了 */
  const lastInputRef = useRef<HTMLInputElement | HTMLTextAreaElement | null>(null)

  useEffect(() => {
    const onFocusIn = (e: FocusEvent) => {
      const t = e.target as HTMLElement | null
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')) {
        lastInputRef.current = t as HTMLInputElement | HTMLTextAreaElement
      }
    }
    document.addEventListener('focusin', onFocusIn)
    return () => document.removeEventListener('focusin', onFocusIn)
  }, [])

  useEffect(() => {
    void api.settingsGet().then((r) => setSettings(r.settings))
    void api.clipboardStats().then(setStats)
  }, [])

  useEffect(() => {
    const unsubs: Array<() => void> = []
    void events.onClipboardStats((s) => setStats(s)).then((u) => unsubs.push(u))
    void events.onNavigate((route) => {
      if (NAV.some((n) => n.id === route)) setPage(route as Page)
    }).then((u) => unsubs.push(u))
    void events
      .onToast((t) => {
        toast.push(t.message, t.type === 'error' ? 'error' : t.type === 'success' ? 'success' : 'info')
      })
      .then((u) => unsubs.push(u))
    // 录制结束：给个明确反馈 + 直接打开所在文件夹（找不到就只提示）
    void events
      .onRecordStop((r) => {
        const mb = (r.bytes / 1048576).toFixed(1)
        const gif = r.gifPath
          ? `；GIF ${(r.gifBytes / 1048576).toFixed(1)} MB`
          : ''
        toast.push(
          `录制已保存：${r.path}（${mb} MB / ${Math.round(r.durationMs / 1000)}s${gif}）`,
          'success'
        )
      })
      .then((u) => unsubs.push(u))
    void events
      .on<string>('record://error', (msg) => toast.push(`录制失败：${msg}`, 'error'))
      .then((u) => unsubs.push(u))
    // 面板选中后要粘到 SnipForge 自己的输入框。
    // Chromium 会丢弃程序模拟的 Ctrl+V（安全策略），所以后端把文本推过来，
    // 由页面插进最近聚焦的那个输入框。
    void events
      .onPasteSelf((text) => {
        const el = lastInputRef.current
        if (!el || !el.isConnected) {
          toast.push('内容已复制，手动 Ctrl+V 即可', 'info')
          return
        }
        insertIntoInput(el, text)
      })
      .then((u) => unsubs.push(u))

    return () => unsubs.forEach((u) => u())
  }, [toast])

  const updateSettings = useCallback(async (patch: unknown) => {
    const r = await api.settingsPatch(patch)
    setSettings(r.settings)
    return r.settings
  }, [])

  const win = getCurrentWindow()

  const sidebarCount = useMemo(() => {
    if (!stats) return null
    return stats.total > 0 ? String(stats.total) : null
  }, [stats])

  return (
    <div className="app">
      <div style={{ display: 'flex', flexDirection: 'column', width: '100%' }}>
        <div className="titlebar">
          <div className="drag" data-tauri-drag-region>
            <span className="brand">SnipForge</span>
          </div>
          <div className="win-actions">
            <button onClick={() => void win.minimize()} title="最小化">
              <IconMinus />
            </button>
            <button onClick={() => void win.toggleMaximize()} title="最大化">
              <IconSquare />
            </button>
            <button className="close" onClick={() => void win.close()} title="关闭到托盘">
              <IconX />
            </button>
          </div>
        </div>

        <div style={{ display: 'flex', flex: 1, minHeight: 0 }}>
          <aside className="sidebar">
            <div style={{ height: 4 }} />
            {NAV.map((item) => (
              <button
                key={item.id}
                className={`nav-item ${page === item.id ? 'active' : ''}`}
                onClick={() => setPage(item.id)}
              >
                {item.icon}
                <span>{item.label}</span>
                {item.id === 'clipboard' && sidebarCount && (
                  <span className="count">{sidebarCount}</span>
                )}
              </button>
            ))}
            <div className="sidebar-footer">
              本地优先 · 不上传任何数据
              {stats && (
                <>
                  <br />
                  {stats.total} 条记录 · {(stats.dbBytes / 1024 / 1024).toFixed(1)} MB
                </>
              )}
            </div>
          </aside>

          <main className="content">
            {page === 'clipboard' && <ClipboardPage onStats={setStats} />}
            {page === 'record' && settings && (
              <RecordPage settings={settings} onChange={updateSettings} />
            )}
            {page === 'tools' && <ToolsPage />}
            {page === 'upload' && <UploadPage settings={settings} onChange={updateSettings} />}
            {page === 'plugins' && <PluginsPage />}
            {page === 'settings' && <SettingsPage settings={settings} onChange={updateSettings} />}
          </main>
        </div>
      </div>
    </div>
  )
}

export default function App() {
  return (
    <ToastProvider>
      <Shell />
    </ToastProvider>
  )
}
