import { useCallback, useEffect, useRef, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { api } from '../lib/api'
import { extOf, prepareWithSettings } from '../lib/compress'
import { IconImage, IconLoader, IconSave, IconUpload, IconCopy } from '../components/Icons'
import { useToast } from '../components/Toast'

interface LoadedImage {
  dataUrl: string
  name: string
  width: number
  height: number
  bytes: number
  format: string
  /** 从磁盘载入时的原始路径（拖进来的没有）——"在资源管理器打开"要用 */
  path?: string
}

const FORMATS = [
  { id: 'image/png', label: 'PNG', ext: 'png', lossy: false },
  { id: 'image/jpeg', label: 'JPEG', ext: 'jpg', lossy: true },
  { id: 'image/webp', label: 'WebP', ext: 'webp', lossy: true }
] as const

function guessFormat(mime: string): string {
  if (mime.includes('jpeg')) return 'image/jpeg'
  if (mime.includes('webp')) return 'image/webp'
  return 'image/png'
}

/**
 * 图片工具：格式转换 / 压缩 / 缩放。
 *
 * 全部在渲染进程用 canvas 完成 —— 不额外引入图像库，
 * 也天然享受浏览器的高质量重采样（比手写双线性好得多）。
 */
export default function ToolsPage() {
  const [img, setImg] = useState<LoadedImage | null>(null)
  const [format, setFormat] = useState<string>('image/webp')
  const [quality, setQuality] = useState(88)
  const [scale, setScale] = useState(100)
  const [busy, setBusy] = useState(false)
  const [result, setResult] = useState<{ dataUrl: string; bytes: number; w: number; h: number } | null>(null)
  const [dragging, setDragging] = useState(false)
  /** 右键菜单的屏幕坐标；null = 不显示 */
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)
  const toast = useToast()
  const fileRef = useRef<HTMLInputElement>(null)

  const loadFromDataUrl = useCallback(async (dataUrl: string, name: string, path?: string) => {
    const info = await api.imageInfo(dataUrl)
    setImg({
      dataUrl,
      name,
      width: info.width,
      height: info.height,
      bytes: info.bytes,
      format: guessFormat(info.format),
      path
    })
    setFormat(info.format.includes('png') ? 'image/webp' : guessFormat(info.format))
    setScale(100)
    setResult(null)
  }, [])

  const pickFile = async () => {
    const picked = await open({
      multiple: false,
      filters: [{ name: '图片', extensions: ['png', 'jpg', 'jpeg', 'webp', 'bmp', 'gif'] }]
    })
    if (typeof picked !== 'string') return
    // 由后端读文件并转成 data URL。别用 convertFileSrc + fetch：
    // 那条路要开 asset 协议、还要把 asset 放进 CSP 的 connect-src，
    // 默认配置下走不通 —— 表现就是"选了图片没反应"。
    try {
      const f = await api.imageLoadFile(picked)
      setImg({
        dataUrl: f.dataUrl,
        name: picked.split(/[\\/]/).pop() ?? 'image',
        width: f.width,
        height: f.height,
        bytes: f.bytes,
        format: guessFormat(f.format),
        path: picked
      })
      setFormat(f.format.includes('png') ? 'image/webp' : guessFormat(f.format))
      setScale(100)
      setResult(null)
    } catch (e) {
      toast.push(`读取失败：${e}`, 'error')
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
    await loadFromDataUrl(dataUrl, file.name)
  }

  /* ---------------- 右键菜单里的快捷操作 ---------------- */

  /** 有转换结果就操作结果，否则操作原图 */
  const activeUrl = result?.dataUrl ?? img?.dataUrl ?? null
  const activeName = img
    ? `${img.name.replace(/\.[^.]+$/, '')}.${FORMATS.find((f) => f.id === format)?.ext ?? 'png'}`
    : 'image.png'

  const doCopy = async () => {
    if (!activeUrl) return
    try {
      await api.imageCopy(activeUrl)
      toast.push('已复制到剪贴板', 'success')
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const doSaveAs = async () => {
    if (!activeUrl) return
    try {
      const p = await api.imageSaveAs(activeUrl, activeName)
      if (p) toast.push(`已保存到 ${p}`, 'success')
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const doPin = async () => {
    if (!activeUrl) return
    try {
      await api.imagePin(activeUrl)
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const doUpload = async () => {
    if (!activeUrl) return
    try {
      // 先按图床配置压缩/转格式，再传
      const p = await prepareWithSettings(activeUrl)
      const out = await api.uploadImage(p.dataUrl, undefined, `image.${extOf(p.format)}`)
      await api.clipboardCopyText(out.result.url)
      toast.push(`已上传并复制链接：${out.result.url}`, 'success')
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const doOcr = async () => {
    if (!activeUrl) return
    try {
      const r = await api.imageOcr(activeUrl)
      const text = (r.text ?? '').trim()
      if (!text) {
        toast.push('没识别到文字', 'info')
        return
      }
      await api.clipboardCopyText(text)
      toast.push(`已识别 ${text.length} 字并复制`, 'success')
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const doOpenFolder = async () => {
    if (!img?.path) return
    try {
      await api.openImageFolder(img.path)
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const resetAll = () => {
    setImg(null)
    setResult(null)
    setMenu(null)
  }

  /** 右键唤出菜单（坐标要夹在视口内，否则贴边会跑出去） */
  const openMenu = (e: React.MouseEvent) => {
    if (!img) return
    e.preventDefault()
    setMenu({
      x: Math.min(e.clientX, window.innerWidth - 208),
      y: Math.min(e.clientY, window.innerHeight - 268)
    })
  }

  // 菜单开着时：Esc 关、点别处关、Ctrl+C / Ctrl+S 走快捷键
  useEffect(() => {
    if (!menu && !img) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMenu(null)
      if (!img) return
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'c') {
        e.preventDefault()
        void doCopy()
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
        e.preventDefault()
        void doSaveAs()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [menu, img, result, format])

  /** 重新编码：canvas 重采样 + 目标格式导出 */
  const convert = useCallback(async () => {
    if (!img) return
    setBusy(true)
    try {
      const image = new Image()
      await new Promise<void>((resolve, reject) => {
        image.onload = () => resolve()
        image.onerror = reject
        image.src = img.dataUrl
      })

      const w = Math.max(1, Math.round((img.width * scale) / 100))
      const h = Math.max(1, Math.round((img.height * scale) / 100))
      const canvas = document.createElement('canvas')
      canvas.width = w
      canvas.height = h
      const ctx = canvas.getContext('2d')!
      // JPEG 没有透明通道，先铺白底，否则透明区域会变黑
      if (format === 'image/jpeg') {
        ctx.fillStyle = '#ffffff'
        ctx.fillRect(0, 0, w, h)
      }
      ctx.imageSmoothingEnabled = true
      ctx.imageSmoothingQuality = 'high'
      ctx.drawImage(image, 0, 0, w, h)

      const dataUrl = canvas.toDataURL(format, quality / 100)
      const bytes = Math.round((dataUrl.length - dataUrl.indexOf(',') - 1) * 0.75)
      setResult({ dataUrl, bytes, w, h })
    } catch (e) {
      toast.push(`处理失败：${e}`, 'error')
    } finally {
      setBusy(false)
    }
  }, [img, format, quality, scale, toast])

  useEffect(() => {
    if (img) void convert()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [img, format, quality, scale])

  const ext = FORMATS.find((f) => f.id === format)
  const saving = result && img ? ((1 - result.bytes / img.bytes) * 100).toFixed(0) : null

  return (
    <>
      <div className="page-head">
        <h1>图片工具</h1>
        <div className="spacer" />
        {img && (
          <button className="btn sm" onClick={resetAll}>
            换一张
          </button>
        )}
      </div>

      <div className="page-body" onContextMenu={openMenu}>
        {!img ? (
          <div
            className={`tool-drop ${dragging ? 'dragging' : ''}`}
            onDragOver={(e) => { e.preventDefault(); setDragging(true) }}
            onDragLeave={() => setDragging(false)}
            onDrop={handleDrop}
            onClick={() => void pickFile()}
            style={{ cursor: 'pointer', marginTop: 40 }}
          >
            <IconImage size={34} />
            <div style={{ marginTop: 12, fontSize: 13.5, fontWeight: 500, color: 'var(--text-2)' }}>
              拖入图片，或点击选择
            </div>
            <div style={{ marginTop: 4, fontSize: 12 }}>
              支持 PNG / JPEG / WebP / BMP · 格式转换、压缩、缩放
            </div>
          </div>
        ) : (
          <div style={{ display: 'grid', gridTemplateColumns: '1fr 268px', gap: 14, alignItems: 'start' }}>
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
                src={result?.dataUrl ?? img.dataUrl}
                alt=""
                onContextMenu={openMenu}
                title="右键：复制 / 另存为 / 钉图 / 上传 / 取字"
                style={{ maxWidth: '100%', maxHeight: 460, borderRadius: 6, display: 'block' }}
              />
            </div>

            <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
              <div className="setting-card">
                <header>输出参数</header>
                <div style={{ padding: '10px 16px 16px', display: 'flex', flexDirection: 'column', gap: 13 }}>
                  <div className="field">
                    <label>格式</label>
                    <div className="seg">
                      {FORMATS.map((f) => (
                        <button
                          key={f.id}
                          className={format === f.id ? 'active' : ''}
                          onClick={() => setFormat(f.id)}
                          style={{ flex: 1 }}
                        >
                          {f.label}
                        </button>
                      ))}
                    </div>
                  </div>

                  {ext?.lossy && (
                    <div className="field">
                      <label style={{ display: 'flex', justifyContent: 'space-between' }}>
                        <span>质量</span>
                        <span style={{ color: 'var(--text-3)' }}>{quality}</span>
                      </label>
                      <input
                        type="range"
                        className="slider"
                        min={30}
                        max={100}
                        value={quality}
                        onChange={(e) => setQuality(Number(e.target.value))}
                        style={{ background: '#e0e0e6' }}
                      />
                    </div>
                  )}

                  <div className="field">
                    <label style={{ display: 'flex', justifyContent: 'space-between' }}>
                      <span>尺寸</span>
                      <span style={{ color: 'var(--text-3)' }}>{scale}%</span>
                    </label>
                    <input
                      type="range"
                      className="slider"
                      min={10}
                      max={200}
                      step={5}
                      value={scale}
                      onChange={(e) => setScale(Number(e.target.value))}
                      style={{ background: '#e0e0e6' }}
                    />
                  </div>
                </div>
              </div>

              <div className="setting-card">
                <header>结果</header>
                <div style={{ padding: '10px 16px 16px', fontSize: 12.5, color: 'var(--text-2)', lineHeight: 1.9 }}>
                  <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                    <span>原图</span>
                    <span>{(img.bytes / 1024).toFixed(1)} KB · {img.width}×{img.height}</span>
                  </div>
                  {result && (
                    <>
                      <div style={{ display: 'flex', justifyContent: 'space-between' }}>
                        <span>输出</span>
                        <span style={{ color: 'var(--text)' }}>
                          {(result.bytes / 1024).toFixed(1)} KB · {result.w}×{result.h}
                        </span>
                      </div>
                      {saving && Number(saving) > 0 && (
                        <div style={{ marginTop: 6 }}>
                          <span className="tag success">小了 {saving}%</span>
                        </div>
                      )}
                    </>
                  )}
                </div>
              </div>

              <div style={{ display: 'flex', gap: 8 }}>
                <button
                  className="btn primary"
                  style={{ flex: 1 }}
                  disabled={!result || busy}
                  onClick={async () => {
                    if (!result) return
                    try {
                      const out = await api.imageSave(
                        result.dataUrl,
                        `${img.name.replace(/\.[^.]+$/, '')}.${ext?.ext ?? 'png'}`,
                        true
                      )
                      toast.push(`已保存到 ${out.path}`, 'success')
                    } catch (e) {
                      toast.push(String(e), 'error')
                    }
                  }}
                >
                  {busy ? <IconLoader /> : <IconSave size={14} />} 保存
                </button>
                <button
                  className="btn"
                  disabled={!result}
                  onClick={async () => {
                    if (!result) return
                    try {
                      await api.imageCopy(result.dataUrl)
                      toast.push('已复制到剪贴板', 'success')
                    } catch (e) {
                      toast.push(String(e), 'error')
                    }
                  }}
                >
                  <IconCopy size={14} />
                </button>
                <button
                  className="btn"
                  disabled={!result}
                  onClick={async () => {
                    if (!result) return
                    try {
                      const p = await prepareWithSettings(result.dataUrl)
                      const out = await api.uploadImage(p.dataUrl, undefined, `image.${extOf(p.format)}`)
                      await api.clipboardCopyText(out.result.url)
                      toast.push(`已上传并复制链接：${out.result.url}`, 'success')
                    } catch (e) {
                      toast.push(String(e), 'error')
                    }
                  }}
                >
                  <IconUpload size={14} />
                </button>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* 右键快捷菜单：不占版面，操作完自动消失 */}
      {menu && img && (
        <>
          <div
            className="ctx-shade"
            onClick={() => setMenu(null)}
            onContextMenu={(e) => {
              e.preventDefault()
              setMenu(null)
            }}
          />
          <div className="ctx-menu" style={{ left: menu.x, top: menu.y }}>
            <button onClick={() => { setMenu(null); void doCopy() }}>
              <span>复制图片</span>
              <kbd>Ctrl+C</kbd>
            </button>
            <button onClick={() => { setMenu(null); void doSaveAs() }}>
              <span>另存为…</span>
              <kbd>Ctrl+S</kbd>
            </button>
            <button onClick={() => { setMenu(null); void doPin() }}>
              <span>钉在屏幕上</span>
            </button>
            <div className="ctx-sep" />
            <button onClick={() => { setMenu(null); void doOcr() }}>
              <span>OCR 取字</span>
            </button>
            <button onClick={() => { setMenu(null); void doUpload() }}>
              <span>上传并复制链接</span>
            </button>
            {img.path && (
              <button onClick={() => { setMenu(null); void doOpenFolder() }}>
                <span>在资源管理器中打开</span>
              </button>
            )}
            <div className="ctx-sep" />
            <button onClick={resetAll}>
              <span>换一张</span>
            </button>
          </div>
        </>
      )}
    </>
  )
}
