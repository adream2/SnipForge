import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { api } from '../../lib/api'
import type { AppSettings, DirEntry, ProviderMeta, RemoteImage, UploadRecord } from '../../lib/types'
import { IconCheck, IconCopy, IconImage, IconLoader } from '../../components/Icons'
import { useToast } from '../../components/Toast'

/**
 * 图床「列表」：浏览图床里**已有的**图片。
 *
 * 之前这个位置放的是"本工具传过的记录"，方向错了 —— 用户要的是
 * "我上次传的那张图在哪、链接是啥"，而不只是本次会话的流水。
 * 所以这里调 provider 的 list_images 直接列远端（GitHub 走 contents API、
 * 本地目录读文件系统）。不支持浏览的图床，退回显示本工具传过的记录。
 */

const isRemote = (u: string) => /^https?:\/\//i.test(u)

function kb(n: number): string {
  if (!n) return '—'
  if (n >= 1024 * 1024) return `${(n / 1048576).toFixed(2)} MB`
  return `${(n / 1024).toFixed(0)} KB`
}

/** file:///E:/a.png → E:\a.png */
function fileUrlToPath(u: string): string | null {
  if (!u.startsWith('file:///')) return null
  try {
    return decodeURIComponent(u.slice(8)).replace(/\//g, '\\')
  } catch {
    return null
  }
}

/**
 * 本地图片的缩略图。
 *
 * `file://` 在 webview 里当不了 img src，所以由后端读图缩小成 base64。
 * 用 IntersectionObserver 懒加载 —— 几百张图的目录不该一进来就全部解码。
 */
function LocalThumb({ path }: { path: string }) {
  const ref = useRef<HTMLDivElement>(null)
  const [src, setSrc] = useState('')

  useEffect(() => {
    const el = ref.current
    if (!el) return
    const io = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) {
        io.disconnect()
        void api
          .uploadThumb(path, 240)
          .then(setSrc)
          .catch(() => {})
      }
    })
    io.observe(el)
    return () => io.disconnect()
  }, [path])

  return (
    <div ref={ref} style={{ position: 'absolute', inset: 0 }}>
      {src && <img src={src} alt="" />}
    </div>
  )
}

function fmtTime(ms?: number): string {
  if (!ms) return ''
  const d = new Date(ms)
  const now = new Date()
  const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
  if (d.toDateString() === now.toDateString()) return hm
  const md = `${d.getMonth() + 1}-${String(d.getDate()).padStart(2, '0')}`
  return d.getFullYear() === now.getFullYear() ? `${md} ${hm}` : `${d.getFullYear()}-${md}`
}

export default function ListTab({ settings }: { settings: AppSettings | null }) {
  const toast = useToast()
  const [providers, setProviders] = useState<ProviderMeta[]>([])
  const [provider, setProvider] = useState('')
  /** 相对图床根的目录；空 = 根 */
  const [dir, setDir] = useState('')
  const [dirs, setDirs] = useState<DirEntry[]>([])
  const [images, setImages] = useState<RemoteImage[]>([])
  const [loading, setLoading] = useState(false)
  /** 图床不支持浏览时，退回显示本工具传过的记录 */
  const [unsupported, setUnsupported] = useState('')
  const [history, setHistory] = useState<UploadRecord[]>([])
  const [copied, setCopied] = useState<string | null>(null)

  const current = providers.find((p) => p.id === provider)

  useEffect(() => {
    void api.uploadProviders().then((list) => {
      setProviders(list)
      if (!provider && list.length) {
        setProvider(settings?.uploader.activeProvider || list[0].id)
      }
    })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    if (settings?.uploader.activeProvider && !provider) {
      setProvider(settings.uploader.activeProvider)
    }
  }, [settings, provider])

  const load = useCallback(
    async (pid: string, d: string) => {
      if (!pid) return
      setLoading(true)
      setUnsupported('')
      try {
        const [ds, imgs] = await Promise.all([
          api.uploadListDirs(pid, d).catch(() => [] as DirEntry[]),
          api.uploadListImages(pid, d, 300)
        ])
        setDirs(ds)
        setImages(imgs)
      } catch (e) {
        // 这个图床没有浏览能力（SM.MS / 自定义 HTTP / 声明式插件）：
        // 不把它当成错误弹出来，而是换一种"至少能看到自己传过什么"的呈现
        setDirs([])
        setImages([])
        setUnsupported(String(e))
        try {
          setHistory(await api.uploadHistoryList(undefined, 300))
        } catch {
          setHistory([])
        }
      } finally {
        setLoading(false)
      }
    },
    []
  )

  useEffect(() => {
    if (provider) void load(provider, dir)
  }, [provider, dir, load])

  const copy = async (text: string, key: string) => {
    try {
      await api.clipboardCopyText(text)
      setCopied(key)
      window.setTimeout(() => setCopied((c) => (c === key ? null : c)), 1200)
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  /** 面包屑：根 + 每一级 */
  const crumbs = useMemo(() => {
    const parts = dir.split('/').filter(Boolean)
    let acc = ''
    return parts.map((p) => {
      acc = acc ? `${acc}/${p}` : p
      return { name: p, path: acc }
    })
  }, [dir])

  /** 不支持浏览时，只显示这个图床传过的 */
  const fallback = history.filter((h) => h.providerId === provider)

  const copyMarkdown = (r: { name: string; url: string }, key: string) =>
    copy(`![${r.name}](${r.url})`, key)

  return (
    <div className="settings-wrap">
      {/* 图床 / 目录 / 刷新 */}
      <div className="list-bar">
        <select
          className="input"
          style={{ width: 150, height: 28, fontSize: 12 }}
          value={provider}
          onChange={(e) => {
            setProvider(e.target.value)
            setDir('')
          }}
        >
          {providers.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>

        <div className="crumb">
          <button className="crumb-item" onClick={() => setDir('')}>
            {current?.name ?? '根目录'}
          </button>
          {crumbs.map((c) => (
            <span key={c.path} style={{ display: 'flex', alignItems: 'center' }}>
              <span className="crumb-sep">/</span>
              <button className="crumb-item" onClick={() => setDir(c.path)}>
                {c.name}
              </button>
            </span>
          ))}
        </div>

        <div style={{ flex: 1 }} />

        <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
          {loading ? '读取中…' : unsupported ? '' : `${images.length} 张`}
        </span>
        <button className="btn sm" onClick={() => void load(provider, dir)} disabled={loading}>
          刷新
        </button>
      </div>

      {loading && (
        <div className="empty" style={{ height: 160 }}>
          <IconLoader />
          <span>正在读图床…</span>
        </div>
      )}

      {/* 不支持浏览：退回本工具传过的记录，并说明原因 */}
      {!loading && unsupported && (
        <>
          <div className="list-note">
            这个图床不支持浏览已有图片（{unsupported.replace(/^Error:\s*/, '')}），
            下面只列出本工具传过的。
          </div>
          {fallback.length === 0 ? (
            <div className="empty" style={{ height: 140 }}>
              <IconImage size={26} />
              <span>还没有传过图</span>
            </div>
          ) : (
            <div className="img-grid">
              {fallback.map((h, i) => (
                <div className="img-card" key={h.id} onClick={() => void copy(h.url, `h${i}`)}>
                  <div className="img-thumb">
                    <span className="img-fallback">{isRemote(h.url) ? '🖼' : '🗂'}</span>
                    {isRemote(h.url) ? (
                      <img
                        src={h.url}
                        alt=""
                        loading="lazy"
                        onError={(e) => (e.currentTarget.style.display = 'none')}
                      />
                    ) : h.localPath ? (
                      <LocalThumb path={h.localPath} />
                    ) : null}
                  </div>
                  <div className="img-name" title={h.url}>
                    {h.name}
                  </div>
                  <div className="img-meta">
                    {kb(h.bytes)} <span className="dot">·</span> {fmtTime(h.createdAt)}
                  </div>
                  {copied === `h${i}` && <span className="img-copied">已复制</span>}
                </div>
              ))}
            </div>
          )}
        </>
      )}

      {/* 正常：子目录 + 图片网格 */}
      {!loading && !unsupported && (
        <>
          {dirs.length > 0 && (
            <div className="dir-row">
              {dirs.map((d) => (
                <button key={d.path} className="dir-chip" onClick={() => setDir(d.path)}>
                  📁 {d.name}
                </button>
              ))}
            </div>
          )}

          {images.length === 0 ? (
            <div className="empty" style={{ height: 160 }}>
              <IconImage size={26} />
              <span>{dirs.length > 0 ? '这个目录里没有图片，进子目录看看' : '这个目录还是空的'}</span>
            </div>
          ) : (
            <div className="img-grid">
              {images.map((r, i) => (
                <div
                  className="img-card"
                  key={r.path}
                  title={`${r.url}\n点击复制链接`}
                  onClick={() => void copy(r.url, `i${i}`)}
                >
                  <div className="img-thumb">
                    <span className="img-fallback">{isRemote(r.url) ? '🖼' : '🗂'}</span>
                    {isRemote(r.url) ? (
                      <img
                        src={r.url}
                        alt=""
                        loading="lazy"
                        onError={(e) => (e.currentTarget.style.display = 'none')}
                      />
                    ) : (
                      <LocalThumb path={fileUrlToPath(r.url) ?? ''} />
                    )}
                  </div>
                  <div className="img-name">{r.name}</div>
                  <div className="img-meta">
                    {kb(r.size)}
                    {r.modifiedAt ? (
                      <>
                        <span className="dot">·</span> {fmtTime(r.modifiedAt)}
                      </>
                    ) : null}
                  </div>

                  {/* hover 才出现的小动作，不占卡片空间 */}
                  <div className="img-acts">
                    <button
                      className="img-act"
                      title="复制 Markdown"
                      onClick={(e) => {
                        e.stopPropagation()
                        void copyMarkdown(r, `m${i}`)
                      }}
                    >
                      MD
                    </button>
                    <button
                      className="img-act"
                      title="打开链接"
                      onClick={(e) => {
                        e.stopPropagation()
                        void api.openPath(r.url)
                      }}
                    >
                      ↗
                    </button>
                  </div>

                  {copied === `i${i}` || copied === `m${i}` ? (
                    <span className="img-copied">
                      {copied === `i${i}` ? (
                        <>
                          <IconCheck size={11} /> 已复制
                        </>
                      ) : (
                        <>
                          <IconCopy size={11} /> MD
                        </>
                      )}
                    </span>
                  ) : null}
                </div>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  )
}
