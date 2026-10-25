import { useCallback, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { api, events } from './lib/api'
import type { DisplayPreview, FramePayload, NativeWindowInfo, Rect } from './lib/types'
import { IconCheck, IconWindow, IconMonitor, IconScroll, IconX } from './components/Icons'
import './styles/selector.css'

/**
 * 截图覆盖层。
 *
 * 坐标系有三套，必须时刻清楚自己在哪一套里：
 *   1. 物理像素 —— 后端返回的显示器 bounds、窗口 bounds
 *   2. CSS 像素  —— DOM 里鼠标事件拿到的
 *   3. 换算关系  —— css = (phys - displayOrigin) / scaleFactor
 *
 * 提交给后端的一律是物理像素。
 */
/** 从 URL 读模式：selector.html?mode=record 表示框完直接开录 */
const IS_RECORD =
  typeof location !== 'undefined' &&
  new URLSearchParams(location.search).get('mode') === 'record'

export default function Selector() {
  const [frame, setFrame] = useState<FramePayload | null>(null)
  const [display, setDisplay] = useState<DisplayPreview | null>(null)
  const [windows, setWindows] = useState<NativeWindowInfo[]>([])
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null)
  const [cursor, setCursor] = useState<{ x: number; y: number }>({ x: 0, y: 0 })
  const [dragging, setDragging] = useState(false)
  const [selection, setSelection] = useState<Rect | null>(null)
  const [hoverWindow, setHoverWindow] = useState<NativeWindowInfo | null>(null)
  const [mode, setMode] = useState<'region' | 'window' | 'scroll'>('region')
  const [busy, setBusy] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)

  const scale = display?.scaleFactor ?? 1

  useEffect(() => {
    let disposed = false
    const boot = async () => {
      try {
        const payload = await api.captureFrame()
        if (disposed) return
        setFrame(payload)
        setDisplay(payload.displays[0] ?? null)
        const wins = await api.captureWindows()
        if (!disposed) setWindows(wins)
      } catch {
        // 帧已失效，直接退出
        void api.captureCancel()
      }
    }
    void boot()
    const unsub = events.onSelectorDisplay((id) => {
      setFrame((prev) => {
        const d = prev?.displays.find((x) => x.id === id)
        if (d) setDisplay(d)
        return prev
      })
    })
    return () => {
      disposed = true
      void unsub.then((u) => u())
    }
  }, [])

  /** 物理坐标 → CSS 坐标 */
  const toCss = useCallback(
    (physX: number, physY: number) => {
      if (!display) return { x: 0, y: 0 }
      return {
        x: (physX - display.bounds.x) / scale,
        y: (physY - display.bounds.y) / scale
      }
    },
    [display, scale]
  )

  /** 命中测试：点到哪个窗口上 */
  const hitTest = useCallback(
    (cssX: number, cssY: number): NativeWindowInfo | null => {
      if (!display) return null
      const physX = display.bounds.x + cssX * scale
      const physY = display.bounds.y + cssY * scale
      for (const w of windows) {
        if (w.ignored) continue
        const r = w.rect
        if (physX >= r.x && physX < r.x + r.width && physY >= r.y && physY < r.y + r.height) {
          return w
        }
      }
      return null
    },
    [display, scale, windows]
  )

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        void api.captureCancel()
      }
      if (e.key === 'Enter' && selection) {
        e.preventDefault()
        void commit()
      }
      if (e.key === 's' && !e.ctrlKey && !e.metaKey) setMode('scroll')
      if (e.key === 'w' && !e.ctrlKey && !e.metaKey && !e.altKey) setMode('window')
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selection])

  const normalize = (a: { x: number; y: number }, b: { x: number; y: number }): Rect => ({
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(a.x - b.x),
    height: Math.abs(a.y - b.y)
  })

  const onMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0 || busy) return
    const rect = rootRef.current?.getBoundingClientRect()
    if (!rect) return
    const x = e.clientX - rect.left
    const y = e.clientY - rect.top

    if (mode === 'window') {
      const target = hitTest(x, y)
      if (target) {
        // 窗口 rect 是物理像素，selection 这一层全是 CSS 像素
        // （commit 里还会乘回物理像素）。不换算的话，缩放屏上
        // 选出来的框和窗口对不上，尺寸标签也是错的。
        setSelection({
          x: (target.rect.x - (display?.bounds.x ?? 0)) / scale,
          y: (target.rect.y - (display?.bounds.y ?? 0)) / scale,
          width: target.rect.width / scale,
          height: target.rect.height / scale
        })
        return
      }
    }
    setAnchor({ x, y })
    setCursor({ x, y })
    setDragging(true)
    setSelection({ x, y, width: 0, height: 0 })
  }

  const onMouseMove = (e: React.MouseEvent) => {
    const rect = rootRef.current?.getBoundingClientRect()
    if (!rect) return
    const x = e.clientX - rect.left
    const y = e.clientY - rect.top

    if (dragging && anchor) {
      setCursor({ x, y })
      setSelection(normalize(anchor, { x, y }))
      setHoverWindow(null)
      return
    }
    if (mode === 'window') {
      setHoverWindow(hitTest(x, y))
    }
  }

  const onMouseUp = () => {
    setDragging(false)
  }

  const commit = async () => {
    if (!selection || !display || busy) return
    if (selection.width < 4 || selection.height < 4) return
    setBusy(true)
    // CSS → 物理
    const phys: Rect = {
      x: Math.round(display.bounds.x + selection.x * scale),
      y: Math.round(display.bounds.y + selection.y * scale),
      width: Math.round(selection.width * scale),
      height: Math.round(selection.height * scale)
    }
    try {
      if (mode === 'scroll' && !IS_RECORD) {
        // 录屏没有"滚动长录"这种东西，这里只可能来自截图模式
        await api.captureScroll(display.id, phys)
      } else {
        if (IS_RECORD) {
          // 录屏：后端会先关掉这个覆盖层再开采集，避免把深色遮罩录进去
          await api.recordStartRegion(phys)
        } else {
          await api.captureCommit(display.id, phys)
        }
      }
    } catch {
      setBusy(false)
      void api.captureCancel()
    }
  }

  const hoverRect = hoverWindow
    ? {
        left: (hoverWindow.rect.x - (display?.bounds.x ?? 0)) / scale,
        top: (hoverWindow.rect.y - (display?.bounds.y ?? 0)) / scale,
        width: hoverWindow.rect.width / scale,
        height: hoverWindow.rect.height / scale
      }
    : null

  const sizeLabel = selection
    ? `${Math.round(selection.width * scale)} × ${Math.round(selection.height * scale)}`
    : ''

  const toolbarStyle = (() => {
    if (selection) {
      const below = selection.y + selection.height + 10
      const top = below + 40 < window.innerHeight ? below : Math.max(8, selection.y - 46)
      const left = Math.min(Math.max(8, selection.x + selection.width - 244), window.innerWidth - 252)
      return { top, left } as React.CSSProperties
    }
    if (IS_RECORD) {
      // 录屏模式没有选区时工具栏也常驻：窗口选择（W）这个能力藏在一个
      // 看不见的快捷键后面，第一次用的人根本发现不了。
      // 放底部居中。注意要抬到任务栏上面 —— 任务栏是置顶窗口，
      // bottom:14 这种"贴边"位置会被它整个盖住，看起来像没渲染。
      return {
        bottom: 60,
        left: Math.max(8, (window.innerWidth - 236) / 2)
      } as React.CSSProperties
    }
    return { display: 'none' } as React.CSSProperties
  })()

  return (
    <div className="selector-root" ref={rootRef}>
      {/*
        交互层单独一个 div，铺满整个窗口但只包住背景和遮罩。
        工具栏是它的兄弟节点 —— 这样点按钮的 mousedown 根本不会冒泡到选区逻辑上。
        之前把事件绑在 root 上，点"确认"会先触发一次 mousedown 开始新选区，
        于是按钮永远点不动。
      */}
      <div
        className="selector-surface"
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={onMouseUp}
        onDoubleClick={() => void commit()}
        onContextMenu={(e) => {
          e.preventDefault()
          void api.captureCancel()
        }}
      >
      {display && (
        <img className="selector-bg" src={display.preview} alt="" draggable={false} />
      )}

      {hoverRect && !selection && mode === 'window' && (
        <div
          className="selector-win-hint"
          style={{
            left: hoverRect.left,
            top: hoverRect.top,
            width: hoverRect.width,
            height: hoverRect.height
          }}
        />
      )}

      {selection && (
        <>
          <div
            className="selector-mask"
            style={{
              left: selection.x,
              top: selection.y,
              width: Math.max(1, selection.width),
              height: Math.max(1, selection.height)
            }}
          />
          <div
            className="selector-edge"
            style={{
              left: selection.x,
              top: selection.y,
              width: Math.max(1, selection.width),
              height: Math.max(1, selection.height)
            }}
          />
          {/* 四角把手 */}
          {[
            ['nw', selection.x, selection.y],
            ['ne', selection.x + selection.width, selection.y],
            ['sw', selection.x, selection.y + selection.height],
            ['se', selection.x + selection.width, selection.y + selection.height]
          ].map(([k, l, t]) => (
            <div
              key={String(k)}
              className="selector-handle"
              style={{ left: Number(l) - 3, top: Number(t) - 3 }}
            />
          ))}
          <div
            className="selector-size-tag"
            style={{
              left: selection.x,
              top: selection.y + selection.height + 6 > window.innerHeight - 30
                ? selection.y - 26
                : selection.y + selection.height + 6
            }}
          >
            {sizeLabel}
          </div>
        </>
      )}

      </div>

      <div
        className="selector-toolbar"
        style={toolbarStyle}
        onMouseDown={(e) => e.stopPropagation()}
        onDoubleClick={(e) => e.stopPropagation()}
        onContextMenu={(e) => {
          e.preventDefault()
          e.stopPropagation()
        }}
      >
        <button
          className={mode === 'region' ? 'active' : ''}
          title="区域截图"
          onClick={() => setMode('region')}
        >
          <IconMonitor size={15} />
        </button>
        <button
          className={mode === 'window' ? 'active' : ''}
          title="窗口模式（W）：点哪个窗口录哪个"
          onClick={() => setMode('window')}
        >
          <IconWindow size={15} />
        </button>
        {!IS_RECORD && (
          <button
            className={mode === 'scroll' ? 'active' : ''}
            title="滚动长截图（S）"
            onClick={() => setMode('scroll')}
          >
            <IconScroll size={15} />
          </button>
        )}
        <div className="divider" />
        <button title="取消（Esc）" onClick={() => void api.captureCancel()}>
          <IconX size={15} />
        </button>
        <button
          title={IS_RECORD ? '开始录制（Enter）' : '确认（Enter）'}
          onClick={() => void commit()}
          style={{ background: 'var(--accent)' }}
        >
          <IconCheck size={15} />
        </button>
      </div>

      {!selection && (
        <div
          className="selector-hint"
          style={{ pointerEvents: 'none', bottom: IS_RECORD ? 112 : undefined }}
        >
          {IS_RECORD
            ? '拖拽框选 · 按 W 或点下方窗口图标选窗口 · 回车开始录制 · Esc 取消'
            : '拖拽框选 · 回车确认 · Esc 取消 · W 窗口模式 · S 长截图'}
        </div>
      )}
    </div>
  )
}

// 独立窗口的挂载点
const container = document.getElementById('root')
if (container) {
  createRoot(container).render(<Selector />)
}
