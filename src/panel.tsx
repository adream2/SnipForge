import { useCallback, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { api, events } from './lib/api'
import type { ClipboardItem } from './lib/types'
import { IconClipboard, IconImage, IconLoader, IconPin, IconSearch } from './components/Icons'
import './styles/global.css'
import './styles/panel.css'

const KINDS = [
  { id: 'all', label: '全部' },
  { id: 'text', label: '文本' },
  { id: 'image', label: '图片' },
  { id: 'files', label: '文件' }
]

function relativeTime(ts: number): string {
  const diff = Date.now() - ts
  if (diff < 60_000) return '刚刚'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`
  if (diff < 7 * 86_400_000) return `${Math.floor(diff / 86_400_000)} 天前`
  return new Date(ts).toLocaleDateString('zh-CN', { month: 'numeric', day: 'numeric' })
}

/** 缩略图按需拉，面板一次只显示几十条，但也没必要全预加载 */
function Thumb({ id }: { id: string }) {
  const [src, setSrc] = useState<string | null>(null)
  useEffect(() => {
    let alive = true
    void api.clipboardThumb(id).then((url) => {
      if (alive) setSrc(url)
    })
    return () => {
      alive = false
    }
  }, [id])

  if (!src) {
    return (
      <div className="p-thumb p-thumb-loading">
        <IconLoader size={14} />
      </div>
    )
  }
  return <img className="p-thumb" src={src} alt="" draggable={false} />
}

function kindLabel(item: ClipboardItem): string {
  switch (item.kind) {
    case 'image':
      return `${item.width ?? 0}×${item.height ?? 0}`
    case 'files':
      return `${item.files?.length ?? 0} 个文件`
    case 'color':
      return item.color ?? '颜色'
    default:
      return '文本'
  }
}

function previewOf(item: ClipboardItem): string {
  if (item.kind === 'files') return (item.files ?? []).join('\n')
  if (item.kind === 'color') return item.color ?? ''
  return item.text ?? ''
}

function Panel() {
  const [items, setItems] = useState<ClipboardItem[]>([])
  const [keyword, setKeyword] = useState('')
  const [kind, setKind] = useState('all')
  const [index, setIndex] = useState(0)
  const [targetApp, setTargetApp] = useState('')
  const [loading, setLoading] = useState(true)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  /** 取数请求序号：只认最新一次的结果 */
  const reqSeq = useRef(0)

  const load = useCallback(async (kw: string, k: string) => {
    // 挂载时有两个 effect 会同时取数（一个初始化、一个按关键字防抖），
    // 加上每次呼出的 reset，很容易出现「先发的请求后回来」。
    // 那样旧列表会盖掉新列表 —— 面板上看到的条目和刚触发的同步结果不一致，
    // 用户就会「回车粘出来是上一次的内容」。用序号丢弃过期响应。
    const mine = ++reqSeq.current
    try {
      const list = await api.clipboardList({
        keyword: kw || undefined,
        kind: k === 'all' ? undefined : k,
        limit: 80
      })
      if (mine !== reqSeq.current) return
      setItems(list)
      setIndex(0)
    } finally {
      if (mine === reqSeq.current) setLoading(false)
    }
  }, [])

  // 每次呼出都重置状态：搜索框清空、光标回第一项。
  // 上一次的搜索词留着会让人以为"列表里怎么没东西"。
  const reset = useCallback(() => {
    setKeyword('')
    setKind('all')
    setLoading(true)
    void load('', 'all')
    void api.clipboardPanelTarget().then(setTargetApp).catch(() => setTargetApp(''))
    requestAnimationFrame(() => inputRef.current?.focus())
  }, [load])

  useEffect(() => {
    void load('', 'all')
    void api.clipboardPanelTarget().then(setTargetApp).catch(() => setTargetApp(''))
    inputRef.current?.focus()

    const unsubs: Array<() => void> = []
    void events.onPanelOpen(() => reset()).then((u) => unsubs.push(u))
    return () => unsubs.forEach((u) => u())
  }, [load, reset])

  // 搜索防抖：面板追求"打完就看到"，120ms 足够，再长就有滞后感
  useEffect(() => {
    const t = setTimeout(() => void load(keyword, kind), keyword ? 120 : 0)
    return () => clearTimeout(t)
  }, [keyword, kind, load])

  // 选中项滚进可视区
  useEffect(() => {
    const el = listRef.current?.querySelector<HTMLElement>(`[data-idx="${index}"]`)
    el?.scrollIntoView({ block: 'nearest' })
  }, [index])

  const paste = (item: ClipboardItem | undefined, copyOnly: boolean) => {
    if (!item) return
    // 后端会先收起面板再还原焦点，所以这里只管发请求
    void api.clipboardPanelPaste(item.id, copyOnly).catch(() => {
      /* 粘贴失败时后端已把内容写进剪贴板，用户仍可手动 Ctrl+V */
    })
  }

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      void api.clipboardPanelHide()
      return
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setIndex((i) => Math.min(i + 1, items.length - 1))
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      setIndex((i) => Math.max(i - 1, 0))
      return
    }
    if (e.key === 'Enter') {
      e.preventDefault()
      paste(items[index], e.shiftKey)
    }
  }

  return (
    <div className="panel" onKeyDown={onKeyDown}>
      <div className="p-search">
        <IconSearch size={15} />
        <input
          ref={inputRef}
          value={keyword}
          placeholder="搜索剪贴板…"
          onChange={(e) => setKeyword(e.target.value)}
          spellCheck={false}
        />
        {keyword && (
          <button className="p-clear" onClick={() => setKeyword('')} title="清空">
            ×
          </button>
        )}
      </div>

      <div className="p-kinds">
        {KINDS.map((k) => (
          <button
            key={k.id}
            className={`p-kind ${kind === k.id ? 'active' : ''}`}
            onClick={() => setKind(k.id)}
          >
            {k.label}
          </button>
        ))}
        <span className="p-count">{items.length}</span>
      </div>

      <div className="p-list" ref={listRef}>
        {loading && items.length === 0 && (
          <div className="p-empty">
            <IconLoader size={16} />
            <span>读取中…</span>
          </div>
        )}

        {!loading && items.length === 0 && (
          <div className="p-empty">
            <IconClipboard size={26} />
            <span>{keyword ? '没有匹配的记录' : '还没有记录'}</span>
          </div>
        )}

        {items.map((item, i) => (
          <div
            key={item.id}
            data-idx={i}
            className={`p-row ${i === index ? 'active' : ''}`}
            onMouseEnter={() => setIndex(i)}
            onClick={(e) => paste(item, e.shiftKey)}
            onAuxClick={(e) => {
              if (e.button === 1) {
                e.preventDefault()
                paste(item, true)
              }
            }}
          >
            {item.kind === 'image' ? (
              <Thumb id={item.id} />
            ) : item.kind === 'color' ? (
              <span className="p-swatch" style={{ background: item.color ?? '#ccc' }} />
            ) : item.kind === 'files' ? (
              <span className="p-type">
                <IconImage size={15} />
              </span>
            ) : (
              <span className="p-type">
                <IconClipboard size={15} />
              </span>
            )}

            <div className="p-body">
              <div className="p-text">{previewOf(item)}</div>
              <div className="p-meta">
                <span>{kindLabel(item)}</span>
                {item.sourceApp && (
                  <>
                    <span className="dot">·</span>
                    <span className="p-src">{item.sourceApp}</span>
                  </>
                )}
                <span className="dot">·</span>
                <span>{relativeTime(item.lastUsedAt)}</span>
                {item.pinned && <IconPin size={11} className="p-pin" />}
              </div>
            </div>
          </div>
        ))}
      </div>

      <div className="p-foot" title="按住 Shift 再按 Enter（或中键点击）= 只复制不粘贴">
        <span className="p-hint">
          <kbd>↑</kbd>
          <kbd>↓</kbd> 选择 <kbd>Enter</kbd> 粘贴 <kbd>Esc</kbd> 关闭
        </span>
        <span className="p-target">
          {targetApp ? `→ ${targetApp}` : '仅复制到剪贴板'}
        </span>
      </div>
    </div>
  )
}

// 独立窗口的挂载点
const container = document.getElementById('root')
if (container) {
  createRoot(container).render(<Panel />)
}
