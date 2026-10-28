import { useCallback, useEffect, useRef, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { api } from '../lib/api'
import type { AppSettings, DirEntry, ProviderMeta, UploadOutcome, UploadRecord } from '../lib/types'
import { dataUrlBytes, extOf, prepareForUpload, type PreparedImage } from '../lib/compress'
import { IconCheck, IconClipboard, IconCopy, IconImage, IconLoader, IconUpload } from '../components/Icons'
import { useToast } from '../components/Toast'

/** 只有 http(s) 的图能直接当缩略图；本地目录图床是 file://，webview 加载不了 */
const isRemote = (u: string) => /^https?:\/\//i.test(u)

/** 今天只显示时分，跨天带日期 —— 列表里最常看的是"刚传的那张" */
function fmtTime(ms: number): string {
  if (!ms) return ''
  const d = new Date(ms)
  const now = new Date()
  const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
  if (d.toDateString() === now.toDateString()) return hm
  const md = `${d.getMonth() + 1}-${String(d.getDate()).padStart(2, '0')}`
  if (d.getFullYear() === now.getFullYear()) return `${md} ${hm}`
  return `${d.getFullYear()}-${md}`
}

const FORMAT_OPTS = [
  { v: 'keep', label: '保持原样' },
  { v: 'webp', label: 'WebP' },
  { v: 'jpeg', label: 'JPEG' },
  { v: 'png', label: 'PNG' }
]
const EDGES = [
  { v: 0, label: '不限' },
  { v: 2560, label: '2560' },
  { v: 1920, label: '1920' },
  { v: 1280, label: '1280' }
]

/** 上一级目录；已在根则返回 null */
function parentOf(path: string): string | null {
  const clean = path.replace(/\/+$/, '')
  if (!clean) return null
  const i = clean.lastIndexOf('/')
  return i < 0 ? '' : clean.slice(0, i)
}

function kb(n: number): string {
  if (n >= 1024 * 1024) return `${(n / 1048576).toFixed(2)} MB`
  return `${(n / 1024).toFixed(0)} KB`
}

/**
 * 图床：只管"传图"。配置在「图床配置」页。
 *
 * 上传前可以在这一页直接压缩 / 转格式 —— 用 canvas 做，能出有损 WebP，
 * 而不是后端那种无损 WebP（体积优势基本没有）。
 */
export default function UploadPage({
  settings,
  onChange
}: {
  settings: AppSettings | null
  onChange: (patch: unknown) => Promise<AppSettings>
}) {
  const toast = useToast()
  const [providers, setProviders] = useState<ProviderMeta[]>([])
  const [provider, setProvider] = useState('')
  const [src, setSrc] = useState<{ dataUrl: string; name: string } | null>(null)
  const [prepared, setPrepared] = useState<PreparedImage | null>(null)
  const [busy, setBusy] = useState(false)
  const [preparing, setPreparing] = useState(false)
  const [dragging, setDragging] = useState(false)
  /** 上传记录：后端落盘（uploads.json），重启后还在 */
  const [records, setRecords] = useState<UploadRecord[]>([])
  /** 清空的两段式确认 */
  const [confirmClear, setConfirmClear] = useState(false)
  const [copied, setCopied] = useState<string | null>(null)
  /** 单次上传的目录覆盖（空 = 用配置里的路径）；支持 {yyyy}/{MM} 占位符 */
  const [dirInput, setDirInput] = useState('')
  /** 目录选择器 */
  const [browse, setBrowse] = useState<{
    open: boolean
    path: string
    dirs: DirEntry[]
    loading: boolean
    newName: string
  }>({ open: false, path: '', dirs: [], loading: false, newName: '' })
  const fileRef = useRef<HTMLInputElement>(null)

  const up = settings?.uploader

  const current = providers.find((p) => p.id === provider)

  /** 配置里那个"默认路径"字段：各家 key 不一样 */
  const cfgPath = (() => {
    const c = (settings?.uploader.providers?.[provider] ?? {}) as Record<string, unknown>
    for (const k of ['path', 'pathPrefix', 'dir']) {
      const v = c[k]
      if (typeof v === 'string' && v.trim()) return v.trim()
    }
    return ''
  })()

  const loadHistory = useCallback(async () => {
    try {
      setRecords(await api.uploadHistoryList(undefined, 200))
    } catch (e) {
      // 历史读不出来不该挡住上传，记一笔就好
      console.warn('读取上传记录失败', e)
    }
  }, [])

  useEffect(() => {
    void loadHistory()
  }, [loadHistory])

  useEffect(() => {
    void api.uploadProviders().then((list) => {
      setProviders(list)
      if (!provider && list.length) setProvider(settings?.uploader.activeProvider || list[0].id)
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    if (settings?.uploader.activeProvider && !provider) {
      setProvider(settings.uploader.activeProvider)
    }
  }, [settings, provider])

  // 图 / 压缩参数一变就重算，用户随时能看到"传上去多大"
  useEffect(() => {
    if (!src || !up) {
      setPrepared(null)
      return
    }
    let alive = true
    setPreparing(true)
    void prepareForUpload(src.dataUrl, {
      enabled: up.compress,
      format: up.compressFormat,
      quality: up.compressQuality,
      maxEdge: up.compressMaxEdge
    })
      .then((p) => {
        if (alive) setPrepared(p)
      })
      .catch((e) => {
        if (alive) {
          setPrepared(null)
          toast.push(String(e), 'error')
        }
      })
      .finally(() => {
        if (alive) setPreparing(false)
      })
    return () => {
      alive = false
    }
  }, [src, up?.compress, up?.compressFormat, up?.compressQuality, up?.compressMaxEdge, toast, up])

  const takeFile = useCallback(async (path: string) => {
    try {
      const f = await api.imageLoadFile(path)
      setSrc({ dataUrl: f.dataUrl, name: path.split(/[\\/]/).pop() ?? 'image' })
    } catch (e) {
      toast.push(`读取失败：${e}`, 'error')
    }
  }, [toast])

  const pickFile = async () => {
    const picked = await open({
      multiple: false,
      filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'gif'] }]
    })
    if (typeof picked === 'string') await takeFile(picked)
  }

  const fromClipboard = async () => {
    try {
      const dataUrl = await api.clipboardImageData()
      setSrc({ dataUrl, name: 'clipboard.png' })
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const handleDrop = async (e: React.DragEvent) => {
    e.preventDefault()
    setDragging(false)
    const file = e.dataTransfer.files?.[0]
    if (!file) return
    const dataUrl = await new Promise<string>((resolve, reject) => {
      const reader = new FileReader()
      reader.onload = () => resolve(String(reader.result))
      reader.onerror = reject
      reader.readAsDataURL(file)
    })
    setSrc({ dataUrl, name: file.name })
  }

  const loadDirs = useCallback(
    async (path: string) => {
      if (!provider) return
      setBrowse((b) => ({ ...b, loading: true }))
      try {
        const dirs = await api.uploadListDirs(provider, path)
        setBrowse((b) => ({ ...b, path, dirs, loading: false }))
      } catch (e) {
        toast.push(String(e), 'error')
        setBrowse((b) => ({ ...b, dirs: [], loading: false }))
      }
    },
    [provider, toast]
  )

  const openBrowse = () => {
    // 浏览用的 path 永远是"相对该图床根"的：
    // 本地图床配置的是绝对磁盘路径，直接拿来当相对路径会把面包屑拆成 E:/WorkBuddy/…
    const start =
      dirInput.trim() || (provider === 'local-folder' ? '' : cfgPath)
    setBrowse({ open: true, path: start, dirs: [], loading: true, newName: '' })
    void loadDirs(start)
  }

  /** 用新建的目录（还没建也能用 —— 各图床上传时都会自动创建） */
  const useNewDir = () => {
    const v = browse.newName.trim()
    setDirInput(v ? [browse.path, v].filter(Boolean).join('/') : browse.path)
    setBrowse((b) => ({ ...b, open: false, newName: '' }))
  }

  const upload = async () => {
    if (!prepared || !provider) return
    setBusy(true)
    try {
      const ext = extOf(prepared.format)
      const base = (src?.name ?? 'image').replace(/\.[^.]+$/, '')
      const out = await api.uploadImage(
        prepared.dataUrl,
        provider,
        `${base}.${ext}`,
        dirInput.trim() || undefined,
        prepared.srcBytes
      )
      // 记录由后端落盘，这里只需要重新拉一次列表
      await loadHistory()
      if (settings?.uploader.copyAsMarkdown) {
        await api.clipboardCopyText(`![${base}](${out.result.url})`)
        toast.push('已上传并复制 Markdown', 'success')
      } else {
        await api.clipboardCopyText(out.result.url)
        toast.push('已上传并复制链接', 'success')
      }
    } catch (e) {
      toast.push(String(e), 'error')
    } finally {
      setBusy(false)
    }
  }

  const copy = async (text: string, key: string) => {
    await api.clipboardCopyText(text)
    setCopied(key)
    window.setTimeout(() => setCopied(null), 1200)
  }

  const setCompress = (patch: Record<string, unknown>) =>
    void onChange({ uploader: { ...patch } })

  const saved = prepared && prepared.changed ? 1 - prepared.bytes / prepared.srcBytes : 0

  return (
    <>
      <div className="page-head">
        <h1>图床</h1>
        <div className="spacer" />
        {prepared && (
          <button className="btn sm" onClick={() => setSrc(null)}>
            换一张
          </button>
        )}
        <button
          className="btn sm"
          onClick={() => void api.showMain('upload-config')}
          title="配置图床账号"
        >
          图床配置
        </button>
      </div>

      <div className="page-body">
        <div style={{ display: 'grid', gridTemplateColumns: '1fr 300px', gap: 14, alignItems: 'start' }}>
          {/* ---------------- 左：图片 ---------------- */}
          {!src ? (
            <div
              className={`tool-drop ${dragging ? 'dragging' : ''}`}
              onDragOver={(e) => {
                e.preventDefault()
                setDragging(true)
              }}
              onDragLeave={() => setDragging(false)}
              onDrop={handleDrop}
              onClick={() => void pickFile()}
              style={{ cursor: 'pointer', minHeight: 300, marginTop: 0 }}
            >
              <IconImage size={34} />
              <div style={{ marginTop: 12, fontSize: 13.5, fontWeight: 500, color: 'var(--text-2)' }}>
                拖入图片，或点击选择
              </div>
              <div style={{ marginTop: 4, fontSize: 12 }}>
                也可以
                <button
                  className="btn sm"
                  style={{ margin: '0 6px' }}
                  onClick={(e) => {
                    e.stopPropagation()
                    void fromClipboard()
                  }}
                >
                  <IconClipboard size={12} /> 取剪贴板图片
                </button>
              </div>
            </div>
          ) : (
            <div
              style={{
                background: 'var(--surface)',
                border: '1px solid var(--border)',
                borderRadius: 'var(--radius)',
                padding: 14,
                display: 'grid',
                placeItems: 'center',
                minHeight: 320
              }}
            >
              <img
                src={(prepared ?? src).dataUrl ?? src.dataUrl}
                alt=""
                style={{ maxWidth: '100%', maxHeight: 440, borderRadius: 6, display: 'block' }}
              />
            </div>
          )}

          {/* ---------------- 右：参数 + 上传 ---------------- */}
          <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
            <div className="setting-card">
              <header>上传到</header>
              <div style={{ padding: '10px 16px 14px' }}>
                <select
                  className="input"
                  value={provider}
                  onChange={(e) => setProvider(e.target.value)}
                >
                  {providers.map((p) => (
                    <option key={p.id} value={p.id}>
                      {p.name}
                      {p.id === settings?.uploader.activeProvider ? '（默认）' : ''}
                    </option>
                  ))}
                </select>
              </div>
            </div>

            {current?.supportsDir && (
              <div className="setting-card">
                <header>上传路径</header>
                <div
                  style={{
                    padding: '10px 16px 16px',
                    display: 'flex',
                    flexDirection: 'column',
                    gap: 10
                  }}
                >
                  <div style={{ display: 'flex', gap: 6 }}>
                    <input
                      className="input"
                      style={{ flex: 1, minWidth: 0 }}
                      placeholder={cfgPath || '（根目录）'}
                      value={dirInput}
                      onChange={(e) => setDirInput(e.target.value)}
                    />
                    {current?.canBrowse && (
                      <button className="btn sm" onClick={openBrowse} title="选已有目录或新建">
                        浏览
                      </button>
                    )}
                  </div>
                  <div style={{ fontSize: 11, color: 'var(--text-3)', lineHeight: 1.6 }}>
                    留空 = 用配置里的 {cfgPath ? <code>{cfgPath}</code> : '根目录'}；支持{' '}
                    <code>{'{yyyy}'}</code> <code>{'{MM}'}</code> <code>{'{dd}'}</code> 占位符，
                    比如 <code>blog/{'{yyyy}/{MM}'}</code>
                  </div>
                </div>
              </div>
            )}

            {/* 右栏只有 300px，用上下堆叠的 field；setting-row 是给宽页面的左右布局 */}
            <div className="setting-card">
              <header>上传前压缩</header>
              <div
                style={{
                  padding: '10px 16px 16px',
                  display: 'flex',
                  flexDirection: 'column',
                  gap: 13
                }}
              >
                <div style={{ display: 'flex', alignItems: 'center', gap: 10 }}>
                  <div style={{ flex: 1 }}>
                    <strong style={{ fontSize: 12.5 }}>启用</strong>
                    <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 2 }}>
                      自动转格式 + 限边长
                    </div>
                  </div>
                  <button
                    className={`switch ${up?.compress ? 'on' : ''}`}
                    onClick={() => setCompress({ compress: !up?.compress })}
                  />
                </div>

                {up?.compress && (
                  <>
                    <div className="field">
                      <label>格式</label>
                      <div className="seg">
                        {FORMAT_OPTS.map((f) => (
                          <button
                            key={f.v}
                            className={up.compressFormat === f.v ? 'active' : ''}
                            onClick={() => setCompress({ compressFormat: f.v })}
                          >
                            {f.label}
                          </button>
                        ))}
                      </div>
                    </div>

                    {(up.compressFormat === 'webp' || up.compressFormat === 'jpeg') && (
                      <div className="field">
                        <label style={{ display: 'flex', justifyContent: 'space-between' }}>
                          <span>质量</span>
                          <span style={{ color: 'var(--text-3)' }}>{up.compressQuality}</span>
                        </label>
                        <input
                          type="range"
                          className="slider"
                          min={40}
                          max={100}
                          value={up.compressQuality}
                          onChange={(e) => setCompress({ compressQuality: Number(e.target.value) })}
                          style={{ background: '#e0e0e6' }}
                        />
                      </div>
                    )}

                    <div className="field">
                      <label>最长边（像素）</label>
                      <div className="seg">
                        {EDGES.map((e) => (
                          <button
                            key={e.v}
                            className={up.compressMaxEdge === e.v ? 'active' : ''}
                            onClick={() => setCompress({ compressMaxEdge: e.v })}
                          >
                            {e.label}
                          </button>
                        ))}
                      </div>
                    </div>
                  </>
                )}
              </div>
            </div>

            <div className="setting-card">
              <header>体积</header>
              <div style={{ padding: '10px 16px 14px', fontSize: 12.5, color: 'var(--text-2)', lineHeight: 1.9 }}>
                {!prepared ? (
                  <span style={{ color: 'var(--text-3)' }}>
                    {preparing ? '计算中…' : '先选一张图'}
                  </span>
                ) : (
                  <>
                    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                      <span>原图</span>
                      <span>
                        {kb(prepared.srcBytes)} · {prepared.srcWidth}×{prepared.srcHeight}
                      </span>
                    </div>
                    <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                      <span>上传</span>
                      <span style={{ color: 'var(--text)' }}>
                        {kb(prepared.bytes)} · {prepared.width}×{prepared.height} ·{' '}
                        {extOf(prepared.format).toUpperCase()}
                      </span>
                    </div>
                    {saved > 0.01 && <span className="tag success">小了 {Math.round(saved * 100)}%</span>}
                    {!prepared.changed && up?.compress && (
                      <div style={{ fontSize: 11, color: 'var(--text-3)', marginTop: 4 }}>
                        这个配置下不会再小，按原图上传
                      </div>
                    )}
                  </>
                )}
              </div>
            </div>

            <button
              className="btn primary"
              style={{ height: 42 }}
              disabled={!prepared || busy || !provider}
              onClick={() => void upload()}
            >
              {busy ? <IconLoader /> : <IconUpload size={14} />} 上传
            </button>
          </div>
        </div>

        {/* ---------------- 上传记录（落盘，重启后还在） ---------------- */}
        <div className="setting-card" style={{ marginTop: 14 }}>
          <header style={{ display: 'flex', alignItems: 'center', gap: 8, paddingBottom: 8 }}>
            <span>上传记录</span>
            <span style={{ fontWeight: 400, textTransform: 'none' }}>{records.length}</span>
            <div style={{ flex: 1 }} />
            {records.length > 0 && (
              <button
                className={`btn sm danger ${confirmClear ? 'armed' : ''}`}
                onClick={async () => {
                  // 两段式：先变红，3 秒内不再点就自动撤销
                  if (!confirmClear) {
                    setConfirmClear(true)
                    window.setTimeout(() => setConfirmClear(false), 3000)
                    return
                  }
                  try {
                    await api.uploadHistoryClear()
                    setConfirmClear(false)
                    await loadHistory()
                    toast.push('记录已清空（远端图片不受影响）', 'success')
                  } catch (e) {
                    toast.push(String(e), 'error')
                  }
                }}
              >
                {confirmClear ? '确认清空' : '清空'}
              </button>
            )}
          </header>

          {records.length === 0 ? (
            <div style={{ padding: '2px 16px 16px', fontSize: 11.5, color: 'var(--text-3)' }}>
              传过的图会记在这里，重启也还在。
            </div>
          ) : (
            records.map((r, i) => {
              const saved = r.origBytes > 0 && r.bytes < r.origBytes ? 1 - r.bytes / r.origBytes : 0
              return (
                <div className="up-row" key={r.id}>
                  <div className="up-thumb">
                    <span className="up-thumb-fallback">{isRemote(r.url) ? '🖼' : '🗂'}</span>
                    {isRemote(r.url) && (
                      <img
                        src={r.url}
                        alt=""
                        loading="lazy"
                        onError={(e) => {
                          // 图床删了 / 防盗链 / 离线：露出底层占位，别显示破图标
                          e.currentTarget.style.display = 'none'
                        }}
                      />
                    )}
                  </div>

                  <div className="up-info">
                    <div className="up-name" title={r.url}>
                      {r.name}
                    </div>
                    <div className="up-meta">
                      <span>{r.providerName}</span>
                      <span className="dot">·</span>
                      <span>{kb(r.bytes)}</span>
                      {saved > 0.01 && (
                        <>
                          <span className="dot">·</span>
                          <span style={{ color: 'var(--accent)' }}>
                            省 {Math.round(saved * 100)}%
                          </span>
                        </>
                      )}
                      <span className="dot">·</span>
                      <span>{fmtTime(r.createdAt)}</span>
                    </div>
                  </div>

                  <div className="up-acts">
                    <button
                      className="btn sm"
                      title="复制链接"
                      onClick={() => void copy(r.url, `u${i}`)}
                    >
                      {copied === `u${i}` ? <IconCheck size={12} /> : <IconCopy size={12} />}
                    </button>
                    <button
                      className="btn sm"
                      title="复制 Markdown"
                      onClick={() => void copy(`![${r.name}](${r.url})`, `m${i}`)}
                    >
                      {copied === `m${i}` ? <IconCheck size={12} /> : 'MD'}
                    </button>
                    <button
                      className="btn sm"
                      title={r.localPath ? '在资源管理器中显示' : '在浏览器打开'}
                      onClick={() =>
                        void (r.localPath ? api.openImageFolder(r.localPath) : api.openPath(r.url))
                      }
                    >
                      ↗
                    </button>
                    <button
                      className="btn sm danger"
                      title="从记录里删掉（不会删除远端图片）"
                      onClick={async () => {
                        try {
                          await api.uploadHistoryRemove(r.id)
                          await loadHistory()
                        } catch (e) {
                          toast.push(String(e), 'error')
                        }
                      }}
                    >
                      ✕
                    </button>
                  </div>
                </div>
              )
            })
          )}
        </div>

        {providers.length === 0 && (
          <div className="empty" style={{ height: 120, marginTop: 14 }}>
            <span>还没有可用的图床，去「图床配置」配一个</span>
          </div>
        )}

        {/* 目录选择器：列已有目录，也能当场输入新目录 */}
        {browse.open && (
          <>
            <div className="ctx-shade" onClick={() => setBrowse((b) => ({ ...b, open: false }))} />
            <div className="dir-picker">
              <div className="dir-head">
                <strong>选择上传目录</strong>
                <span style={{ flex: 1 }} />
                <button className="btn sm" onClick={() => setBrowse((b) => ({ ...b, open: false }))}>
                  关闭
                </button>
              </div>

              <div className="dir-path">
                <button className="dir-crumb" onClick={() => void loadDirs('')} title="回到根目录">
                  {current?.name ?? provider}
                </button>
                {browse.path
                  .split('/')
                  .filter(Boolean)
                  .map((seg, i, arr) => {
                    const segPath = arr.slice(0, i + 1).join('/')
                    return (
                      <span key={segPath} style={{ display: 'contents' }}>
                        <span className="dir-sep">/</span>
                        <button className="dir-crumb" onClick={() => void loadDirs(segPath)}>
                          {seg}
                        </button>
                      </span>
                    )
                  })}
              </div>

              <div className="dir-body">
                {parentOf(browse.path) !== null && (
                  <button
                    className="dir-item"
                    onClick={() => void loadDirs(parentOf(browse.path) ?? '')}
                  >
                    <span className="dir-icon">↩</span> 上一级
                  </button>
                )}
                {browse.loading && <div className="dir-empty">读取中…</div>}
                {!browse.loading && browse.dirs.length === 0 && (
                  <div className="dir-empty">这个目录下还没有子目录 —— 可以在下面新建一个</div>
                )}
                {browse.dirs.map((d) => (
                  <button key={d.path} className="dir-item" onClick={() => void loadDirs(d.path)}>
                    <span className="dir-icon">📁</span>
                    <span
                      style={{
                        flex: 1,
                        textAlign: 'left',
                        overflow: 'hidden',
                        textOverflow: 'ellipsis'
                      }}
                    >
                      {d.name}
                    </span>
                  </button>
                ))}
              </div>

              <div className="dir-new">
                <input
                  className="input"
                  placeholder="新建子目录名，如 2026 或 blog"
                  value={browse.newName}
                  onChange={(e) => setBrowse((b) => ({ ...b, newName: e.target.value }))}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter') useNewDir()
                  }}
                />
                <button className="btn sm" onClick={useNewDir}>
                  用这个
                </button>
              </div>

              <div className="dir-foot">
                <span style={{ fontSize: 11, color: 'var(--text-3)' }}>
                  目录不存在也没关系，上传时自动创建
                </span>
                <span style={{ flex: 1 }} />
                <button
                  className="btn primary sm"
                  onClick={() => {
                    setDirInput(browse.path)
                    setBrowse((b) => ({ ...b, open: false }))
                  }}
                >
                  就用{browse.path ? ` ${browse.path}` : '根目录'}
                </button>
              </div>
            </div>
          </>
        )}
      </div>
    </>
  )
}
