import { useCallback, useEffect, useRef, useState } from 'react'
import { api, events } from '../lib/api'
import type { ClipboardItem, ClipboardStats } from '../lib/types'
import { IconCheck, IconCopy, IconPin, IconSearch, IconTrash, IconClipboard, IconLoader } from '../components/Icons'
import { autoName } from '../lib/naming'
import { maskSensitive } from '../lib/mask'
import { useToast } from '../components/Toast'
import SecretsDialog from '../components/SecretsDialog'

const KINDS = [
  { id: 'all', label: '全部' },
  { id: 'link', label: '链接' },
  { id: 'text', label: '文本' },
  { id: 'image', label: '图片' },
  { id: 'files', label: '文件' },
  { id: 'color', label: '颜色' }
]

function relativeTime(ts: number): string {
  const diff = Date.now() - ts
  if (diff < 60_000) return '刚刚'
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)} 分钟前`
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)} 小时前`
  if (diff < 7 * 86_400_000) return `${Math.floor(diff / 86_400_000)} 天前`
  return new Date(ts).toLocaleDateString('zh-CN', { month: 'numeric', day: 'numeric' })
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

/** 缩略图懒加载：只有在列表里时才去后端取图，避免一次拉几百张 */
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
      <div
        className="thumb"
        style={{ display: 'grid', placeItems: 'center', color: '#c4c4cc' }}
      >
        <IconLoader size={18} />
      </div>
    )
  }
  return <img className="thumb" src={src} alt="" loading="lazy" draggable={false} />
}

export default function ClipboardPage({ onStats }: { onStats: (s: ClipboardStats) => void }) {
  const [items, setItems] = useState<ClipboardItem[]>([])
  /** 密钥/密码收藏的管理弹窗 */
  const [showSecrets, setShowSecrets] = useState(false)
  /** 刚存为收藏的条目，按钮短暂变成勾 */
  const [starred, setStarred] = useState<string | null>(null)
  const [keyword, setKeyword] = useState('')
  const [kind, setKind] = useState('all')
  /** 按来源程序筛选：点一下卡片底部的程序名就会切进来 */
  const [source, setSource] = useState<string | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const toast = useToast()
  const searchRef = useRef<HTMLInputElement>(null)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      const list = await api.clipboardList({
        keyword: keyword || undefined,
        kind: kind === 'all' ? undefined : kind,
        source: source || undefined,
        limit: 200
      })
      setItems(list)
      onStats(await api.clipboardStats())
    } finally {
      setLoading(false)
    }
  }, [keyword, kind, source, onStats])

  useEffect(() => {
    const t = setTimeout(() => void refresh(), keyword ? 180 : 0)
    return () => clearTimeout(t)
  }, [refresh, keyword])

  // 新条目进来时只做增量提示，不打断用户当前操作
  useEffect(() => {
    const unsubs: Array<() => void> = []
    void events
      .onClipboardNew(() => {
        void refresh()
      })
      .then((u) => unsubs.push(u))
    return () => unsubs.forEach((u) => u())
  }, [refresh])

  /** 把这条内容存进收藏。名字自动取，之后在收藏里可以改。 */
  const saveToSecrets = async (item: ClipboardItem) => {
    const text = item.text?.trim()
    if (!text) return
    const name = autoName(text)
    try {
      await api.secretsSave({ name, value: text })
      setStarred(item.id)
      window.setTimeout(() => setStarred((v) => (v === item.id ? null : v)), 1500)
      toast.push(`已收藏：${name}`, 'success')
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const handlePaste = async (item: ClipboardItem, autoPaste: boolean) => {
    try {
      await api.clipboardPaste(item.id, autoPaste)
      toast.push(autoPaste ? '已粘贴到前台窗口' : '已复制到剪贴板', 'success')
      void refresh()
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const handleKey = (e: React.KeyboardEvent) => {
    if (!items.length) return
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const idx = items.findIndex((i) => i.id === selected)
      const next = e.key === 'ArrowDown' ? Math.min(idx + 1, items.length - 1) : Math.max(idx - 1, 0)
      setSelected(items[next < 0 ? 0 : next].id)
    }
    if (e.key === 'Enter' && selected) {
      const item = items.find((i) => i.id === selected)
      if (item) void handlePaste(item, true)
    }
  }

  return (
    <>
      <div className="page-head">
        <h1>剪贴板</h1>
        <div className="spacer" />
        <button className="btn sm" onClick={() => setShowSecrets(true)}>
          🔑 收藏
        </button>
        <button className="btn sm danger" onClick={() => void api.clipboardClear(true).then(refresh)}>
          清空（保留置顶）
        </button>
      </div>

      <div className="clip-toolbar">
        <div className="search">
          <IconSearch />
          <input
            ref={searchRef}
            placeholder="搜索文本、文件名…"
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
            onKeyDown={handleKey}
          />
        </div>
        <div className="seg">
          {KINDS.map((k) => (
            <button
              key={k.id}
              className={kind === k.id ? 'active' : ''}
              onClick={() => setKind(k.id)}
            >
              {k.label}
            </button>
          ))}
        </div>
        {source && (
          <button
            className="btn sm"
            title="清除来源筛选"
            onClick={() => setSource(null)}
            style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}
          >
            来自 {source}
            <span aria-hidden>✕</span>
          </button>
        )}
      </div>

      <div className="page-body">
        {loading && items.length === 0 && (
          <div className="empty">
            <IconLoader size={20} />
            <span>读取中…</span>
          </div>
        )}

        {!loading && items.length === 0 && (
          <div className="empty">
            <IconClipboard size={34} />
            <span>{keyword ? '没有匹配的记录' : '还没有记录，复制点什么试试'}</span>
            <span style={{ fontSize: 11.5 }}>监听已开启，后台会自动收录</span>
          </div>
        )}

        <div className="clip-grid">
          {items.map((item) => (
            <div
              key={item.id}
              className={`clip-card ${selected === item.id ? 'selected' : ''}`}
              onClick={() => setSelected(item.id)}
              onDoubleClick={() => void handlePaste(item, true)}
            >
              {item.pinned && (
                <span className="pin-flag">
                  <IconPin size={12} />
                </span>
              )}

              <div className="actions" onClick={(e) => e.stopPropagation()}>
                <button title="复制" onClick={() => void handlePaste(item, false)}>
                  <IconCopy />
                </button>
                {item.text?.trim() && (
                  <button
                    title="存为收藏（之后可在「收藏」里改名）"
                    onClick={() => void saveToSecrets(item)}
                  >
                    {starred === item.id ? <IconCheck size={12} /> : <span>🔑</span>}
                  </button>
                )}
                <button
                  title={item.pinned ? '取消置顶' : '置顶'}
                  onClick={() => void api.clipboardTogglePin(item.id).then(refresh)}
                >
                  <IconPin />
                </button>
                <button
                  title="删除"
                  onClick={() => void api.clipboardRemove(item.id).then(refresh)}
                >
                  <IconTrash />
                </button>
              </div>

              {item.kind === 'image' ? (
                <Thumb id={item.id} />
              ) : item.kind === 'color' ? (
                <div
                  className="color-swatch"
                  style={{ background: item.color ?? '#ccc' }}
                  title={item.color}
                />
              ) : (
                <div className="body">
                  {maskSensitive(item.text ?? (item.files ?? []).join('\n'))}
                </div>
              )}

              <div className="meta">
                <span className="kind">
                  {item.kind === 'image'
                    ? `${item.width}×${item.height}`
                    : item.kind === 'files'
                      ? `${item.files?.length ?? 0} 个文件`
                      : item.kind === 'color'
                        ? item.color
                        : item.kind === 'link'
                          ? '链接'
                          : '文本'}
                </span>
                <span>·</span>
                <span>{formatSize(item.size)}</span>
                {item.html && (
                  <>
                    <span>·</span>
                    <span title="粘贴时保留格式（同时写入纯文本兜底）">富文本</span>
                  </>
                )}
                {item.sourceApp && (
                  <>
                    <span>·</span>
                    <button
                      className="source-chip"
                      title={`只看来自 ${item.sourceApp} 的记录`}
                      onClick={(e) => {
                        e.stopPropagation()
                        setSource((prev) => (prev === item.sourceApp ? null : item.sourceApp!))
                      }}
                      style={{
                        maxWidth: 76,
                        overflow: 'hidden',
                        textOverflow: 'ellipsis',
                        whiteSpace: 'nowrap',
                        background: 'none',
                        border: 0,
                        padding: 0,
                        font: 'inherit',
                        cursor: 'pointer',
                        color:
                          source === item.sourceApp
                            ? 'var(--accent, #6c5ce7)'
                            : 'var(--text-2)'
                      }}
                    >
                      {item.sourceApp}
                    </button>
                  </>
                )}
                <span style={{ marginLeft: 'auto' }}>{relativeTime(item.lastUsedAt)}</span>
              </div>
            </div>
          ))}
        </div>
      </div>

      {showSecrets && <SecretsDialog onClose={() => setShowSecrets(false)} />}
    </>
  )
}
