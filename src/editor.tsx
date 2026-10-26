import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { api, events } from './lib/api'
import type { CaptureResult, OcrResult } from './lib/types'
import { extOf, prepareWithSettings } from './lib/compress'
import {
  ASPECTS,
  BG_PRESETS,
  FRAMES,
  WATERMARK_COLORS,
  defaultFrame,
  type ArrowStyle,
  type FrameOptions,
  aspectValue,
  defaultBeautify,
  defaultWatermark,
  getParam,
  hitShapeEdge,
  newId,
  normalizeBox,
  shapeBounds,
  shapeParam,
  withParam,
  type BeautifyOptions,
  type Shape,
  type ToolId,
  type WatermarkOptions
} from './editor/model'
import {
  applyMosaic,
  composeFinal,
  drawHoverOutline,
  drawShapes,
  frameMetrics,
  frameOuterSize
} from './editor/render'
import {
  IconCheck,
  IconCopy,
  IconLoader,
  IconLayers,
  IconPin,
  IconSave,
  IconText,
  IconUpload,
  IconX,
  IconOcr,
  IconMinus
} from './components/Icons'
import './styles/editor.css'

const COLORS = ['#FF3B30', '#FF9500', '#FFCC00', '#34C759', '#0A84FF', '#6C5CE7', '#FFFFFF', '#1C1C1E']

const TOOLS: Array<{ id: ToolId; label: string; icon: React.ReactNode; key: string }> = [
  { id: 'rect', label: '矩形', icon: <BoxIcon />, key: 'R' },
  { id: 'ellipse', label: '椭圆', icon: <CircleIcon />, key: 'O' },
  { id: 'arrow', label: '箭头', icon: <ArrowIcon />, key: 'A' },
  { id: 'pen', label: '画笔', icon: <PenIcon />, key: 'P' },
  { id: 'marker', label: '高亮', icon: <MarkerIcon />, key: 'H' },
  { id: 'text', label: '文字', icon: <IconText size={16} />, key: 'T' },
  { id: 'number', label: '序号', icon: <NumberIcon />, key: 'N' },
  { id: 'mosaic', label: '马赛克', icon: <MosaicIcon />, key: 'M' }
]

export default function Editor() {
  const [capture, setCapture] = useState<CaptureResult | null>(null)
  const [shapes, setShapes] = useState<Shape[]>([])
  const [tool, setTool] = useState<ToolId>('rect')
  const [color, setColor] = useState(COLORS[0])
  const [strokeWidth, setStrokeWidth] = useState(3)
  const [fontSize, setFontSize] = useState(18)
  const [arrowStyle, setArrowStyle] = useState<ArrowStyle>('solid')
  /** 鼠标悬停的图形：滚轮会改它的参数 */
  const [hoverId, setHoverId] = useState<string | null>(null)
  const [hoverMeta, setHoverMeta] = useState('')
  const [drawing, setDrawing] = useState<Shape | null>(null)
  const [beautify, setBeautify] = useState<BeautifyOptions>(defaultBeautify())
  const [watermark, setWatermark] = useState<WatermarkOptions>(defaultWatermark())
  const [frame, setFrame] = useState<FrameOptions>(defaultFrame())
  const [ocr, setOcr] = useState<OcrResult | null>(null)
  const [busy, setBusy] = useState<string | null>(null)
  /** 导出失败的提示，直接显示在界面上 */
  const [error, setError] = useState('')
  const [textInput, setTextInput] = useState<{ x: number; y: number; value: string } | null>(null)
  const [numberSeq, setNumberSeq] = useState(1)
  /** 相对"适配窗口"的缩放倍率。1 = 刚好完整显示 */
  const [zoom, setZoom] = useState(1)

  const canvasRef = useRef<HTMLCanvasElement>(null)
  const stageRef = useRef<HTMLDivElement>(null)
  const artRef = useRef<HTMLDivElement>(null)
  const imageRef = useRef<HTMLImageElement | null>(null)
  const undoStack = useRef<Shape[][]>([])
  const redoStack = useRef<Shape[][]>([])
  const dragStart = useRef<{ x: number; y: number } | null>(null)
  /** 滚轮缩放时，光标下的那个点在画板里的相对位置（0-1），缩放后要把它钉回原处 */
  const zoomAnchor = useRef<{ fx: number; fy: number; cx: number; cy: number } | null>(null)
  const panRef = useRef<{ x: number; y: number; sl: number; st: number } | null>(null)
  const [spaceDown, setSpaceDown] = useState(false)
  /** 原生 wheel 监听闭包拿不到最新的 state，用 ref 同步一份 */
  const shapesRef = useRef<Shape[]>([])
  const hoverRef = useRef<string | null>(null)
  const scaleRef = useRef(1)
  /** 连续调节同一个图形只记一次撤销 */
  const adjustSession = useRef<string | null>(null)
  const adjustTimer = useRef<number | undefined>(undefined)
  const win = getCurrentWindow()

  /* ---------------- 预览布局 ---------------- */

  // 画布显示尺寸会随窗口变化，美化参数也得跟着换算成 CSS 像素才能所见即所得
  const [stageSize, setStageSize] = useState({ w: 0, h: 0 })
  useEffect(() => {
    const el = stageRef.current
    if (!el) return
    const update = () => setStageSize({ w: el.clientWidth, h: el.clientHeight })
    update()
    const ro = new ResizeObserver(update)
    ro.observe(el)
    return () => ro.disconnect()
  }, [])

  /**
   * 把"原图像素"换算成"屏幕 CSS 像素"。
   *
   * 和 composeFinal 用同一套尺寸公式，保证预览与导出结果一致：
   *   输出尺寸 = 原图 + 内边距 → 按比例扩边 → 整体等比缩放到可用空间内
   */
  const layout = useMemo(() => {
    if (!capture || stageSize.w === 0) return null
    const pad = beautify.enabled ? beautify.padding : 0
    // 外壳先占位，内边距在它外面 —— 顺序和 composeFinal 保持一致
    const outer = frameOuterSize(capture.width, capture.height, frame)
    let outW = outer.width + pad * 2
    let outH = outer.height + pad * 2
    const ratio = beautify.enabled ? aspectValue(beautify.aspect) : null
    if (ratio) {
      if (outW / outH < ratio) outW = Math.round(outH * ratio)
      else outH = Math.round(outW / ratio)
    }
    const availW = Math.max(80, stageSize.w - 56)
    const availH = Math.max(80, stageSize.h - 56)
    // baseScale = 刚好塞进窗口的缩放；zoom 是用户在上面滚出来的倍率。
    // 所有尺寸都从最终的 scale 推出来，滚轮一滚，画布、内边距、外壳、
    // 水印字号会一起缩放——否则放大后边框和水印会错位。
    const baseScale = Math.min(availW / outW, availH / outH, 1)
    const scale = baseScale * zoom
    return {
      scale,
      baseScale,
      canvasW: capture.width * scale,
      canvasH: capture.height * scale,
      padX: ((outW - outer.width) / 2) * scale,
      padY: ((outH - outer.height) / 2) * scale,
      radius: beautify.enabled ? beautify.radius * scale : 0,
      shellW: outer.width * scale,
      shellH: outer.height * scale,
      metrics: outer.metrics
    }
  }, [
    capture,
    beautify.enabled,
    beautify.padding,
    beautify.radius,
    beautify.aspect,
    frame,
    stageSize,
    zoom
  ])

  /* ---------------- 缩放 / 平移 ---------------- */

  // 原生 wheel 监听只注册一次，闭包里拿不到最新的 state，靠 ref 同步
  shapesRef.current = shapes
  scaleRef.current = layout?.scale ?? 1

  const ZOOM_MIN = 0.2
  const ZOOM_MAX = 8
  const clampZoom = (z: number) => Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, z))

  /** 以舞台中心为锚点按步长缩放（给按钮和快捷键用） */
  const zoomStep = (dir: number) => {
    const stage = stageRef.current
    const art = artRef.current
    if (stage && art) {
      const sr = stage.getBoundingClientRect()
      const r = art.getBoundingClientRect()
      const cx = sr.left + sr.width / 2
      const cy = sr.top + sr.height / 2
      zoomAnchor.current = {
        fx: (cx - r.left) / Math.max(1, r.width),
        fy: (cy - r.top) / Math.max(1, r.height),
        cx,
        cy
      }
    }
    setZoom((z) => clampZoom(z * (dir > 0 ? 1.25 : 0.8)))
  }

  /** 切成 1:1 实际像素 */
  const actualSize = () => {
    const base = layout?.baseScale ?? 1
    setZoom(clampZoom(1 / base))
  }

  // 滚轮：压在标注上 = 调它的参数（粗细/字号/…），空白处 = 缩放画布。
  //
  // 必须用非 passive 的原生监听：React 的 onWheel 是 passive 的，
  // preventDefault 拦不住，一滚页面就会跟着滚。
  useEffect(() => {
    const stage = stageRef.current
    if (!stage) return
    const onWheel = (e: WheelEvent) => {
      if (!imageRef.current) return
      e.preventDefault()

      // ---- 1. 命中标注 → 调参数 ----
      const hovered = hoverRef.current
        ? shapesRef.current.find((s) => s.id === hoverRef.current)
        : null
      if (hovered) {
        const param = shapeParam(hovered)
        const dir = e.deltaY < 0 ? 1 : -1
        const next = withParam(hovered, param, getParam(hovered, param) + dir * param.step)

        // 撤销分组：一次连续调节算一步，否则滚十下要按十次撤销
        if (adjustSession.current !== hovered.id) {
          undoStack.current.push(shapesRef.current)
          if (undoStack.current.length > 60) undoStack.current.shift()
          redoStack.current = []
          adjustSession.current = hovered.id
        }
        window.clearTimeout(adjustTimer.current)
        adjustTimer.current = window.setTimeout(() => {
          adjustSession.current = null
        }, 900)

        const updated = shapesRef.current.map((s) => (s.id === hovered.id ? next : s))
        shapesRef.current = updated
        setShapes(updated)
        const v = getParam(next, param)
        setHoverMeta(
          `${param.label} ${param.percent ? Math.round(v * 100) + '%' : Math.round(v)}`
        )
        return
      }

      // ---- 2. 空白处 → 缩放 ----
      const art = artRef.current
      if (art) {
        const r = art.getBoundingClientRect()
        zoomAnchor.current = {
          fx: (e.clientX - r.left) / Math.max(1, r.width),
          fy: (e.clientY - r.top) / Math.max(1, r.height),
          cx: e.clientX,
          cy: e.clientY
        }
      }
      // 指数缩放：触控板的 deltaY 忽大忽小，线性加减会一顿一顿的
      const factor = Math.exp(-e.deltaY * 0.0016)
      setZoom((z) => clampZoom(z * factor))
    }
    stage.addEventListener('wheel', onWheel, { passive: false })
    return () => stage.removeEventListener('wheel', onWheel)
  }, [])

  // 缩放后把光标下的那个点钉回原处。
  // 少了这一步，滚轮会变成"从左上角放大"，用户想看清的内容会越滚越远。
  useEffect(() => {
    const a = zoomAnchor.current
    const art = artRef.current
    const stage = stageRef.current
    if (!a || !art || !stage) return
    zoomAnchor.current = null
    const r = art.getBoundingClientRect()
    stage.scrollLeft += r.left + a.fx * r.width - a.cx
    stage.scrollTop += r.top + a.fy * r.height - a.cy
  }, [zoom, layout])

  // 空格按住 = 抓手（和 PS / Figma 一致，画大图时比拖滚动条顺手）
  useEffect(() => {
    const down = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement
      if (t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA')) return
      if (e.code === 'Space') setSpaceDown(true)
    }
    const up = (e: KeyboardEvent) => {
      if (e.code === 'Space') setSpaceDown(false)
    }
    window.addEventListener('keydown', down)
    window.addEventListener('keyup', up)
    return () => {
      window.removeEventListener('keydown', down)
      window.removeEventListener('keyup', up)
    }
  }, [])

  const stageBackdrop = useMemo(() => {
    if (!beautify.enabled || beautify.bgMode === 'transparent') return 'transparent'
    if (beautify.bgMode === 'gradient') {
      return `linear-gradient(135deg, ${beautify.gradientFrom}, ${beautify.gradientTo})`
    }
    return beautify.background
  }, [beautify])

  const stageShadow = useMemo(() => {
    if (!beautify.enabled || beautify.shadow <= 0) return 'none'
    const s = beautify.shadow / 100
    const scale = layout?.scale ?? 1
    return `0 ${Math.round(10 * s * scale * 2)}px ${Math.round(40 * s * scale * 2)}px rgba(0,0,0,${(
      0.18 * s + 0.06
    ).toFixed(3)})`
  }, [beautify.enabled, beautify.shadow, layout])

  /* ---------------- 载入截图 ---------------- */

  useEffect(() => {
    const load = async (cap: CaptureResult) => {
      const img = new Image()
      await new Promise<void>((resolve, reject) => {
        img.onload = () => resolve()
        img.onerror = reject
        img.src = cap.dataUrl
      })
      imageRef.current = img
      setCapture(cap)
      setShapes([])
      setZoom(1)
      undoStack.current = []
      redoStack.current = []
      const canvas = canvasRef.current
      if (canvas) {
        canvas.width = img.naturalWidth
        canvas.height = img.naturalHeight
        const ctx = canvas.getContext('2d')!
        ctx.drawImage(img, 0, 0)
      }
    }

    void api.capturePending().then((cap) => {
      if (cap) void load(cap)
    })
    const unsub = events.onEditorCapture((cap) => void load(cap))
    return () => {
      void unsub.then((u) => u())
    }
  }, [])

  /* ---------------- 重绘 ---------------- */

  const redraw = useCallback(() => {
    const canvas = canvasRef.current
    const img = imageRef.current
    if (!canvas || !img) return
    const ctx = canvas.getContext('2d')!
    ctx.clearRect(0, 0, canvas.width, canvas.height)
    ctx.drawImage(img, 0, 0)

    for (const shape of shapes) {
      if (shape.type === 'mosaic') {
        applyMosaic(ctx, canvas, shape.x, shape.y, shape.width, shape.height, shape.blockSize ?? 12)
      } else {
        drawShapes(ctx, [shape])
      }
    }
    if (drawing) {
      if (drawing.type !== 'mosaic') drawShapes(ctx, [drawing])
    }

    // 悬停高亮：虚线框住整个图形，告诉用户"滚轮会改这个"
    const hovered = hoverId ? shapes.find((s) => s.id === hoverId) : null
    if (hovered) {
      drawHoverOutline(ctx, shapeBounds(hovered), 1.6 / Math.max(0.2, scaleRef.current))
    }
  }, [shapes, drawing, hoverId])

  useEffect(() => {
    redraw()
  }, [redraw])

  /* ---------------- 撤销/重做 ---------------- */

  const commitShapes = (next: Shape[]) => {
    undoStack.current.push(shapes)
    if (undoStack.current.length > 60) undoStack.current.shift()
    redoStack.current = []
    setShapes(next)
  }

  const undo = () => {
    const prev = undoStack.current.pop()
    if (prev) {
      redoStack.current.push(shapes)
      setShapes(prev)
    }
  }
  const redo = () => {
    const next = redoStack.current.pop()
    if (next) {
      undoStack.current.push(shapes)
      setShapes(next)
    }
  }

  /* ---------------- 鼠标交互 ---------------- */

  const toCanvas = (e: React.MouseEvent) => {
    const canvas = canvasRef.current!
    const rect = canvas.getBoundingClientRect()
    return {
      x: ((e.clientX - rect.left) / rect.width) * canvas.width,
      y: ((e.clientY - rect.top) / rect.height) * canvas.height
    }
  }

  const onDown = (e: React.MouseEvent) => {
    if (!capture || e.button !== 0) return
    // 空格 / 中键是平移手势，不能同时开始画
    if (spaceDown || panRef.current) return
    // 开始画新图形时清掉悬停态，否则旧图形的高亮会一直挂在那儿
    if (hoverRef.current) syncHover(null)
    const p = toCanvas(e)
    dragStart.current = p

    if (tool === 'text') {
      setTextInput({ x: p.x, y: p.y, value: '' })
      return
    }
    if (tool === 'number') {
      commitShapes([
        ...shapes,
        { id: newId(), type: 'number', x: p.x, y: p.y, index: numberSeq, radius: 14, color, strokeWidth }
      ])
      setNumberSeq((n) => n + 1)
      return
    }

    if (tool === 'rect' || tool === 'ellipse' || tool === 'mosaic' || tool === 'marker') {
      setDrawing({ id: newId(), type: tool, x: p.x, y: p.y, width: 0, height: 0, color, strokeWidth })
    } else if (tool === 'arrow') {
      setDrawing({
        id: newId(),
        type: 'arrow',
        x1: p.x,
        y1: p.y,
        x2: p.x,
        y2: p.y,
        color,
        strokeWidth,
        style: arrowStyle
      })
    } else if (tool === 'pen') {
      setDrawing({ id: newId(), type: 'pen', points: [p], color, strokeWidth })
    }
  }

  /** 鼠标下是哪个图形。后画的在上面，所以倒着找。 */
  const shapeAt = (x: number, y: number): Shape | null => {
    // 容差换算成图片像素：放大到 300% 时，屏幕上 8px 只对应画布 2-3px
    const tol = Math.max(3, 8 / Math.max(0.2, scaleRef.current))
    for (let i = shapes.length - 1; i >= 0; i--) {
      if (hitShapeEdge(shapes[i], x, y, tol)) return shapes[i]
    }
    return null
  }

  const syncHover = (hit: Shape | null) => {
    const id = hit?.id ?? null
    if (id === hoverRef.current) {
      // 同一个图形，参数可能刚被滚轮改过，刷新提示文字
      if (hit) {
        const param = shapeParam(hit)
        const v = getParam(hit, param)
        setHoverMeta(`${param.label} ${param.percent ? Math.round(v * 100) + '%' : Math.round(v)}`)
      }
      return
    }
    hoverRef.current = id
    setHoverId(id)
    adjustSession.current = null
    if (hit) {
      const param = shapeParam(hit)
      const v = getParam(hit, param)
      setHoverMeta(`${param.label} ${param.percent ? Math.round(v * 100) + '%' : Math.round(v)}`)
    } else {
      setHoverMeta('')
    }
  }

  const onMove = (e: React.MouseEvent) => {
    if (dragStart.current && drawing) {
      const p = toCanvas(e)
      if (drawing.type === 'arrow') {
        setDrawing({ ...drawing, x2: p.x, y2: p.y })
      } else if (drawing.type === 'pen') {
        setDrawing({ ...drawing, points: [...drawing.points, p] })
      } else if ('width' in drawing) {
        const box = normalizeBox(dragStart.current, p)
        setDrawing({ ...drawing, ...box } as Shape)
      }
      return
    }

    // 没在画：更新悬停目标（滚轮要改它）
    if (!capture || panRef.current) return
    const p = toCanvas(e)
    syncHover(shapeAt(p.x, p.y))
  }

  const onUp = () => {
    if (drawing) {
      // 太小的框视为误触，不留垃圾
      const isBox = drawing.type === 'rect' || drawing.type === 'ellipse' ||
        drawing.type === 'mosaic' || drawing.type === 'marker'
      const tooSmall =
        isBox && (drawing as { width: number; height: number }).width < 3 &&
        (drawing as { width: number; height: number }).height < 3
      if (!tooSmall) commitShapes([...shapes, drawing])
      setDrawing(null)
    }
    dragStart.current = null
  }

  const submitText = () => {
    if (textInput && textInput.value.trim()) {
      commitShapes([
        ...shapes,
        {
          id: newId(),
          type: 'text',
          x: textInput.x,
          y: textInput.y,
          text: textInput.value,
          fontSize,
          color,
          strokeWidth
        }
      ])
    }
    setTextInput(null)
  }

  /* ---------------- 键盘 ---------------- */

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (textInput) return
      const target = e.target as HTMLElement
      if (target.tagName === 'INPUT' || target.tagName === 'TEXTAREA') return

      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') {
        e.preventDefault()
        if (e.shiftKey) redo()
        else undo()
        return
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'c') {
        e.preventDefault()
        void exportCopy()
        return
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
        e.preventDefault()
        void exportSave()
        return
      }
      // 缩放：Ctrl+= / Ctrl+- / Ctrl+0（回适配）
      if ((e.ctrlKey || e.metaKey) && (e.key === '=' || e.key === '+')) {
        e.preventDefault()
        zoomStep(1)
        return
      }
      if ((e.ctrlKey || e.metaKey) && e.key === '-') {
        e.preventDefault()
        zoomStep(-1)
        return
      }
      if ((e.ctrlKey || e.metaKey) && e.key === '0') {
        e.preventDefault()
        setZoom(1)
        return
      }
      // F3 钉图，和 Snipaste 的键位一致
      if (e.key === 'F3') {
        e.preventDefault()
        void exportPin()
        return
      }
      if (e.key === 'Escape') {
        e.preventDefault()
        void api.closeEditor()
        return
      }
      const t = TOOLS.find((x) => x.key.toLowerCase() === e.key.toLowerCase())
      if (t) setTool(t.id)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [shapes, textInput, beautify, watermark, frame, capture])

  /* ---------------- 导出 ---------------- */

  /**
   * 出错了要说出来。
   *
   * 生产包里看不到 devtools，alert 在 WebView 里也不可靠 ——
   * 用户只会看到「点了没反应」。所以同时做三件事：
   * 屏幕上有提示、日志文件里有记录、按钮状态能恢复。
   */
  const fail = (where: string, e: unknown) => {
    const msg = e instanceof Error ? e.message : String(e)
    setError(`${where}：${msg}`)
    void api.log('error', `${where}: ${msg}`)
    window.setTimeout(() => setError(''), 8000)
  }

  const buildFinal = useCallback((): string | null => {
    const canvas = canvasRef.current
    if (!canvas) {
      fail('导出', '画布还没准备好')
      return null
    }
    try {
      const out = composeFinal(canvas, beautify, watermark, frame)
      return out.toDataURL('image/png')
    } catch (e) {
      fail('合成图片', e)
      return null
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [beautify, watermark, frame])

  const exportCopy = async () => {
    const dataUrl = buildFinal()
    if (!dataUrl) return
    setBusy('copy')
    try {
      await api.imageCopy(dataUrl)
      await api.closeEditor()
    } catch (e) {
      fail('复制失败', e)
    } finally {
      setBusy(null)
    }
  }

  const exportPin = async () => {
    const dataUrl = buildFinal()
    if (!dataUrl) return
    setBusy('pin')
    try {
      await api.imagePin(dataUrl)
      // 钉在屏幕上之后编辑器就没事了，收起来免得两个窗口叠一起
      await api.closeEditor()
    } catch (e) {
      fail('钉图失败', e)
    } finally {
      setBusy(null)
    }
  }

  const exportSave = async (as = false) => {
    const dataUrl = buildFinal()
    if (!dataUrl) return
    setBusy('save')
    try {
      if (as) await api.imageSaveAs(dataUrl)
      else await api.imageSave(dataUrl)
      await api.closeEditor()
    } catch (e) {
      fail('保存失败', e)
    } finally {
      setBusy(null)
    }
  }

  const exportUpload = async () => {
    const dataUrl = buildFinal()
    if (!dataUrl) return
    setBusy('upload')
    try {
      const p = await prepareWithSettings(dataUrl)
      const out = await api.uploadImage(p.dataUrl, undefined, `image.${extOf(p.format)}`)
      await api.clipboardCopyText(out.result.url)
      setError(`上传成功，链接已复制：${out.result.url}`)
      window.setTimeout(() => setError(''), 8000)
    } catch (e) {
      fail('上传失败', e)
    } finally {
      setBusy(null)
    }
  }

  const runOcr = async () => {
    const dataUrl = buildFinal()
    if (!dataUrl) return
    setBusy('ocr')
    try {
      setOcr(await api.imageOcr(dataUrl))
    } catch (e) {
      fail('识别失败', e)
    } finally {
      setBusy(null)
    }
  }

  const fm = layout?.metrics ?? { top: 0, side: 0, bottom: 0, radius: 0 }
  const sc = layout?.scale ?? 1
  const dotR = Math.max(3, fm.top * 0.18 * sc)
  const hasBar =
    frame.id === 'mac' || frame.id === 'windows' || frame.id === 'browser'

  return (
    <div className="editor-root">
      <div className="editor-topbar">
        <div className="drag" data-tauri-drag-region>
          <span style={{ fontSize: 12.5, fontWeight: 600, color: '#a1a1aa', paddingLeft: 6 }}>
            {capture ? `${capture.width} × ${capture.height}` : '编辑器'}
          </span>
        </div>
        <button
          className="btn sm"
          style={{ background: '#2c2c30', border: 'none', color: '#e4e4e7' }}
          onClick={undo}
          disabled={undoStack.current.length === 0}
        >
          撤销
        </button>
        <button
          className="btn sm"
          style={{ background: '#2c2c30', border: 'none', color: '#e4e4e7' }}
          onClick={redo}
          disabled={redoStack.current.length === 0}
        >
          重做
        </button>
        <div style={{ width: 8 }} />
        <button className="btn sm" style={{ background: '#2c2c30', border: 'none', color: '#e4e4e7' }} onClick={() => void runOcr()}>
          {busy === 'ocr' ? <IconLoader /> : <IconOcr size={14} />} 取字
        </button>
        <button className="btn sm" style={{ background: '#2c2c30', border: 'none', color: '#e4e4e7' }} onClick={() => void exportUpload()}>
          {busy === 'upload' ? <IconLoader /> : <IconUpload size={14} />} 上传
        </button>
        <button className="btn sm" style={{ background: '#2c2c30', border: 'none', color: '#e4e4e7' }} onClick={() => void exportSave(true)}>
          <IconSave size={14} /> 另存
        </button>
        <button className="btn sm" style={{ background: '#2c2c30', border: 'none', color: '#e4e4e7' }} onClick={() => void exportSave(false)}>
          {busy === 'save' ? <IconLoader /> : <IconSave size={14} />} 保存
        </button>
        <button
          className="btn sm"
          style={{ background: '#2c2c30', border: 'none', color: '#e4e4e7' }}
          title="钉在屏幕最上层当参考 (F3)"
          onClick={() => void exportPin()}
        >
          {busy === 'pin' ? <IconLoader /> : <IconPin size={14} />} 钉图
        </button>
        <button className="btn sm primary" onClick={() => void exportCopy()}>
          {busy === 'copy' ? <IconLoader /> : <IconCopy size={14} />} 复制
        </button>
        <button

          className="btn sm"
          style={{ background: 'transparent', border: 'none', color: '#a1a1aa', width: 32 }}
          onClick={() => void api.closeEditor()}
        >
          <IconX />
        </button>
      </div>

      {error && (
        <div className="editor-error" onClick={() => setError('')} title="点击关闭">
          {error}
        </div>
      )}

      <div className="editor-body">
        <div className="editor-tools">
          {TOOLS.map((t) => (
            <button
              key={t.id}
              className={`tool-btn ${tool === t.id ? 'active' : ''}`}
              title={`${t.label} (${t.key})`}
              onClick={() => setTool(t.id)}
            >
              {t.icon}
            </button>
          ))}
        </div>

        <div className="stage-wrap">
          <div
            className={`editor-stage ${spaceDown ? 'is-panning' : ''}`}
            ref={stageRef}
            onMouseDown={(e) => {
              // 中键 / 空格+左键 = 平移。放大到 300% 之后靠拖滚动条太难受了
              if (e.button === 1 || (e.button === 0 && spaceDown)) {
                e.preventDefault()
                panRef.current = {
                  x: e.clientX,
                  y: e.clientY,
                  sl: e.currentTarget.scrollLeft,
                  st: e.currentTarget.scrollTop
                }
              }
            }}
            onMouseMove={(e) => {
              const p = panRef.current
              if (!p) return
              e.currentTarget.scrollLeft = p.sl - (e.clientX - p.x)
              e.currentTarget.scrollTop = p.st - (e.clientY - p.y)
            }}
            onMouseUp={() => {
              panRef.current = null
            }}
            onMouseLeave={() => {
              panRef.current = null
            }}
          >
          <div
            className="stage-artboard"
            ref={artRef}
            style={{
              paddingLeft: layout?.padX ?? 0,
              paddingRight: layout?.padX ?? 0,
              paddingTop: layout?.padY ?? 0,
              paddingBottom: layout?.padY ?? 0,
              background: stageBackdrop,
              boxShadow: stageShadow,
              borderRadius: layout?.radius ?? 0
            }}
          >
            {/* 外壳：和 composeFinal 里 drawFrame 的结构一一对应，保证预览=导出 */}
            <div
              className="frame-shell"
              style={{
                width: layout?.shellW ?? undefined,
                background:
                  frame.id === 'none' ? 'transparent' : frame.dark ? '#33333a' : '#f4f4f7',
                borderRadius: (fm.radius ?? 0) * sc,
                display: 'flex',
                flexDirection: 'column',
                position: 'relative'
              }}
            >
              {hasBar && (
                <div
                  className="frame-bar"
                  style={{
                    height: fm.top * sc,
                    flexShrink: 0,
                    display: 'flex',
                    alignItems: 'center',
                    paddingLeft: (frame.id === 'windows' ? 14 : 16) * sc,
                    paddingRight: 10 * sc,
                    gap: 6 * sc
                  }}
                >
                  {(frame.id === 'mac' || frame.id === 'browser') && (
                    <div style={{ display: 'flex', gap: dotR * 1.1 }}>
                      {['#FF5F57', '#FEBC2E', '#28C840'].map((c) => (
                        <span
                          key={c}
                          style={{
                            width: dotR * 2,
                            height: dotR * 2,
                            borderRadius: '50%',
                            background: c,
                            display: 'block'
                          }}
                        />
                      ))}
                    </div>
                  )}

                  {frame.id === 'windows' && (
                    <>
                      <span
                        style={{
                          fontSize: Math.max(8, fm.top * 0.34 * sc),
                          color: frame.dark ? 'rgba(255,255,255,0.72)' : 'rgba(0,0,0,0.62)',
                          whiteSpace: 'nowrap',
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          flex: 1
                        }}
                      >
                        {frame.title}
                      </span>
                      <div
                        style={{
                          display: 'flex',
                          gap: 4 * sc,
                          color: frame.dark ? 'rgba(255,255,255,0.72)' : 'rgba(0,0,0,0.62)',
                          fontSize: Math.max(9, fm.top * 0.32 * sc)
                        }}
                      >
                        <span>—</span>
                        <span>▢</span>
                        <span>✕</span>
                      </div>
                    </>
                  )}

                  {frame.id === 'browser' && (
                    <span
                      style={{
                        flex: 1,
                        height: fm.top * 0.5 * sc,
                        borderRadius: (fm.top * 0.25 * sc),
                        background: frame.dark ? 'rgba(255,255,255,0.09)' : 'rgba(0,0,0,0.05)',
                        border: `1px solid ${
                          frame.dark ? 'rgba(255,255,255,0.12)' : 'rgba(0,0,0,0.08)'
                        }`,
                        display: 'flex',
                        alignItems: 'center',
                        justifyContent: 'center',
                        fontSize: Math.max(8, fm.top * 0.3 * sc),
                        color: frame.dark ? 'rgba(255,255,255,0.72)' : 'rgba(0,0,0,0.62)',
                        overflow: 'hidden',
                        whiteSpace: 'nowrap'
                      }}
                    >
                      {frame.title}
                    </span>
                  )}
                </div>
              )}

              <div
                style={{
                  flex: 1,
                  display: 'flex',
                  justifyContent: 'center',
                  paddingLeft: fm.side * sc,
                  paddingRight: fm.side * sc,
                  minHeight: 0
                }}
              >
                <div
                  className="stage-canvas-holder"
                  style={{
                    width: layout?.canvasW ?? 'auto',
                    height: layout?.canvasH ?? 'auto',
                    borderRadius:
                      frame.id === 'none'
                        ? (layout?.radius ?? 0)
                        : Math.max(2, (fm.radius - 6) * sc)
                  }}
                >
                  <canvas
                    ref={canvasRef}
                    onMouseDown={onDown}
                    onMouseMove={onMove}
                    onMouseUp={onUp}
                    onMouseLeave={onUp}
                    style={{
                      width: '100%',
                      height: '100%',
                      display: 'block',
                      cursor: hoverId
                        ? 'ns-resize'
                        : tool === 'text'
                          ? 'text'
                          : 'crosshair'
                    }}
                  />

                  {watermark.enabled && watermark.text.trim() && layout && (
                    <div
                      className="wm-layer"
                      style={{
                        opacity: watermark.opacity,
                        fontSize: Math.max(7, watermark.fontSize * layout.scale),
                        color: watermark.color
                      }}
                    >
                      {watermark.tiled ? (
                        <div className="wm-tiled-wrap">
                          {Array.from({ length: 80 }).map((_, i) => (
                            <span key={i} className="wm-tile">
                              {watermark.text}
                            </span>
                          ))}
                        </div>
                      ) : (
                        <span className="wm-single">{watermark.text}</span>
                      )}
                    </div>
                  )}
                </div>
              </div>

              {(frame.id === 'laptop' || frame.id === 'phone') && (
                <div
                  style={{
                    height: fm.bottom * sc,
                    flexShrink: 0,
                    display: 'flex',
                    alignItems: 'center',
                    justifyContent: 'center'
                  }}
                >
                  {frame.id === 'phone' && (
                    <span
                      style={{
                        width: (layout?.shellW ?? 200) * 0.32,
                        height: Math.max(2, fm.bottom * 0.12 * sc),
                        borderRadius: fm.bottom * 0.06 * sc,
                        background: frame.dark ? 'rgba(255,255,255,0.55)' : 'rgba(0,0,0,0.35)'
                      }}
                    />
                  )}
                </div>
              )}
            </div>
          </div>
          </div>

          {textInput && (
            <input
              autoFocus
              className="dark-input"
              style={{ position: 'absolute', left: 40, bottom: 20, width: 260 }}
              placeholder="输入文字后回车"
              value={textInput.value}
              onChange={(e) => setTextInput({ ...textInput, value: e.target.value })}
              onKeyDown={(e) => {
                if (e.key === 'Enter') submitText()
                if (e.key === 'Escape') setTextInput(null)
              }}
              onBlur={submitText}
            />
          )}

          {capture && layout && (
            <div className="zoom-bar" onMouseDown={(e) => e.stopPropagation()}>
              <button onClick={() => zoomStep(-1)} title="缩小 (Ctrl -)">
                <IconMinus size={13} />
              </button>
              <button
                className="zoom-val"
                title="切成 1:1 实际像素"
                onClick={actualSize}
              >
                {Math.round(layout.scale * 100)}%
              </button>
              <button onClick={() => zoomStep(1)} title="放大 (Ctrl +)">
                +
              </button>
              <span className="zoom-sep" />
              <button
                className={zoom === 1 ? 'zoom-fit active' : 'zoom-fit'}
                onClick={() => setZoom(1)}
                title="适配窗口 (Ctrl 0)"
              >
                适配
              </button>
            </div>
          )}

          {hoverId && hoverMeta && !drawing && (
            <div className="shape-hint">滚轮调整 · {hoverMeta}</div>
          )}
        </div>

        <div className="editor-panel">
          <div className="panel-group">
            <div className="title">颜色</div>
            <div className="swatches">
              {COLORS.map((c) => (
                <button
                  key={c}
                  className={`swatch ${color === c ? 'active' : ''}`}
                  style={{ background: c }}
                  onClick={() => setColor(c)}
                />
              ))}
            </div>
          </div>

          <div className="panel-group">
            <div className="title">粗细 · {strokeWidth}px</div>
            <input
              type="range"
              className="slider"
              min={1}
              max={14}
              value={strokeWidth}
              onChange={(e) => setStrokeWidth(Number(e.target.value))}
            />
          </div>

          {tool === 'arrow' && (
            <div className="panel-group">
              <div className="title">箭头样式</div>
              <div className="seg" style={{ background: '#2c2c30' }}>
                {(
                  [
                    { id: 'solid', label: '实心箭头' },
                    { id: 'tapered', label: '锥尾箭头' }
                  ] as Array<{ id: ArrowStyle; label: string }>
                ).map((a) => (
                  <button
                    key={a.id}
                    className={arrowStyle === a.id ? 'active' : ''}
                    style={{
                      flex: 1,
                      height: 24,
                      fontSize: 11,
                      background: arrowStyle === a.id ? '#6c5ce7' : 'transparent',
                      color: arrowStyle === a.id ? '#fff' : '#a1a1aa'
                    }}
                    onClick={() => setArrowStyle(a.id)}
                  >
                    {a.label}
                  </button>
                ))}
              </div>
              <div style={{ fontSize: 10.5, color: '#71717a', marginTop: 4, lineHeight: 1.6 }}>
                锥尾：细长锥身 + 宽大头，肩部横出成燕尾（按参考 SVG 复刻）
              </div>
            </div>
          )}

          {tool === 'text' && (
            <div className="panel-group">
              <div className="title">字号 · {fontSize}px</div>
              <input
                type="range"
                className="slider"
                min={12}
                max={64}
                value={fontSize}
                onChange={(e) => setFontSize(Number(e.target.value))}
              />
            </div>
          )}

          <div className="panel-group">
            <div
              className="title"
              style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}
            >
              <span>美化</span>
              <button
                className={`switch ${beautify.enabled ? 'on' : ''}`}
                style={{ transform: 'scale(0.82)' }}
                onClick={() => setBeautify({ ...beautify, enabled: !beautify.enabled })}
              />
            </div>

            {beautify.enabled && (
              <>
                <div className="title" style={{ marginTop: 2 }}>
                  内边距 · {beautify.padding}px
                </div>
                <input
                  type="range"
                  className="slider"
                  min={0}
                  max={160}
                  value={beautify.padding}
                  onChange={(e) => setBeautify({ ...beautify, padding: Number(e.target.value) })}
                />

                <div className="title" style={{ marginTop: 2 }}>
                  圆角 · {beautify.radius}px
                </div>
                <input
                  type="range"
                  className="slider"
                  min={0}
                  max={60}
                  value={beautify.radius}
                  onChange={(e) => setBeautify({ ...beautify, radius: Number(e.target.value) })}
                />

                <div className="title" style={{ marginTop: 2 }}>
                  阴影 · {beautify.shadow}
                </div>
                <input
                  type="range"
                  className="slider"
                  min={0}
                  max={100}
                  value={beautify.shadow}
                  onChange={(e) => setBeautify({ ...beautify, shadow: Number(e.target.value) })}
                />

                <div className="title" style={{ marginTop: 6 }}>
                  背景
                </div>
                <div className="swatches">
                  {BG_PRESETS.map((p) => (
                    <button
                      key={p.id}
                      title={p.label}
                      className="swatch"
                      style={{
                        background: `linear-gradient(135deg, ${p.from}, ${p.to})`,
                        width: 26,
                        height: 26,
                        borderColor:
                          beautify.bgMode === 'gradient' &&
                          beautify.gradientFrom === p.from &&
                          beautify.gradientTo === p.to
                            ? '#fff'
                            : 'transparent'
                      }}
                      onClick={() =>
                        setBeautify({
                          ...beautify,
                          bgMode: 'gradient',
                          gradientFrom: p.from,
                          gradientTo: p.to
                        })
                      }
                    />
                  ))}
                  <button
                    title="纯色"
                    className={`swatch ${beautify.bgMode === 'solid' ? 'active' : ''}`}
                    style={{ background: beautify.background, width: 26, height: 26 }}
                    onClick={() => setBeautify({ ...beautify, bgMode: 'solid' })}
                  />
                  <label
                    title="透明"
                    className={`swatch ${beautify.bgMode === 'transparent' ? 'active' : ''}`}
                    style={{
                      width: 26,
                      height: 26,
                      cursor: 'pointer',
                      backgroundImage:
                        'linear-gradient(45deg,#3a3a40 25%,transparent 25%),linear-gradient(-45deg,#3a3a40 25%,transparent 25%),linear-gradient(45deg,transparent 75%,#3a3a40 75%),linear-gradient(-45deg,transparent 75%,#3a3a40 75%)',
                      backgroundSize: '8px 8px',
                      backgroundPosition: '0 0,0 4px,4px -4px,-4px 0'
                    }}
                    onClick={() => setBeautify({ ...beautify, bgMode: 'transparent' })}
                  >
                    <input
                      type="color"
                      value={beautify.background}
                      onChange={(e) =>
                        setBeautify({ ...beautify, background: e.target.value, bgMode: 'solid' })
                      }
                      style={{ opacity: 0, width: 0, height: 0 }}
                    />
                  </label>
                </div>

                <div className="title" style={{ marginTop: 6 }}>
                  输出比例
                </div>
                <div className="seg" style={{ background: '#2c2c30' }}>
                  {ASPECTS.map((a) => (
                    <button
                      key={a.id}
                      className={beautify.aspect === a.id ? 'active' : ''}
                      style={{
                        flex: 1,
                        height: 24,
                        fontSize: 11,
                        background: beautify.aspect === a.id ? '#6c5ce7' : 'transparent',
                        color: beautify.aspect === a.id ? '#fff' : '#a1a1aa'
                      }}
                      onClick={() => setBeautify({ ...beautify, aspect: a.id })}
                    >
                      {a.label}
                    </button>
                  ))}
                </div>

                <div className="title" style={{ marginTop: 6 }}>
                  外壳
                </div>
                <div className="swatches">
                  {FRAMES.map((f) => (
                    <button
                      key={f.id}
                      title={f.label}
                      className={`frame-chip ${frame.id === f.id ? 'active' : ''}`}
                      onClick={() => setFrame({ ...frame, id: f.id })}
                    >
                      {f.label}
                    </button>
                  ))}
                </div>

                {frame.id !== 'none' && (
                  <>
                    <div className="seg" style={{ background: '#2c2c30', marginTop: 4 }}>
                      <button
                        className={frame.dark ? 'active' : ''}
                        style={{
                          flex: 1,
                          height: 24,
                          fontSize: 11,
                          background: frame.dark ? '#6c5ce7' : 'transparent',
                          color: frame.dark ? '#fff' : '#a1a1aa'
                        }}
                        onClick={() => setFrame({ ...frame, dark: true })}
                      >
                        深色
                      </button>
                      <button
                        className={!frame.dark ? 'active' : ''}
                        style={{
                          flex: 1,
                          height: 24,
                          fontSize: 11,
                          background: !frame.dark ? '#6c5ce7' : 'transparent',
                          color: !frame.dark ? '#fff' : '#a1a1aa'
                        }}
                        onClick={() => setFrame({ ...frame, dark: false })}
                      >
                        浅色
                      </button>
                    </div>
                    {(frame.id === 'mac' ||
                      frame.id === 'windows' ||
                      frame.id === 'browser') && (
                      <input
                        className="dark-input"
                        placeholder="标题栏文字（可留空）"
                        value={frame.title}
                        onChange={(e) => setFrame({ ...frame, title: e.target.value })}
                        style={{ marginTop: 4 }}
                      />
                    )}
                  </>
                )}
              </>
            )}
          </div>

          <div className="panel-group">
            <div
              className="title"
              style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}
            >
              <span>水印</span>
              <button
                className={`switch ${watermark.enabled ? 'on' : ''}`}
                style={{ transform: 'scale(0.82)' }}
                onClick={() => {
                  const next = !watermark.enabled
                  // 打开时如果没写文案，给个默认值，否则开了也看不到任何变化
                  setWatermark({
                    ...watermark,
                    enabled: next,
                    text: next && !watermark.text.trim() ? 'SnipForge' : watermark.text
                  })
                }}
              />
            </div>

            {watermark.enabled && (
              <>
                <input
                  className="dark-input"
                  placeholder="水印文字"
                  value={watermark.text}
                  onChange={(e) => setWatermark({ ...watermark, text: e.target.value })}
                />

                <div className="title" style={{ marginTop: 4 }}>
                  颜色
                </div>
                <div className="swatches">
                  {WATERMARK_COLORS.map((c) => (
                    <button
                      key={c}
                      className={`swatch ${watermark.color === c ? 'active' : ''}`}
                      style={{ background: c, width: 20, height: 20 }}
                      onClick={() => setWatermark({ ...watermark, color: c })}
                    />
                  ))}
                  <label
                    className="swatch"
                    style={{ width: 20, height: 20, cursor: 'pointer', position: 'relative' }}
                  >
                    <span
                      style={{
                        position: 'absolute',
                        inset: 0,
                        borderRadius: 6,
                        background:
                          'conic-gradient(#ff3b30,#ff9500,#ffcc00,#34c759,#0a84ff,#6c5ce7,#ff3b30)'
                      }}
                    />
                    <input
                      type="color"
                      value={watermark.color}
                      onChange={(e) => setWatermark({ ...watermark, color: e.target.value })}
                      style={{ opacity: 0, width: '100%', height: '100%' }}
                    />
                  </label>
                </div>

                <div className="title" style={{ marginTop: 4 }}>
                  不透明度 · {Math.round(watermark.opacity * 100)}%
                </div>
                <input
                  type="range"
                  className="slider"
                  min={5}
                  max={90}
                  value={watermark.opacity * 100}
                  onChange={(e) =>
                    setWatermark({ ...watermark, opacity: Number(e.target.value) / 100 })
                  }
                />

                <div className="title" style={{ marginTop: 4 }}>
                  字号 · {watermark.fontSize}px
                </div>
                <input
                  type="range"
                  className="slider"
                  min={10}
                  max={64}
                  value={watermark.fontSize}
                  onChange={(e) =>
                    setWatermark({ ...watermark, fontSize: Number(e.target.value) })
                  }
                />

                <div className="seg" style={{ background: '#2c2c30', marginTop: 4 }}>
                  <button
                    className={!watermark.tiled ? 'active' : ''}
                    style={{
                      flex: 1,
                      height: 24,
                      fontSize: 11,
                      background: !watermark.tiled ? '#6c5ce7' : 'transparent',
                      color: !watermark.tiled ? '#fff' : '#a1a1aa'
                    }}
                    onClick={() => setWatermark({ ...watermark, tiled: false })}
                  >
                    单行
                  </button>
                  <button
                    className={watermark.tiled ? 'active' : ''}
                    style={{
                      flex: 1,
                      height: 24,
                      fontSize: 11,
                      background: watermark.tiled ? '#6c5ce7' : 'transparent',
                      color: watermark.tiled ? '#fff' : '#a1a1aa'
                    }}
                    onClick={() => setWatermark({ ...watermark, tiled: true })}
                  >
                    平铺
                  </button>
                </div>
              </>
            )}
          </div>

          {ocr && (
            <div className="panel-group">
              <div className="title" style={{ display: 'flex', justifyContent: 'space-between' }}>
                <span>OCR 结果</span>
                <span style={{ textTransform: 'none', color: '#52525b' }}>
                  {ocr.durationMs}ms
                </span>
              </div>
              <textarea
                className="dark-input"
                style={{ height: 130, padding: 8, fontFamily: 'var(--mono)', fontSize: 11, resize: 'vertical' }}
                value={ocr.text}
                onChange={(e) => setOcr({ ...ocr, text: e.target.value })}
              />
              <button
                className="btn sm"
                style={{ background: '#2c2c30', border: 'none', color: '#e4e4e7' }}
                onClick={async () => {
                  await api.clipboardCopyText(ocr.text)
                }}
              >
                <IconCopy size={13} /> 复制文字
              </button>
            </div>
          )}
        </div>
      </div>
    </div>
  )
}

/* ---------------- 工具图标（内联，避免依赖图标库） ---------------- */

function BoxIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <rect x="4" y="5" width="16" height="14" rx="2" />
    </svg>
  )
}
function CircleIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <ellipse cx="12" cy="12" rx="8" ry="7" />
    </svg>
  )
}
function ArrowIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <path d="M5 19L19 5M19 5h-7M19 5v7" strokeLinecap="round" />
    </svg>
  )
}
function PenIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <path d="M4 20c4-1 6-3 8-6s3-6 5-7 3 0 3 2-3 4-6 4-6-1-8 2-2 5-2 5z" strokeLinecap="round" />
    </svg>
  )
}
function MarkerIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <path d="M5 17h14" strokeLinecap="round" />
      <path d="M7 13l7-8 4 4-7 8H7v-4z" strokeLinejoin="round" />
    </svg>
  )
}
function NumberIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8">
      <circle cx="12" cy="12" r="8" />
      <path d="M11 9.5l1.5-1v7" strokeLinecap="round" />
    </svg>
  )
}
function MosaicIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="currentColor">
      <rect x="4" y="4" width="5" height="5" rx="1" opacity=".9" />
      <rect x="10" y="4" width="5" height="5" rx="1" opacity=".5" />
      <rect x="16" y="4" width="4" height="5" rx="1" opacity=".85" />
      <rect x="4" y="10" width="5" height="5" rx="1" opacity=".45" />
      <rect x="10" y="10" width="5" height="5" rx="1" opacity=".95" />
      <rect x="16" y="10" width="4" height="5" rx="1" opacity=".5" />
      <rect x="4" y="16" width="5" height="4" rx="1" opacity=".8" />
      <rect x="10" y="16" width="5" height="4" rx="1" opacity=".4" />
      <rect x="16" y="16" width="4" height="4" rx="1" opacity=".9" />
    </svg>
  )
}

// 独立窗口的挂载点
const container = document.getElementById('root')
if (container) {
  createRoot(container).render(<Editor />)
}
