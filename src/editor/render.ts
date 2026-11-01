import {
  aspectValue,
  type BeautifyOptions,
  type FrameId,
  type FrameOptions,
  type Shape,
  type WatermarkOptions
} from './model'

/**
 * 标注渲染。
 * 所有图形都是"重绘"而不是"烧进像素"——这是能随时改色、撤销的前提。
 */

export function drawShapes(ctx: CanvasRenderingContext2D, shapes: Shape[]): void {
  for (const shape of shapes) drawShape(ctx, shape)
}

export function drawShape(ctx: CanvasRenderingContext2D, shape: Shape): void {
  ctx.save()
  ctx.lineCap = 'round'
  ctx.lineJoin = 'round'
  ctx.strokeStyle = shape.color
  ctx.fillStyle = shape.color
  ctx.lineWidth = shape.strokeWidth

  switch (shape.type) {
    case 'rect': {
      ctx.beginPath()
      roundRect(ctx, shape.x, shape.y, shape.width, shape.height, 4)
      ctx.stroke()
      break
    }
    case 'ellipse': {
      ctx.beginPath()
      ctx.ellipse(
        shape.x + shape.width / 2,
        shape.y + shape.height / 2,
        Math.max(1, shape.width / 2),
        Math.max(1, shape.height / 2),
        0,
        0,
        Math.PI * 2
      )
      ctx.stroke()
      break
    }
    case 'arrow': {
      drawArrow(ctx, shape.x1, shape.y1, shape.x2, shape.y2, shape.strokeWidth, shape.style)
      break
    }
    case 'pen': {
      if (shape.points.length > 1) {
        ctx.beginPath()
        ctx.moveTo(shape.points[0].x, shape.points[0].y)
        for (let i = 1; i < shape.points.length; i++) {
          ctx.lineTo(shape.points[i].x, shape.points[i].y)
        }
        ctx.stroke()
      }
      break
    }
    case 'marker': {
      ctx.globalAlpha = shape.alpha ?? 0.34
      ctx.fillRect(shape.x, shape.y, shape.width, shape.height)
      break
    }
    case 'text': {
      ctx.font = `600 ${shape.fontSize}px "Segoe UI Variable Display", "Microsoft YaHei UI", sans-serif`
      ctx.textBaseline = 'alphabetic'
      // 先描一层浅色底，保证在深浅背景上都看得清
      const metrics = ctx.measureText(shape.text)
      const pad = 3
      ctx.globalAlpha = 0.72
      ctx.fillStyle = 'rgba(0,0,0,0.55)'
      roundRect(
        ctx,
        shape.x - pad,
        shape.y - shape.fontSize + 2,
        metrics.width + pad * 2,
        shape.fontSize + 8,
        4
      )
      ctx.fill()
      ctx.globalAlpha = 1
      ctx.fillStyle = shape.color
      ctx.fillText(shape.text, shape.x, shape.y)
      break
    }
    case 'number': {
      const r = shape.radius
      ctx.beginPath()
      ctx.arc(shape.x, shape.y, r, 0, Math.PI * 2)
      ctx.fillStyle = shape.color
      ctx.fill()
      ctx.fillStyle = '#ffffff'
      ctx.font = `700 ${r}px "Segoe UI Variable Display", sans-serif`
      ctx.textAlign = 'center'
      ctx.textBaseline = 'middle'
      ctx.fillText(String(shape.index), shape.x, shape.y + 0.5)
      break
    }
    case 'mosaic':
      // 马赛克需要源像素，单独处理（见 applyMosaic）
      break
    default: {
      // 编译期完整性检查：新增 Shape 类型却忘了在这里画，这行会直接报类型错误
      const unhandled: never = shape
      void unhandled
      break
    }
  }
  ctx.restore()
}

function roundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number
): void {
  const radius = Math.min(r, Math.abs(w) / 2, Math.abs(h) / 2)
  if (typeof ctx.roundRect === 'function') {
    ctx.roundRect(x, y, w, h, radius)
    return
  }
  ctx.moveTo(x + radius, y)
  ctx.arcTo(x + w, y, x + w, y + h, radius)
  ctx.arcTo(x + w, y + h, x, y + h, radius)
  ctx.arcTo(x, y + h, x, y, radius)
  ctx.arcTo(x, y, x + w, y, radius)
  ctx.closePath()
}

function drawArrow(
  ctx: CanvasRenderingContext2D,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  width: number,
  style?: 'solid' | 'tapered'
): void {
  if (style === 'tapered') {
    drawTaperedArrow(ctx, x1, y1, x2, y2, width)
    return
  }

  const angle = Math.atan2(y2 - y1, x2 - x1)
  const headLen = Math.max(10, width * 4)
  const headAngle = Math.PI / 7

  // 线尾退让一点，避免线头从箭头里露出来
  const lineEndX = x2 - Math.cos(angle) * headLen * 0.6
  const lineEndY = y2 - Math.sin(angle) * headLen * 0.6

  ctx.beginPath()
  ctx.moveTo(x1, y1)
  ctx.lineTo(lineEndX, lineEndY)
  ctx.stroke()

  ctx.beginPath()
  ctx.moveTo(x2, y2)
  ctx.lineTo(
    x2 - headLen * Math.cos(angle - headAngle),
    y2 - headLen * Math.sin(angle - headAngle)
  )
  ctx.lineTo(
    x2 - headLen * Math.cos(angle + headAngle),
    y2 - headLen * Math.sin(angle + headAngle)
  )
  ctx.closePath()
  ctx.fill()
}

/**
 * 锥尾箭头：照老板给的 SVG 复刻。
 *
 * 形状拆开看就三段，全是对称的直线：
 *   1. 身：从尾尖（宽 0）一路张开到头基处（0.38 × 头宽）—— 一条很细的长锥
 *   2. 肩：头基比身宽得多，两侧各横出一条平肩（这就是"燕尾"的那个叉）
 *   3. 头：平肩外沿直接收到箭尖，一个大三角
 *
 * 参考比例（来自 箭头.svg，以头宽 W 为基准）：
 *   头长 = 0.72W　肩宽 = 0.5W（半宽）　身宽 = 0.38W　全长 ≈ 2.5W
 *
 * 用直线不用贝塞尔：原图就是直线，加了弧度反而不像了。
 */
function drawTaperedArrow(
  ctx: CanvasRenderingContext2D,
  x1: number,
  y1: number,
  x2: number,
  y2: number,
  width: number
): void {
  const dx = x2 - x1
  const dy = y2 - y1
  const len = Math.hypot(dx, dy)
  if (len < 2) return

  const ux = dx / len
  const uy = dy / len
  // 轴的法线方向
  const px = -uy
  const py = ux

  const headW = Math.max(16, width * 5)
  const headLen = Math.min(len * 0.6, headW * 0.72)
  // 头基处箭身有多宽（0.38 × 头宽），也就是两个肩各自伸出去多少
  const shaftHalf = headW * 0.19

  // 头基：肩线所在的位置
  const jx = x2 - ux * headLen
  const jy = y2 - uy * headLen

  ctx.beginPath()
  ctx.moveTo(x1, y1)
  // 身：尾尖 → 头基处的身宽
  ctx.lineTo(jx + px * shaftHalf, jy + py * shaftHalf)
  // 右肩：横出去到头的半宽
  ctx.lineTo(jx + px * (headW / 2), jy + py * (headW / 2))
  // 头：右肩外沿收到箭尖
  ctx.lineTo(x2, y2)
  // 左半边镜像
  ctx.lineTo(jx - px * (headW / 2), jy - py * (headW / 2))
  ctx.lineTo(jx - px * shaftHalf, jy - py * shaftHalf)
  ctx.closePath()
  ctx.fill()
  // 再描一圈同色，把小尺寸下的锯齿边抹平
  ctx.lineWidth = Math.max(1, width * 0.3)
  ctx.stroke()
}

/** 悬停高亮：虚线包住整个图形，告诉用户滚轮会改哪个 */
/**
 * 选中框：实线 + 四角方块。
 *
 * 和悬停框（虚线）刻意做得不一样 —— 悬停只是"滚轮会改这个"的提示，
 * 选中才意味着"点 Delete 会删掉、拖动会移动"，两者混在一起用户会误删。
 */
export function drawSelectOutline(
  ctx: CanvasRenderingContext2D,
  bounds: { x: number; y: number; width: number; height: number },
  lineWidth: number
): void {
  const pad = lineWidth * 2.5
  const x = bounds.x - pad
  const y = bounds.y - pad
  const w = Math.max(bounds.width + pad * 2, lineWidth * 4)
  const h = Math.max(bounds.height + pad * 2, lineWidth * 4)

  ctx.save()
  ctx.strokeStyle = 'rgba(108, 92, 231, 1)'
  ctx.lineWidth = lineWidth
  ctx.setLineDash([])
  ctx.strokeRect(x, y, w, h)

  const s = lineWidth * 4
  ctx.fillStyle = '#ffffff'
  for (const [hx, hy] of [
    [x, y],
    [x + w, y],
    [x, y + h],
    [x + w, y + h]
  ]) {
    ctx.fillRect(hx - s / 2, hy - s / 2, s, s)
    ctx.strokeRect(hx - s / 2, hy - s / 2, s, s)
  }
  ctx.restore()
}

export function drawHoverOutline(
  ctx: CanvasRenderingContext2D,
  bounds: { x: number; y: number; width: number; height: number },
  lineWidth: number
): void {
  const pad = lineWidth * 2.5
  ctx.save()
  ctx.strokeStyle = 'rgba(108, 92, 231, 0.95)'
  ctx.lineWidth = lineWidth
  ctx.setLineDash([lineWidth * 4, lineWidth * 3])
  ctx.strokeRect(
    bounds.x - pad,
    bounds.y - pad,
    bounds.width + pad * 2,
    bounds.height + pad * 2
  )
  ctx.restore()
}

/**
 * 马赛克：把区域内的像素按块平均后放大。
 * 用"缩小再放大 + 关闭平滑"实现，比逐块计算快一个数量级，
 * 视觉效果对脱敏场景完全够。
 */
export function applyMosaic(
  ctx: CanvasRenderingContext2D,
  canvas: HTMLCanvasElement,
  x: number,
  y: number,
  w: number,
  h: number,
  blockSize = 12
): void {
  const sx = Math.max(0, Math.floor(x))
  const sy = Math.max(0, Math.floor(y))
  const sw = Math.min(Math.floor(w), canvas.width - sx)
  const sh = Math.min(Math.floor(h), canvas.height - sy)
  if (sw <= 0 || sh <= 0) return

  const small = document.createElement('canvas')
  small.width = Math.max(1, Math.floor(sw / blockSize))
  small.height = Math.max(1, Math.floor(sh / blockSize))
  const sctx = small.getContext('2d')!
  sctx.imageSmoothingEnabled = true
  sctx.drawImage(canvas, sx, sy, sw, sh, 0, 0, small.width, small.height)

  ctx.save()
  ctx.imageSmoothingEnabled = false
  ctx.drawImage(small, 0, 0, small.width, small.height, sx, sy, sw, sh)
  ctx.restore()
}

/** 一块已经算好的马赛克：直接 drawImage 就能贴回去 */
export interface MosaicTile {
  canvas: HTMLCanvasElement
  sx: number
  sy: number
  sw: number
  sh: number
}

/**
 * 马赛克小块的缓存。
 *
 * 以前每帧每个马赛克都要 `createElement('canvas')` + 缩小 + 放大，
 * 画上三四个再拖动就开始掉帧 —— 而马赛克的输入（底图某块 + 颗粒大小）
 * 在参数没变时**完全一样**，算一次就该存下来。
 *
 * 取样源是**只含原图的离屏底图**，不是正在画的那块画布：
 * 这样两个马赛克挨在一起时不会互相二次像素化，后画的标注也不会被它吃掉。
 */
const mosaicCache = new Map<string, HTMLCanvasElement>()
/** 缓存上限。超了直接清空 —— 比维护 LRU 简单，而且马赛克通常就几个 */
const MOSAIC_CACHE_MAX = 24

export function mosaicTile(
  base: HTMLCanvasElement,
  x: number,
  y: number,
  w: number,
  h: number,
  blockSize: number,
  version: number
): MosaicTile | null {
  const sx = Math.max(0, Math.floor(x))
  const sy = Math.max(0, Math.floor(y))
  const sw = Math.min(Math.floor(w), base.width - sx)
  const sh = Math.min(Math.floor(h), base.height - sy)
  if (sw <= 0 || sh <= 0) return null

  const block = Math.max(2, Math.round(blockSize))
  const key = `${version}:${sx},${sy},${sw},${sh},${block}`
  const hit = mosaicCache.get(key)
  if (hit) return { canvas: hit, sx, sy, sw, sh }

  const small = document.createElement('canvas')
  small.width = Math.max(1, Math.floor(sw / block))
  small.height = Math.max(1, Math.floor(sh / block))
  const g = small.getContext('2d')
  if (!g) return null
  g.imageSmoothingEnabled = true
  g.drawImage(base, sx, sy, sw, sh, 0, 0, small.width, small.height)

  if (mosaicCache.size >= MOSAIC_CACHE_MAX) mosaicCache.clear()
  mosaicCache.set(key, small)
  return { canvas: small, sx, sy, sw, sh }
}

/** 换了一张截图，旧缓存全部作废 */
export function clearMosaicCache(): void {
  mosaicCache.clear()
}

/** 判断某个图形是否需要马赛克后处理 */
export const isMosaic = (s: Shape): boolean => s.type === 'mosaic'

/**
 * 合成最终图片：美化底板 + 圆角裁剪 + 阴影 + 水印。
 * 返回新 canvas，不改动输入的源 canvas。
 *
 * 输出尺寸的算法：
 *   - 基础尺寸 = 原图 + 内边距
 *   - 选了比例且比基础尺寸更"宽"或更"高"，就按比例扩边，图片居中
 *   - 任何时候都不裁剪原图（截图工具不能悄悄切掉用户的内容）
 */
export function composeFinal(
  source: HTMLCanvasElement,
  beautify: BeautifyOptions,
  watermark: WatermarkOptions,
  frame: FrameOptions
): HTMLCanvasElement {
  const pad = beautify.enabled ? beautify.padding : 0

  // 外壳先占一块地方，再算内边距，顺序不能反 ——
  // 反了的话标题栏会盖在内边距上，视觉上像贴纸贴歪了
  const outer = frameOuterSize(source.width, source.height, frame)

  let outW = outer.width + pad * 2
  let outH = outer.height + pad * 2

  const ratio = aspectValue(beautify.aspect)
  if (beautify.enabled && ratio) {
    if (outW / outH < ratio) outW = Math.round(outH * ratio)
    else outH = Math.round(outW / ratio)
  }

  const out = document.createElement('canvas')
  out.width = outW
  out.height = outH
  const ctx = out.getContext('2d')!

  // 内容块（外壳）在输出画布上居中
  const cx = Math.round((outW - outer.width) / 2)
  const cy = Math.round((outH - outer.height) / 2)

  if (beautify.enabled && beautify.bgMode !== 'transparent') {
    if (beautify.bgMode === 'gradient') {
      const g = ctx.createLinearGradient(0, 0, outW, outH)
      g.addColorStop(0, beautify.gradientFrom)
      g.addColorStop(1, beautify.gradientTo)
      ctx.fillStyle = g
    } else {
      ctx.fillStyle = beautify.background
    }
    ctx.fillRect(0, 0, outW, outH)
  }

  // 阴影：多层半透明圆角矩形，比 shadowBlur 更可控也更均匀
  if (beautify.enabled && beautify.shadow > 0) {
    const strength = beautify.shadow / 100
    const baseRadius = frame.id === 'none' ? beautify.radius : outer.metrics.radius
    for (let i = 6; i >= 1; i--) {
      const spread = ((strength * 90) / 6) * i
      ctx.save()
      ctx.globalAlpha = 0.028 * (7 - i) * (0.4 + strength)
      ctx.fillStyle = '#000000'
      ctx.beginPath()
      roundRectPath(
        ctx,
        cx - spread * 0.5,
        cy - spread * 0.3,
        outer.width + spread,
        outer.height + spread,
        baseRadius + spread * 0.3
      )
      ctx.fill()
      ctx.restore()
    }
  }

  // 外壳 + 图片位置
  const { imgX, imgY } = drawFrame(ctx, frame, cx, cy, source.width, source.height)

  ctx.save()
  const innerRadius =
    frame.id === 'none' ? (beautify.enabled ? beautify.radius : 0) : Math.max(2, outer.metrics.radius - 6)
  if (innerRadius > 0) {
    ctx.beginPath()
    roundRectPath(ctx, imgX, imgY, source.width, source.height, innerRadius)
    ctx.clip()
  }
  ctx.drawImage(source, imgX, imgY)
  ctx.restore()

  if (watermark.enabled && watermark.text.trim()) {
    drawWatermark(ctx, out.width, out.height, watermark)
  }

  return out
}

function drawWatermark(
  ctx: CanvasRenderingContext2D,
  width: number,
  height: number,
  wm: WatermarkOptions
): void {
  ctx.save()
  ctx.globalAlpha = wm.opacity
  ctx.fillStyle = wm.color || '#ffffff'
  ctx.font = `600 ${wm.fontSize}px "Segoe UI Variable Display", "Microsoft YaHei UI", sans-serif`
  ctx.textBaseline = 'middle'
  // 一层柔和阴影：浅色文字压在亮背景上时也能看清，不用给用户加描边参数
  ctx.shadowColor = 'rgba(0,0,0,0.35)'
  ctx.shadowBlur = Math.max(2, wm.fontSize * 0.22)

  if (wm.tiled) {
    ctx.save()
    ctx.translate(width / 2, height / 2)
    ctx.rotate(-Math.PI / 6)
    const stepX = ctx.measureText(wm.text).width + 90
    const stepY = wm.fontSize * 4
    const cols = Math.ceil(width / stepX) + 2
    const rows = Math.ceil(height / stepY) + 2
    ctx.textAlign = 'center'
    for (let r = -rows; r <= rows; r++) {
      for (let c = -cols; c <= cols; c++) {
        ctx.fillText(wm.text, c * stepX, r * stepY)
      }
    }
    ctx.restore()
  } else {
    const metrics = ctx.measureText(wm.text)
    ctx.textAlign = 'right'
    ctx.fillText(wm.text, width - wm.fontSize * 1.2, height - wm.fontSize * 1.2)
    ctx.globalAlpha = wm.opacity * 0.5
    ctx.fillRect(width - wm.fontSize * 1.2 - metrics.width - 8, height - wm.fontSize * 1.2, metrics.width + 8, 1)
  }
  ctx.restore()
}


/* ============================ 窗口边框 / 设备外壳 ============================ */

/**
 * 每种外壳占用的"额外空间"。
 *
 * 全部按**原图像素**计，不随预览缩放变化 —— 这样导出时永远精确，
 * 预览侧只需乘一个 scale 就能对齐。
 */
export interface FrameMetrics {
  /** 标题栏高度 */
  top: number
  /** 左右边框宽度 */
  side: number
  /** 底部机身厚度 */
  bottom: number
  /** 外壳圆角 */
  radius: number
}

export function frameMetrics(id: FrameId): FrameMetrics {
  switch (id) {
    case 'mac':
      return { top: 34, side: 0, bottom: 0, radius: 12 }
    case 'windows':
      return { top: 38, side: 0, bottom: 0, radius: 10 }
    case 'browser':
      return { top: 48, side: 0, bottom: 0, radius: 12 }
    case 'laptop':
      return { top: 16, side: 16, bottom: 34, radius: 18 }
    case 'phone':
      return { top: 30, side: 14, bottom: 40, radius: 48 }
    default:
      return { top: 0, side: 0, bottom: 0, radius: 0 }
  }
}

/** 外壳占用的总尺寸（不含图片本身） */
export function frameOuterSize(imgW: number, imgH: number, opts: FrameOptions) {
  const m = frameMetrics(opts.id)
  return {
    width: imgW + m.side * 2,
    height: imgH + m.top + m.bottom,
    metrics: m
  }
}

function roundRectPath(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number
): void {
  const radius = Math.min(r, Math.abs(w) / 2, Math.abs(h) / 2)
  ctx.beginPath()
  if (typeof ctx.roundRect === 'function') {
    ctx.roundRect(x, y, w, h, radius)
    return
  }
  ctx.moveTo(x + radius, y)
  ctx.arcTo(x + w, y, x + w, y + h, radius)
  ctx.arcTo(x + w, y + h, x, y + h, radius)
  ctx.arcTo(x, y + h, x, y, radius)
  ctx.arcTo(x, y, x + w, y, radius)
  ctx.closePath()
}

/**
 * 画外壳，返回图片应该放的位置。
 * 坐标系是输出画布，单位原图像素。
 */
export function drawFrame(
  ctx: CanvasRenderingContext2D,
  opts: FrameOptions,
  x: number,
  y: number,
  imgW: number,
  imgH: number
): { imgX: number; imgY: number } {
  if (opts.id === 'none') return { imgX: x, imgY: y }

  const m = frameMetrics(opts.id)
  const outerW = imgW + m.side * 2
  const outerH = imgH + m.top + m.bottom

  const shell = opts.dark ? '#33333a' : '#f4f4f7'
  const shellEdge = opts.dark ? '#45454f' : '#e2e2e8'
  const ink = opts.dark ? 'rgba(255,255,255,0.72)' : 'rgba(0,0,0,0.62)'
  const subtle = opts.dark ? 'rgba(255,255,255,0.12)' : 'rgba(0,0,0,0.08)'

  ctx.save()

  // 外壳本体
  roundRectPath(ctx, x, y, outerW, outerH, m.radius)
  ctx.fillStyle = shell
  ctx.fill()
  if (m.side > 0 || m.bottom > 0) {
    ctx.strokeStyle = shellEdge
    ctx.lineWidth = 1
    ctx.stroke()
  }

  // 标题栏
  if (m.top > 0 && opts.id !== 'phone') {
    const barH = m.top
    const barY = y

    if (opts.id === 'mac') {
      // 三个圆点：红黄绿
      const dotR = Math.max(4, barH * 0.18)
      const cy = barY + barH / 2
      const colors = ['#FF5F57', '#FEBC2E', '#28C840']
      colors.forEach((c, i) => {
        ctx.beginPath()
        ctx.arc(x + 18 + i * (dotR * 3), cy, dotR, 0, Math.PI * 2)
        ctx.fillStyle = c
        ctx.fill()
      })
      if (opts.title) {
        ctx.fillStyle = ink
        ctx.font = `500 ${Math.round(barH * 0.38)}px "Segoe UI", "Microsoft YaHei UI", sans-serif`
        ctx.textAlign = 'center'
        ctx.textBaseline = 'middle'
        ctx.fillText(opts.title, x + outerW / 2, cy, outerW * 0.6)
      }
    } else if (opts.id === 'windows') {
      ctx.fillStyle = ink
      ctx.font = `500 ${Math.round(barH * 0.34)}px "Segoe UI", "Microsoft YaHei UI", sans-serif`
      ctx.textAlign = 'left'
      ctx.textBaseline = 'middle'
      if (opts.title) {
        ctx.fillText(opts.title, x + 14, barY + barH / 2, outerW * 0.6)
      }
      // 右侧三个窗口按钮：— □ ✕
      const bw = Math.round(barH * 0.46)
      const bx = x + outerW - bw - 6
      const cy = barY + barH / 2
      ctx.strokeStyle = ink
      ctx.lineWidth = Math.max(1, barH * 0.035)
      for (let i = 0; i < 3; i++) {
        const cx = bx - i * (bw + 4) + bw / 2
        const half = bw * 0.24
        ctx.beginPath()
        if (i === 0) {
          ctx.moveTo(cx - half, cy)
          ctx.lineTo(cx + half, cy)
        } else if (i === 1) {
          ctx.rect(cx - half, cy - half, half * 2, half * 2)
        } else {
          ctx.moveTo(cx - half, cy - half)
          ctx.lineTo(cx + half, cy + half)
          ctx.moveTo(cx + half, cy - half)
          ctx.lineTo(cx - half, cy + half)
        }
        ctx.stroke()
      }
    } else if (opts.id === 'browser') {
      // 圆点 + 圆角地址栏
      const dotR = Math.max(3.5, barH * 0.12)
      const cy = barY + barH / 2
      const colors = ['#FF5F57', '#FEBC2E', '#28C840']
      colors.forEach((c, i) => {
        ctx.beginPath()
        ctx.arc(x + 16 + i * (dotR * 2.9), cy, dotR, 0, Math.PI * 2)
        ctx.fillStyle = c
        ctx.fill()
      })

      const pillX = x + 16 + dotR * 8
      const pillW = outerW - (pillX - x) - 16
      const pillH = barH * 0.5
      roundRectPath(ctx, pillX, cy - pillH / 2, pillW, pillH, pillH / 2)
      ctx.fillStyle = opts.dark ? 'rgba(255,255,255,0.09)' : 'rgba(0,0,0,0.05)'
      ctx.fill()
      ctx.strokeStyle = subtle
      ctx.lineWidth = 1
      ctx.stroke()

      if (opts.title) {
        ctx.fillStyle = ink
        ctx.font = `400 ${Math.round(barH * 0.3)}px "Segoe UI", "Microsoft YaHei UI", sans-serif`
        ctx.textAlign = 'center'
        ctx.textBaseline = 'middle'
        ctx.fillText(opts.title, pillX + pillW / 2, cy, pillW * 0.85)
      }
    }
  }

  // 手机：顶部听筒 + 底部指示条
  if (opts.id === 'phone') {
    const notchW = outerW * 0.28
    const notchH = Math.max(6, m.top * 0.24)
    roundRectPath(ctx, x + (outerW - notchW) / 2, y + m.top * 0.32, notchW, notchH, notchH / 2)
    ctx.fillStyle = opts.dark ? 'rgba(0,0,0,0.75)' : 'rgba(0,0,0,0.35)'
    ctx.fill()

    const barW = outerW * 0.32
    roundRectPath(
      ctx,
      x + (outerW - barW) / 2,
      y + outerH - m.bottom * 0.62,
      barW,
      Math.max(3, m.bottom * 0.12),
      m.bottom * 0.06
    )
    ctx.fillStyle = opts.dark ? 'rgba(255,255,255,0.55)' : 'rgba(0,0,0,0.35)'
    ctx.fill()
  }

  // 笔记本底座：底部一条收窄的梯形，做出厚度感
  if (opts.id === 'laptop') {
    const baseY = y + outerH - m.bottom
    const inset = outerW * 0.06
    ctx.beginPath()
    ctx.moveTo(x + inset, baseY)
    ctx.lineTo(x + outerW - inset, baseY)
    ctx.lineTo(x + outerW - inset * 0.4, y + outerH)
    ctx.lineTo(x + inset * 0.4, y + outerH)
    ctx.closePath()
    ctx.fillStyle = opts.dark ? '#2a2a30' : '#e8e8ee'
    ctx.fill()
    // 屏幕与底座之间的分界线
    ctx.beginPath()
    ctx.moveTo(x + inset, baseY)
    ctx.lineTo(x + outerW - inset, baseY)
    ctx.strokeStyle = opts.dark ? 'rgba(0,0,0,0.5)' : 'rgba(0,0,0,0.12)'
    ctx.lineWidth = 1
    ctx.stroke()
  }

  // 摄像头点（笔记本 / 手机以外留给 mac）
  if (opts.id === 'laptop') {
    ctx.beginPath()
    ctx.arc(x + outerW / 2, y + m.top / 2, Math.max(2, m.top * 0.12), 0, Math.PI * 2)
    ctx.fillStyle = 'rgba(0,0,0,0.55)'
    ctx.fill()
  }

  ctx.restore()

  return { imgX: x + m.side, imgY: y + m.top }
}
