import { useCallback, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { api, events } from './lib/api'
import type { ClipboardItem, Secret } from './lib/types'
import { autoName } from './lib/naming'
import {
  IconCheck,
  IconClipboard,
  IconImage,
  IconLayers,
  IconLoader,
  IconPin,
  IconSearch,
  IconSettings,
  IconTrash
} from './components/Icons'
import './styles/global.css'
import './styles/panel.css'

const KINDS = [
  { id: 'all', label: '全部' },
  { id: 'link', label: '链接' },
  { id: 'text', label: '文本' },
  { id: 'image', label: '图片' },
  { id: 'files', label: '文件' },
  { id: 'favorite', label: '收藏' }
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
    case 'link':
      return '链接'
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
  /** 收藏（密钥/密码）：另一份数据，不在剪贴板历史里 */
  const [secrets, setSecrets] = useState<Secret[]>([])
  /** 刚存为收藏的条目，按钮短暂变成勾 */
  const [starred, setStarred] = useState<string | null>(null)
  /** 面板里的收藏就地改名（右键进入） */
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null)
  const cancelRename = useRef(false)
  const [keyword, setKeyword] = useState('')
  const [kind, setKind] = useState('all')
  const [index, setIndex] = useState(0)
  const [targetApp, setTargetApp] = useState('')
  const [loading, setLoading] = useState(true)
  const inputRef = useRef<HTMLInputElement>(null)
  const listRef = useRef<HTMLDivElement>(null)
  /** 取数请求序号：只认最新一次的结果 */
  const reqSeq = useRef(0)

  const load = useCallback(async (kw: string, k: string, keepId?: string) => {
    // 挂载时有两个 effect 会同时取数（一个初始化、一个按关键字防抖），
    // 加上每次呼出的 reset，很容易出现「先发的请求后回来」。
    // 那样旧列表会盖掉新列表 —— 面板上看到的条目和刚触发的同步结果不一致，
    // 用户就会「回车粘出来是上一次的内容」。用序号丢弃过期响应。
    const mine = ++reqSeq.current

    // 收藏是另一份数据：它不在剪贴板历史里，也不该被「清空」按钮清掉
    if (k === 'favorite') {
      try {
        const list = await api.secretsList()
        if (mine !== reqSeq.current) return
        const kwLower = kw.trim().toLowerCase()
        setSecrets(
          kwLower
            ? list.filter(
                (x) =>
                  x.name.toLowerCase().includes(kwLower) ||
                  x.note.toLowerCase().includes(kwLower)
              )
            : list
        )
        setIndex(0)
        void api.clipboardPanelReady(null)
      } catch {
        if (mine === reqSeq.current) setSecrets([])
      } finally {
        if (mine === reqSeq.current) setLoading(false)
      }
      return
    }

    try {
      const list = await api.clipboardList({
        keyword: kw || undefined,
        kind: k === 'all' ? undefined : k,
        limit: 80
      })
      if (mine !== reqSeq.current) return
      setItems(list)
      // 管理操作（置顶/删除）后要停在原来那条上，别把光标甩回第一行；
      // 换搜索词/筛选时才回到顶部。
      if (keepId) {
        setIndex((prev) => {
          const at = list.findIndex((i) => i.id === keepId)
          return at >= 0 ? at : Math.min(prev, Math.max(0, list.length - 1))
        })
      } else {
        setIndex(0)
      }
      // 列表到位了才报就绪。后端在"预热"期间接住的回车会在这一步补做，
      // 所以必须等 setItems 之后再报，否则补做的那次粘贴会粘到旧的首项。
      void api.clipboardPanelReady(list[0]?.id ?? null)
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

  const isFav = kind === 'favorite'
  /** 当前视图的条目：收藏走另一份数组，导航/计数都用它 */
  const rows: Array<ClipboardItem | Secret> = isFav ? secrets : items

  const pasteSecret = (secret: Secret | undefined, copyOnly: boolean) => {
    if (!secret) return
    // 后端负责写剪贴板 + 收面板 + 还原焦点，失败时内容也已经在剪贴板里
    void api.secretPaste(secret.id, copyOnly).catch(() => {
      /* 同 paste：失败不打断，用户可手动 Ctrl+V */
    })
  }

  const paste = (item: ClipboardItem | undefined, copyOnly: boolean) => {
    if (!item) return
    // 后端会先收起面板再还原焦点，所以这里只管发请求
    void api.clipboardPanelPaste(item.id, copyOnly).catch(() => {
      /* 粘贴失败时后端已把内容写进剪贴板，用户仍可手动 Ctrl+V */
    })
  }

  /* ---------------- 管理操作 ---------------- */

  /** 把这条内容存进收藏。名字自动取，之后在剪贴板页的「收藏」里能改。 */
  const saveToSecrets = async (item: ClipboardItem | undefined) => {
    const text = item?.text?.trim()
    if (!text) return
    try {
      await api.secretsSave({ name: autoName(text), value: text })
      setStarred(item!.id)
      window.setTimeout(() => setStarred((v) => (v === item!.id ? null : v)), 1500)
    } catch {
      /* 收藏失败不该打断粘贴流程，静默即可 */
    }
  }


  /** 提交收藏改名。空名字/没改动都当取消。 */
  const commitRename = async () => {
    const cur = renaming
    if (!cur) return
    setRenaming(null)
    if (cancelRename.current) return
    const name = cur.name.trim()
    const old = secrets.find((x) => x.id === cur.id)
    if (!old || old.name === name || !name) return
    try {
      await api.secretsRename(cur.id, name)
      setSecrets((prev) => prev.map((x) => (x.id === cur.id ? { ...x, name } : x)))
    } catch {
      /* 改失败就维持原样，面板里没地方放错误提示 */
    }
  }

  /** 置顶/取消置顶。列表里置顶项本来就排在最前，刷新后光标跟回这一条。 */
  const togglePin = async (item: ClipboardItem | undefined) => {
    if (!item) return
    try {
      await api.clipboardTogglePin(item.id)
      await load(keyword, kind, item.id)
    } catch {
      /* 置顶失败不值得打断用户，列表刷新后自然还原 */
    }
  }

  const removeItem = async (item: ClipboardItem | undefined) => {
    if (!item) return
    try {
      await api.clipboardRemove(item.id)
      await load(keyword, kind)
    } catch {
      /* 同上 */
    }
  }

  /** 图片：钉在屏幕上（置顶显示），面板顺手收起。 */
  const pinToScreen = async (item: ClipboardItem) => {
    try {
      await api.clipboardPin(item.id)
      await api.clipboardPanelHide()
    } catch {
      /* 钉图失败就在面板里待着，用户能看到列表没变 */
    }
  }

  /** 清空历史。默认保留置顶项；按住 Shift 才连置顶一起清。 */
  const [armed, setArmed] = useState(false)
  const clearHistory = async (includePinned: boolean) => {
    try {
      await api.clipboardClear(!includePinned)
      setArmed(false)
      await load('', 'all')
    } catch {
      setArmed(false)
    }
  }

  /** 打开主窗口的设置页（顺手收起面板） */
  const openSettings = async () => {
    await api.clipboardPanelHide()
    await api.showMain('settings')
  }

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Escape') {
      e.preventDefault()
      if (armed) {
        setArmed(false)
        return
      }
      void api.clipboardPanelHide()
      return
    }
    if (e.key === 'ArrowDown') {
      e.preventDefault()
      setIndex((i) => Math.min(i + 1, rows.length - 1))
      return
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault()
      setIndex((i) => Math.max(i - 1, 0))
      return
    }
    // 收藏项没有置顶/删除语义，这两个键留给输入框
    if (isFav && (e.key === 'Delete' || ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'p'))) {
      return
    }
    // Ctrl+P 置顶：不占用普通字符，输入框里也不会误触
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'p') {
      e.preventDefault()
      void togglePin(items[index])
      return
    }
    // 搜索框空着的时候 Delete 才是"删这条"，否则留给输入框删字符
    if (e.key === 'Delete' && !keyword) {
      e.preventDefault()
      void removeItem(items[index])
      return
    }
    if (e.key === 'Enter') {
      e.preventDefault()
      // Shift+Enter = 只复制不粘贴，两种视图一致
      if (isFav) pasteSecret(secrets[index], e.shiftKey)
      else paste(items[index], e.shiftKey)
    }
  }

  return (
    <div
      className="panel"
      onKeyDown={onKeyDown}
      // 面板是应用界面，不是网页：右键弹出"返回 / 刷新 / 另存为 / 打印"
      // 在桌面应用里非常出戏。收藏行的右键改名在行内自己处理。
      // 输入框不拦 —— 那里用户要的是系统自带的复制/粘贴。
      onContextMenu={(e) => {
        if (!(e.target as HTMLElement).closest('input, textarea')) e.preventDefault()
      }}
    >
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
        <span className="p-count">{rows.length}</span>
        <button
          className={`p-tool ${armed ? 'danger' : ''}`}
          title={
            armed
              ? '再点一次确认清空（Shift 点=连置顶项一起清）'
              : '清空历史（保留置顶项）'
          }
          onClick={(e) => {
            if (!armed) {
              setArmed(true)
              // 3 秒没确认就撤销，避免用户误点后又点别的按钮时被清空
              window.setTimeout(() => setArmed(false), 3000)
              return
            }
            void clearHistory(e.shiftKey)
          }}
        >
          {armed ? <span className="p-tool-text">确认清空</span> : <IconTrash size={12} />}
        </button>
        <button className="p-tool" title="打开设置" onClick={() => void openSettings()}>
          <IconSettings size={12} />
        </button>
      </div>

      <div className="p-list" ref={listRef}>
        {loading && rows.length === 0 && (
          <div className="p-empty">
            <IconLoader size={16} />
            <span>读取中…</span>
          </div>
        )}

        {!loading && rows.length === 0 && (
          <div className="p-empty">
            <IconClipboard size={26} />
            <span>
              {isFav
                ? keyword
                  ? '没有匹配的收藏'
                  : '还没有收藏，去剪贴板页添加'
                : keyword
                  ? '没有匹配的记录'
                  : '还没有记录'}
            </span>
          </div>
        )}

        {!isFav && items.map((item, i) => (
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

            {/* 行内动作：悬停/选中才显示，不干扰"看列表"这件事 */}
            <div className="p-actions">
              {item.kind === 'image' && (
                <button
                  className="p-act"
                  title="钉在屏幕上"
                  onClick={(e) => {
                    e.stopPropagation()
                    void pinToScreen(item)
                  }}
                >
                  <IconLayers size={12} />
                </button>
              )}
              {item.text?.trim() && (
                <button
                  className="p-act"
                  title="存为收藏"
                  onClick={(e) => {
                    e.stopPropagation()
                    void saveToSecrets(item)
                  }}
                >
                  {starred === item.id ? <IconCheck size={12} /> : <span>🔑</span>}
                </button>
              )}
              <button
                className={`p-act ${item.pinned ? 'on' : ''}`}
                title={item.pinned ? '取消置顶（Ctrl+P）' : '置顶（Ctrl+P）'}
                onClick={(e) => {
                  e.stopPropagation()
                  void togglePin(item)
                }}
              >
                <IconPin size={12} />
              </button>
              <button
                className="p-act danger"
                title="删除这一条（Delete）"
                onClick={(e) => {
                  e.stopPropagation()
                  void removeItem(item)
                }}
              >
                <IconTrash size={12} />
              </button>
            </div>
          </div>
        ))}

        {/* 收藏：只显示名称和备注，内容打码 —— 面板是公共视线里会打开的东西 */}
        {isFav &&
          secrets.map((sec, i) => (
            <div
              key={sec.id}
              data-idx={i}
              className={`p-row ${i === index ? 'active' : ''}`}
              onMouseEnter={() => setIndex(i)}
              onClick={(e) => pasteSecret(sec, e.shiftKey)}
              onAuxClick={(e) => {
                if (e.button === 1) {
                  e.preventDefault()
                  pasteSecret(sec, true)
                }
              }}
              // 面板里收藏只有"粘出去"一件事，可改的只有名字 ——
              // 右键直接进改名态，比弹一个只有一项的菜单实在
              onContextMenu={(e) => {
                e.preventDefault()
                cancelRename.current = false
                setRenaming({ id: sec.id, name: sec.name })
              }}
            >
              <span className="p-type p-secret-key">🔑</span>
              <div className="p-body">
                {renaming?.id === sec.id ? (
                  <input
                    className="p-rename"
                    value={renaming.name}
                    autoFocus
                    spellCheck={false}
                    onFocus={(e) => e.currentTarget.select()}
                    onChange={(e) => setRenaming({ id: sec.id, name: e.target.value })}
                    onClick={(e) => e.stopPropagation()}
                    onKeyDown={(e) => {
                      // 必须拦住：根节点上 Enter 是"粘贴选中项"
                      e.stopPropagation()
                      if (e.key === 'Enter') {
                        e.preventDefault()
                        void commitRename()
                      } else if (e.key === 'Escape') {
                        e.preventDefault()
                        cancelRename.current = true
                        setRenaming(null)
                      }
                    }}
                    onBlur={() => void commitRename()}
                  />
                ) : (
                  <div className="p-text" title="右键改名">
                    {sec.name}
                  </div>
                )}
                <div className="p-meta">
                  <span className="p-secret-dots">••••••••</span>
                  {sec.note && (
                    <>
                      <span className="dot">·</span>
                      <span>{sec.note}</span>
                    </>
                  )}
                </div>
              </div>
            </div>
          ))}
      </div>

      <div className="p-foot" title="按住 Shift 再按 Enter（或中键点击）= 只复制不粘贴">
        <span className="p-hint" title="Esc 关闭">
          <kbd>↑</kbd>
          <kbd>↓</kbd> <kbd>Enter</kbd> 粘贴 · <kbd>Ctrl</kbd>+<kbd>P</kbd> 置顶 ·{' '}
          <kbd>Del</kbd> 删除
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
