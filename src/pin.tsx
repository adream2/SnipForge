import { useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { PhysicalPosition, PhysicalSize } from '@tauri-apps/api/dpi'
import { api, events } from './lib/api'
import './styles/pin.css'

/**
 * 贴图窗口：把一张图钉在屏幕最上层当参考。
 *
 * 交互取舍：
 *   - 左键拖动移动（没有标题栏，拖动整块就是唯一的抓手）
 *   - 滚轮缩放窗口尺寸（图片 contain，所以窗口大小 = 可见大小）
 *   - **四角等比缩放**：只保留角上的把手，并且强制保持宽高比。
 *     贴图是拿来比对/描画的，被拉扁就没有参考价值了 —— 所以窗口本身
 *     设成不可调（OS 的边框中拖是自由拉伸的），边和角的拉伸全部由这里接管。
 *   - 右键单击关闭（贴图是临时参考物，用完就想让它消失）
 *   - Ctrl+右键：复制 / 另存 / 透明度菜单
 *   - 双击关闭，Esc 关闭
 */

type Corner = 'nw' | 'ne' | 'sw' | 'se'

const MIN_W = 80

export default function Pin() {
  const [src, setSrc] = useState<string | null>(null)
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)
  const [opacity, setOpacity] = useState(1)
  const [flash, setFlash] = useState('')
  /** 宽高比（宽 / 高），由图片自身决定；等比缩放的唯一依据 */
  const ratio = useRef(1)
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
      const w = Math.max(MIN_W, Math.round(size.width * factor))
      // 高度跟着宽度算，别让滚轮把图越滚越扁
      const h = Math.max(1, Math.round(w / (ratio.current || 1)))
      await win.setSize(new PhysicalSize(w, h))
    } catch {
      /* 窗口正在销毁时可能失败，忽略 */
    }
  }

  /**
   * 四角等比缩放。
   *
   * 这里故意不用 CSS resize 或窗口边框：两种都是自由拉伸。拖动一个角时，
   * 宽度按指针位移算，高度永远由宽高比推出来；同时把窗口位置挪回去，
   * 让**对角那个角钉在原地** —— 不然图会一边缩一边往左上跑。
   */
  const startResize = (corner: Corner) => (e: React.PointerEvent) => {
    e.preventDefault()
    e.stopPropagation()
    if (e.button !== 0) return
    // 用**屏幕坐标**算位移：窗口自己在缩，clientX 是相对窗口的，
    // 一缩就跟着变，会把缩放拖成抖动的死循环。
    e.currentTarget.setPointerCapture(e.pointerId)
    const start = { x: e.screenX, y: e.screenY }
    const dpr = window.devicePixelRatio || 1
    void (async () => {
      const size = await win.innerSize()
      const pos = await win.outerPosition()
      const startW = size.width
      const startH = size.height
      const r = ratio.current || startW / Math.max(1, startH)

      const move = async (ev: PointerEvent) => {
        const dx = (ev.screenX - start.x) * dpr
        const dy = (ev.screenY - start.y) * dpr
        // 横向拖用 dx、纵向拖用 dy，哪个幅度大听哪个（斜着拖更跟手）
        const wByX =
          corner === 'nw' || corner === 'sw' ? startW - dx : startW + dx
        const hByY =
          corner === 'nw' || corner === 'ne' ? startH - dy : startH + dy
        let w = Math.abs(dx) >= Math.abs(dy) ? wByX : hByY * r
        w = Math.max(MIN_W, Math.round(w))
        const h = Math.max(1, Math.round(w / r))
        try {
          await win.setSize(new PhysicalSize(w, h))
          await win.setPosition(
            new PhysicalPosition(
              Math.round(corner === 'nw' || corner === 'sw' ? pos.x + startW - w : pos.x),
              Math.round(corner === 'nw' || corner === 'ne' ? pos.y + startH - h : pos.y)
            )
          )
        } catch {
          /* 窗口关闭中，忽略 */
        }
      }
      const up = () => {
        window.removeEventListener('pointermove', move)
        window.removeEventListener('pointerup', up)
      }
      window.addEventListener('pointermove', move)
      window.addEventListener('pointerup', up)
    })()
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
      // 右键单击直接关掉：贴图是临时参考物，用完就想让它消失。
      // 留 Ctrl+右键 弹菜单，复制/另存这些低频操作不至于没有入口。
      onContextMenu={(e) => {
        e.preventDefault()
        if (e.ctrlKey) {
          setMenu({ x: e.clientX, y: e.clientY })
        } else {
          void win.close()
        }
      }}
      onMouseDown={(e) => {
        setMenu(null)
        if (e.button === 0) void win.startDragging()
      }}
      onDoubleClick={() => void win.close()}
    >
      {src && (
        <img
          src={src}
          alt=""
          draggable={false}
          style={{ opacity }}
          onLoad={(e) => {
            const img = e.currentTarget
            if (img.naturalWidth > 0 && img.naturalHeight > 0) {
              ratio.current = img.naturalWidth / img.naturalHeight
            }
          }}
        />
      )}

      {/* 四个角是仅有的缩放入口：只等比，不给你拉扁 */}
      {(['nw', 'ne', 'sw', 'se'] as Corner[]).map((c) => (
        <div
          key={c}
          className={`pin-handle pin-handle-${c}`}
          onPointerDown={startResize(c)}
          onMouseDown={(e) => e.stopPropagation()}
        />
      ))}

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
