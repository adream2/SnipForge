import { useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { PhysicalSize } from '@tauri-apps/api/dpi'
import { api, events } from './lib/api'
import './styles/pin.css'

/**
 * 贴图窗口：把一张图钉在屏幕最上层当参考。
 *
 * 交互取舍：
 *   - 左键拖动移动（没有标题栏，拖动整块就是唯一的抓手）
 *   - 滚轮缩放窗口尺寸（图片 contain，所以窗口大小 = 可见大小）
 *   - 右键菜单：复制 / 另存 / 透明度 / 关闭
 *   - 双击关闭，Esc 关闭
 */
export default function Pin() {
  const [src, setSrc] = useState<string | null>(null)
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)
  const [opacity, setOpacity] = useState(1)
  const [flash, setFlash] = useState('')
  const win = getCurrentWindow()

  useEffect(() => {
    const unsub = events.onPinInit((p) => {
      setSrc(p.dataUrl)
      if (typeof p.opacity === 'number' && p.opacity > 0) setOpacity(p.opacity)
    })
    return () => {
      void unsub.then((u) => u())
    }
  }, [])

  // Esc 关掉当前贴图
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') void win.close()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [win])

  /** 滚轮缩放：改窗口尺寸而不是 CSS transform —— 这样窗口边缘就是图片边缘，不会挡到后面的操作 */
  const onWheel = async (e: React.WheelEvent) => {
    const factor = e.deltaY > 0 ? 0.9 : 1.1
    try {
      const size = await win.innerSize()
      const w = Math.max(80, Math.round(size.width * factor))
      const h = Math.max(60, Math.round(size.height * factor))
      await win.setSize(new PhysicalSize(w, h))
    } catch {
      /* 窗口正在销毁时可能失败，忽略 */
    }
  }

  const copyImage = async () => {
    if (!src) return
    setMenu(null)
    try {
      await api.imageCopy(src)
      setFlash('已复制')
      setTimeout(() => setFlash(''), 900)
    } catch {
      setFlash('复制失败')
      setTimeout(() => setFlash(''), 1200)
    }
  }

  const saveImage = async () => {
    if (!src) return
    setMenu(null)
    await api.imageSaveAs(src).catch(() => {})
  }

  return (
    <div
      className="pin-root"
      onWheel={onWheel}
      onContextMenu={(e) => {
        e.preventDefault()
        setMenu({ x: e.clientX, y: e.clientY })
      }}
      onMouseDown={(e) => {
        setMenu(null)
        if (e.button === 0) void win.startDragging()
      }}
      onDoubleClick={() => void win.close()}
    >
      {src && <img src={src} alt="" draggable={false} style={{ opacity }} />}

      {flash && <div className="pin-flash">{flash}</div>}

      {menu && (
        <div
          className="pin-menu"
          style={{ left: Math.min(menu.x, window.innerWidth - 160), top: Math.min(menu.y, window.innerHeight - 150) }}
          onClick={(e) => e.stopPropagation()}
          onMouseDown={(e) => e.stopPropagation()}
          onDoubleClick={(e) => e.stopPropagation()}
        >
          <button onClick={() => void copyImage()}>复制图片</button>
          <button onClick={() => void saveImage()}>另存为…</button>
          <div className="pin-menu-sep" />
          <div className="pin-menu-label">不透明度 {Math.round(opacity * 100)}%</div>
          <input
            type="range"
            min={20}
            max={100}
            value={Math.round(opacity * 100)}
            onChange={(e) => setOpacity(Number(e.target.value) / 100)}
            onClick={(e) => e.stopPropagation()}
          />
          <div className="pin-menu-sep" />
          <button className="danger" onClick={() => void win.close()}>
            关闭贴图
          </button>
        </div>
      )}
    </div>
  )
}

// 独立窗口的挂载点
const container = document.getElementById('root')
if (container) {
  createRoot(container).render(<Pin />)
}
