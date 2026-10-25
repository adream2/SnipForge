import { useCallback, useEffect, useRef, useState } from 'react'
import { open } from '@tauri-apps/plugin-dialog'
import { api } from '../lib/api'
import { IconImage, IconLoader, IconSave, IconUpload, IconCopy } from '../components/Icons'
import { useToast } from '../components/Toast'

interface LoadedImage {
  dataUrl: string
  name: string
  width: number
  height: number
  bytes: number
  format: string
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
  const toast = useToast()
  const fileRef = useRef<HTMLInputElement>(null)

  const loadFromDataUrl = useCallback(async (dataUrl: string, name: string) => {
    const info = await api.imageInfo(dataUrl)
    setImg({
      dataUrl,
      name,
      width: info.width,
      height: info.height,
      bytes: info.bytes,
      format: guessFormat(info.format)
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
    // 主进程不开放任意文件读取，这里借系统对话框拿到路径后由后端读取为 data URL
    try {
      const png = await import('@tauri-apps/api/core').then((m) =>
        m.convertFileSrc(picked)
      )
      const res = await fetch(png)
      const blob = await res.blob()
      const dataUrl = await new Promise<string>((resolve, reject) => {
        const reader = new FileReader()
        reader.onload = () => resolve(String(reader.result))
        reader.onerror = reject
        reader.readAsDataURL(blob)
      })
      await loadFromDataUrl(dataUrl, picked.split(/[\\/]/).pop() ?? 'image')
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
          <button className="btn sm" onClick={() => { setImg(null); setResult(null) }}>
            换一张
          </button>
        )}
      </div>

      <div className="page-body">
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
                      const out = await api.uploadImage(result.dataUrl)
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
    </>
  )
}
