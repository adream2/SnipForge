import { useCallback, useEffect, useRef, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { api } from '../lib/api'
import type { AppSettings, ProviderMeta, UploadOutcome } from '../lib/types'
import { dataUrlBytes, extOf, prepareForUpload, type PreparedImage } from '../lib/compress'
import { IconCheck, IconClipboard, IconCopy, IconImage, IconLoader, IconUpload } from '../components/Icons'
import { useToast } from '../components/Toast'

/** 本次会话的上传记录（不落库，够用就行） */
interface Sent extends UploadOutcome {
  name: string
  bytes: number
  saved?: number
  at: number
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
  const [sent, setSent] = useState<Sent[]>([])
  const [copied, setCopied] = useState<string | null>(null)
  const fileRef = useRef<HTMLInputElement>(null)

  const up = settings?.uploader

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

  const upload = async () => {
    if (!prepared || !provider) return
    setBusy(true)
    const started = Date.now()
    try {
      const ext = extOf(prepared.format)
      const base = (src?.name ?? 'image').replace(/\.[^.]+$/, '')
      const out = await api.uploadImage(prepared.dataUrl, provider, `${base}.${ext}`)
      setSent((prev) => [
        {
          ...out,
          name: `${base}.${ext}`,
          bytes: prepared.bytes,
          saved: prepared.changed ? 1 - prepared.bytes / prepared.srcBytes : undefined,
          at: started
        },
        ...prev
      ])
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

        {/* ---------------- 本次会话的上传记录 ---------------- */}
        {sent.length > 0 && (
          <div className="setting-card" style={{ marginTop: 14 }}>
            <header>刚才传的</header>
            {sent.map((s, i) => (
              <div
                key={`${s.at}-${i}`}
                style={{
                  display: 'flex',
                  alignItems: 'center',
                  gap: 10,
                  padding: '9px 16px',
                  borderTop: i === 0 ? 'none' : '1px solid var(--border)',
                  fontSize: 12
                }}
              >
                <span style={{ color: 'var(--text-3)', width: 92, flex: 'none' }}>{s.providerName}</span>
                <span
                  style={{
                    flex: 1,
                    minWidth: 0,
                    overflow: 'hidden',
                    textOverflow: 'ellipsis',
                    whiteSpace: 'nowrap',
                    color: 'var(--text-2)'
                  }}
                  title={s.result.url}
                >
                  {s.name} · {kb(s.bytes)}
                  {s.saved !== undefined && s.saved > 0.01 && (
                    <span style={{ color: 'var(--accent)', marginLeft: 6 }}>
                      省 {Math.round(s.saved * 100)}%
                    </span>
                  )}
                </span>
                <button
                  className="btn sm"
                  onClick={() => void copy(s.result.url, `u${i}`)}
                  title="复制链接"
                >
                  {copied === `u${i}` ? <IconCheck size={12} /> : <IconCopy size={12} />}
                </button>
                <button
                  className="btn sm"
                  onClick={() => void copy(`![${s.name}](${s.result.url})`, `m${i}`)}
                  title="复制 Markdown"
                >
                  {copied === `m${i}` ? <IconCheck size={12} /> : 'MD'}
                </button>
                <button className="btn sm" onClick={() => void api.openPath(s.result.url)} title="在浏览器打开">
                  ↗
                </button>
              </div>
            ))}
          </div>
        )}

        {providers.length === 0 && (
          <div className="empty" style={{ height: 120, marginTop: 14 }}>
            <span>还没有可用的图床，去「图床配置」配一个</span>
          </div>
        )}
      </div>
    </>
  )
}
