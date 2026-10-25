import { useCallback, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { api, events } from './lib/api'
import type { RecordLensEvt } from './lib/types'
import './styles/overlay.css'

type Tool = 'pen' | 'arrow' | 'rect' | 'highlight'

interface Stroke {
  id: number
  tool: Tool
  color: string
  width: number
  /** 起止点（CSS 像素，区域相对） */
  x1: number
  y1: number
  x2: number
  y2: number
  /** 自由画笔的点 */
  points: Array<{ x: number; y: number }>
  /** 生成时间，用来做淡出 */
  at: number
}

const COLORS = ['#ff3b30', '#ffcc00', '#34c759', '#0a84ff']
const FADE_MS = 6000
const RING = 44

function Overlay() {
  /** 光标位置（CSS 像素） */
  const [cur, setCur] = useState<{ x: number; y: number } | null>(null)
  const [recording, setRecording] = useState(false)
  const [ripples, setRipples] = useState<Array<{ id: number; x: number; y: number }>>([])
  const [lens, setLens] = useState<RecordLensEvt | null>(null)
  const [anno, setAnno] = useState(false)
  const [tool, setTool] = useState<Tool>('pen')
  const [color, setColor] = useState(COLORS[0])
  const [strokes, setStrokes] = useState<Stroke[]>([])

  const firstCursor = useRef(false)
  const canvasRef = useRef<HTMLCanvasElement>(null)
  const drawing = useRef<Stroke | null>(null)
  const seq = useRef(1)
  const dpr = window.devicePixelRatio || 1

  /* ---------------- 后端事件 ---------------- */

  useEffect(() => {
    // 挂载时报告一次视口尺寸：覆盖层是"按物理像素摆、按 CSS 像素画"，
    // 这两个数对不上时鼠标环会整体偏移，直接记下来省得反复猜。
    void api.log('info', `覆盖层就绪 ${window.innerWidth}×${window.innerHeight} dpr=${dpr}`)

    const unsubs: Array<() => void> = []
    void events
      .onRecordCursor((p) => {
        if (!firstCursor.current) {
          firstCursor.current = true
          void api.log('info', `首个光标事件 ${p.x},${p.y} recording=${p.recording}`)
        }
        // 物理像素 → CSS 像素
        setCur({ x: p.x / dpr, y: p.y / dpr })
        setRecording(p.recording)
      })
      .then((u) => unsubs.push(u))

    void events
      .onRecordClick((p) => {
        const id = seq.current++
        setRipples((list) => [...list, { id, x: p.x / dpr, y: p.y / dpr }])
        window.setTimeout(() => {
          setRipples((list) => list.filter((r) => r.id !== id))
        }, 620)
      })
      .then((u) => unsubs.push(u))

    void events.onRecordLens((p) => setLens(p)).then((u) => unsubs.push(u))

    // 控制条切标注模式时后端会广播，保证两边状态一致
    void events
      .on<boolean>('record://anno', (on) => setAnno(on))
      .then((u) => unsubs.push(u))

    return () => unsubs.forEach((u) => u())
  }, [dpr])

  /* ---------------- 标注画布 ---------------- */

  const redraw = useCallback(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    const ctx = canvas.getContext('2d')
    if (!ctx) return
    ctx.clearRect(0, 0, canvas.width, canvas.height)
    ctx.save()
    ctx.scale(dpr, dpr)

    const all = drawing.current ? [...strokes, drawing.current] : strokes
    for (const s of all) {
      ctx.strokeStyle = s.color
      ctx.fillStyle = s.color
      ctx.lineWidth = s.width
      ctx.lineCap = 'round'
      ctx.lineJoin = 'round'

      if (s.tool === 'pen') {
        if (s.points.length < 2) continue
        ctx.beginPath()
        ctx.moveTo(s.points[0].x, s.points[0].y)
        for (const p of s.points.slice(1)) ctx.lineTo(p.x, p.y)
        ctx.stroke()
      } else if (s.tool === 'highlight') {
        // 荧光笔：半透明粗线
        ctx.globalAlpha = 0.35
        ctx.lineWidth = s.width * 6
        ctx.beginPath()
        ctx.moveTo(s.x1, s.y1)
        ctx.lineTo(s.x2, s.y2)
        ctx.stroke()
        ctx.globalAlpha = 1
      } else if (s.tool === 'rect') {
        ctx.strokeRect(s.x1, s.y1, s.x2 - s.x1, s.y2 - s.y1)
      } else {
        // 箭头：线 + 三角头，头部比例固定
        const a = Math.atan2(s.y2 - s.y1, s.x2 - s.x1)
        const head = Math.max(12, s.width * 4)
        ctx.beginPath()
        ctx.moveTo(s.x1, s.y1)
        ctx.lineTo(s.x2 - Math.cos(a) * head * 0.6, s.y2 - Math.sin(a) * head * 0.6)
        ctx.stroke()
        ctx.beginPath()
        ctx.moveTo(s.x2, s.y2)
        ctx.lineTo(s.x2 - head * Math.cos(a - 0.5), s.y2 - head * Math.sin(a - 0.5))
        ctx.lineTo(s.x2 - head * Math.cos(a + 0.5), s.y2 - head * Math.sin(a + 0.5))
        ctx.closePath()
        ctx.fill()
      }
    }
    ctx.restore()
  }, [strokes, dpr])

  useEffect(() => {
    const canvas = canvasRef.current
    if (!canvas) return
    canvas.width = Math.round(window.innerWidth * dpr)
    canvas.height = Math.round(window.innerHeight * dpr)
    redraw()
  }, [dpr, redraw])

  // 自动淡出：标注停一会儿就消失，免得整段视频糊满箭头
  useEffect(() => {
    if (!strokes.length) return
    const id = window.setInterval(() => {
      const now = Date.now()
      setStrokes((list) => (list.some((s) => now - s.at > FADE_MS) ? list.filter((s) => now - s.at <= FADE_MS) : list))
    }, 500)
    return () => window.clearInterval(id)
  }, [strokes.length])

  const toLocal = (e: React.PointerEvent): { x: number; y: number } => {
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect()
    return { x: e.clientX - rect.left, y: e.clientY - rect.top }
  }

  const onDown = (e: React.PointerEvent) => {
    if (!anno) return
    e.currentTarget.setPointerCapture(e.pointerId)
    const p = toLocal(e)
    drawing.current = {
      id: seq.current++,
      tool,
      color,
      width: tool === 'highlight' ? 4 : 3,
      x1: p.x,
      y1: p.y,
      x2: p.x,
      y2: p.y,
      points: [p],
      at: Date.now()
    }
    redraw()
  }

  const onMove = (e: React.PointerEvent) => {
    if (!anno || !drawing.current) return
    const p = toLocal(e)
    const d = drawing.current
    if (d.tool === 'pen') d.points.push(p)
    else {
      d.x2 = p.x
      d.y2 = p.y
    }
    redraw()
  }

  const onUp = () => {
    if (!anno || !drawing.current) return
    const d = drawing.current
    drawing.current = null
    // 太小的笔画当误触丢掉
    const tiny =
      d.tool === 'pen'
        ? d.points.length < 3
        : Math.abs(d.x2 - d.x1) < 4 && Math.abs(d.y2 - d.y1) < 4
    if (!tiny) setStrokes((list) => [...list, d])
  }

  const exitAnno = useCallback(() => {
    setAnno(false)
    void api.recordSetInteractive(false)
  }, [])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && anno) {
        e.preventDefault()
        exitAnno()
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z' && anno) {
        e.preventDefault()
        setStrokes((list) => list.slice(0, -1))
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [anno, exitAnno])

  // 放大镜放光标上方，靠近顶部就翻到下方；都没空间就贴边
  const lensStyle = (() => {
    const p = lens ? { x: lens.x / dpr, y: lens.y / dpr } : null
    if (!p) return undefined
    const size = 240
    const gap = 26
    const above = p.y - size - gap
    const top = above > 8 ? above : p.y + gap
    const left = Math.min(Math.max(p.x - size / 2, 8), window.innerWidth - size - 8)
    return { width: size, height: size, left, top }
  })()

  return (
    <div className={`ov-root ${anno ? 'interactive' : ''}`}>
      {anno && (
        <canvas
          ref={canvasRef}
          className="ov-canvas"
          onPointerDown={onDown}
          onPointerMove={onMove}
          onPointerUp={onUp}
          onPointerCancel={onUp}
        />
      )}

      {/* 鼠标高亮环：光标进到录制区域里才画 */}
      {cur && recording && !anno && (
        <div
          className="ov-ring"
          style={{
            width: RING,
            height: RING,
            transform: `translate(${cur.x - RING / 2}px, ${cur.y - RING / 2}px)`
          }}
        />
      )}

      {ripples.map((r) => (
        <span key={r.id} className="ov-ripple" style={{ left: r.x, top: r.y }} />
      ))}

      {lens && lensStyle && (
        <div className="ov-lens" style={lensStyle}>
          <img src={lens.url} alt="" />
        </div>
      )}

      {anno && (
        <div className="ov-tools">
          {(['pen', 'arrow', 'rect', 'highlight'] as Tool[]).map((t) => (
            <button
              key={t}
              className={tool === t ? 'on' : ''}
              onClick={() => setTool(t)}
              title={{ pen: '画笔', arrow: '箭头', rect: '矩形', highlight: '荧光笔' }[t]}
            >
              {{ pen: '✎', arrow: '↗', rect: '▭', highlight: '▬' }[t]}
            </button>
          ))}
          <span className="ov-tool-sep" />
          {COLORS.map((c) => (
            <button
              key={c}
              className={`ov-color ${color === c ? 'on' : ''}`}
              style={{ background: c }}
              onClick={() => setColor(c)}
            />
          ))}
          <span className="ov-tool-sep" />
          <button onClick={() => setStrokes((l) => l.slice(0, -1))} title="撤销 (Ctrl+Z)">
            ↺
          </button>
          <button onClick={() => setStrokes([])} title="清空">
            ⌫
          </button>
          <span className="ov-tool-sep" />
          <button onClick={exitAnno} title="退出标注 (Esc)">
            完成
          </button>
        </div>
      )}
    </div>
  )
}

const container = document.getElementById('root')
if (container) {
  createRoot(container).render(<Overlay />)
}
