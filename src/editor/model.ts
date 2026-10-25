/**
 * 标注对象模型。
 *
 * 纯数据 + 纯函数，不依赖 React 和 DOM。
 * 好处：撤销/重做只是数组快照，序列化就是 JSON，测试不用起浏览器。
 */

export type ToolId =
  | 'select'
  | 'rect'
  | 'ellipse'
  | 'arrow'
  | 'pen'
  | 'marker'
  | 'text'
  | 'number'
  | 'mosaic'

export interface BaseShape {
  id: string
  type: ToolId
  color: string
  strokeWidth: number
}

export interface BoxShape extends BaseShape {
  type: 'rect' | 'ellipse' | 'mosaic'
  x: number
  y: number
  width: number
  height: number
  /** 仅马赛克用：颗粒大小，越大越糊 */
  blockSize?: number
}

/** 箭头样式：solid = 细线 + 实心三角头；tapered = 尾宽头尖的锥形（拖尾） */
export type ArrowStyle = 'solid' | 'tapered'

export interface ArrowShape extends BaseShape {
  type: 'arrow'
  x1: number
  y1: number
  x2: number
  y2: number
  style?: ArrowStyle
}

export interface PenShape extends BaseShape {
  type: 'pen'
  points: Array<{ x: number; y: number }>
}

export interface MarkerShape extends BaseShape {
  type: 'marker'
  x: number
  y: number
  width: number
  height: number
  /** 荧光笔的不透明度，滚轮调深浅 */
  alpha?: number
}

export interface TextShape extends BaseShape {
  type: 'text'
  x: number
  y: number
  text: string
  fontSize: number
}

export interface NumberShape extends BaseShape {
  type: 'number'
  x: number
  y: number
  index: number
  radius: number
}

export type Shape =
  | BoxShape
  | ArrowShape
  | PenShape
  | MarkerShape
  | TextShape
  | NumberShape

let seq = 0
export const newId = (): string => `s${++seq}_${Date.now().toString(36)}`

/** 输出比例。auto = 贴紧内容，其余按比例留白 */
export type AspectId = 'auto' | '1:1' | '4:3' | '16:9' | '9:16' | '3:2'

export const ASPECTS: Array<{ id: AspectId; label: string }> = [
  { id: 'auto', label: '原始' },
  { id: '1:1', label: '1:1' },
  { id: '4:3', label: '4:3' },
  { id: '3:2', label: '3:2' },
  { id: '16:9', label: '16:9' },
  { id: '9:16', label: '9:16' }
]

export function aspectValue(id: AspectId): number | null {
  switch (id) {
    case '1:1':
      return 1
    case '4:3':
      return 4 / 3
    case '3:2':
      return 3 / 2
    case '16:9':
      return 16 / 9
    case '9:16':
      return 9 / 16
    default:
      return null
  }
}

export interface BeautifyOptions {
  enabled: boolean
  padding: number
  radius: number
  /** 阴影强度 0-100 */
  shadow: number
  background: string
  /** 纯色 / 渐变 / 透明 */
  bgMode: 'solid' | 'gradient' | 'transparent'
  gradientFrom: string
  gradientTo: string
  /** 输出比例，auto 表示跟随内容 */
  aspect: AspectId
}

export const defaultBeautify = (): BeautifyOptions => ({
  enabled: false,
  padding: 56,
  radius: 14,
  shadow: 45,
  background: '#f2f2f5',
  bgMode: 'gradient',
  gradientFrom: '#6c5ce7',
  gradientTo: '#a29bfe',
  aspect: 'auto'
})

/** 背景预设，用一个数组管住，UI 直接遍历 */
export const BG_PRESETS: Array<{ id: string; label: string; from: string; to: string }> = [
  { id: 'indigo', label: '靛紫', from: '#6c5ce7', to: '#a29bfe' },
  { id: 'midnight', label: '午夜', from: '#0f172a', to: '#334155' },
  { id: 'sunset', label: '日落', from: '#ff9a9e', to: '#fad0c4' },
  { id: 'forest', label: '森林', from: '#1f4037', to: '#99f2c8' },
  { id: 'ocean', label: '海洋', from: '#2b5876', to: '#4e4376' },
  { id: 'paper', label: '纸白', from: '#f2f2f5', to: '#ffffff' }
]

export interface WatermarkOptions {
  enabled: boolean
  text: string
  opacity: number
  fontSize: number
  /** 平铺还是右下角 */
  tiled: boolean
  /** 水印颜色 */
  color: string
}

export const WATERMARK_COLORS = ['#FFFFFF', '#1C1C1E', '#FF3B30', '#FF9500', '#34C759', '#0A84FF', '#6C5CE7']

export const defaultWatermark = (): WatermarkOptions => ({
  enabled: false,
  text: '',
  opacity: 0.28,
  fontSize: 16,
  tiled: false,
  color: '#FFFFFF'
})

/* ---------------- 窗口边框 / 设备外壳 ---------------- */

export type FrameId = 'none' | 'mac' | 'windows' | 'browser' | 'laptop' | 'phone'

export const FRAMES: Array<{ id: FrameId; label: string }> = [
  { id: 'none', label: '无' },
  { id: 'mac', label: 'macOS' },
  { id: 'windows', label: 'Windows' },
  { id: 'browser', label: '浏览器' },
  { id: 'laptop', label: '笔记本' },
  { id: 'phone', label: '手机' }
]

export interface FrameOptions {
  /** 边框/外壳类型 */
  id: FrameId
  /** 深色 or 浅色外观 */
  dark: boolean
  /** 标题栏上显示的文字 */
  title: string
}

export const defaultFrame = (): FrameOptions => ({
  id: 'none',
  dark: true,
  title: ''
})

/** 命中测试：判断点是否落在图形上（用于选中/删除） */
export function hitTest(shape: Shape, x: number, y: number, tolerance = 6): boolean {
  switch (shape.type) {
    case 'rect':
    case 'ellipse':
    case 'mosaic': {
      const s = shape as BoxShape
      return (
        x >= s.x - tolerance &&
        x <= s.x + s.width + tolerance &&
        y >= s.y - tolerance &&
        y <= s.y + s.height + tolerance
      )
    }
    case 'arrow': {
      const s = shape as ArrowShape
      return distToSegment(x, y, s.x1, s.y1, s.x2, s.y2) <= Math.max(tolerance, s.strokeWidth)
    }
    case 'pen': {
      const s = shape as PenShape
      for (let i = 1; i < s.points.length; i++) {
        const a = s.points[i - 1]
        const b = s.points[i]
        if (distToSegment(x, y, a.x, a.y, b.x, b.y) <= Math.max(tolerance, s.strokeWidth)) return true
      }
      return false
    }
    case 'marker': {
      const s = shape as MarkerShape
      return x >= s.x && x <= s.x + s.width && y >= s.y && y <= s.y + s.height
    }
    case 'text': {
      const s = shape as TextShape
      return x >= s.x - tolerance && x <= s.x + 200 && y >= s.y - s.fontSize && y <= s.y + 8
    }
    case 'number': {
      const s = shape as NumberShape
      return Math.hypot(x - s.x, y - s.y) <= s.radius + tolerance
    }
    default:
      return false
  }
}

/**
 * 贴边命中测试：只有鼠标靠近描边才算命中。
 *
 * 和 hitTest 的区别：矩形/椭圆的**内部不算**。否则画面上有个大框，
 * 框里任何位置滚轮都会去改框的粗细，用户想放大那块内容就放大不了。
 */
export function hitShapeEdge(shape: Shape, x: number, y: number, tolerance = 6): boolean {
  switch (shape.type) {
    case 'rect': {
      const s = shape as BoxShape
      const t = Math.max(tolerance, s.strokeWidth * 0.9)
      const insideOuter =
        x >= s.x - t && x <= s.x + s.width + t && y >= s.y - t && y <= s.y + s.height + t
      const insideInner =
        x > s.x + t && x < s.x + s.width - t && y > s.y + t && y < s.y + s.height - t
      return insideOuter && !insideInner
    }
    case 'ellipse': {
      const s = shape as BoxShape
      const rx = Math.max(1, s.width / 2)
      const ry = Math.max(1, s.height / 2)
      const nx = (x - (s.x + rx)) / rx
      const ny = (y - (s.y + ry)) / ry
      const r = Math.sqrt(nx * nx + ny * ny)
      // 把归一化半径差换算回像素，乘较短半轴就够近似了
      return Math.abs(r - 1) * Math.min(rx, ry) <= Math.max(tolerance, s.strokeWidth)
    }
    default:
      return hitTest(shape, x, y, tolerance)
  }
}

/** 图形外接矩形，用来画悬停高亮框 */
export function shapeBounds(shape: Shape): { x: number; y: number; width: number; height: number } {
  switch (shape.type) {
    case 'rect':
    case 'ellipse':
    case 'mosaic':
    case 'marker': {
      const s = shape as BoxShape
      return { x: s.x, y: s.y, width: s.width, height: s.height }
    }
    case 'arrow': {
      const s = shape as ArrowShape
      return {
        x: Math.min(s.x1, s.x2),
        y: Math.min(s.y1, s.y2),
        width: Math.abs(s.x2 - s.x1),
        height: Math.abs(s.y2 - s.y1)
      }
    }
    case 'pen': {
      const s = shape as PenShape
      let x0 = Infinity
      let y0 = Infinity
      let x1 = -Infinity
      let y1 = -Infinity
      for (const p of s.points) {
        x0 = Math.min(x0, p.x)
        y0 = Math.min(y0, p.y)
        x1 = Math.max(x1, p.x)
        y1 = Math.max(y1, p.y)
      }
      if (!Number.isFinite(x0)) return { x: 0, y: 0, width: 0, height: 0 }
      return { x: x0, y: y0, width: x1 - x0, height: y1 - y0 }
    }
    case 'text': {
      const s = shape as TextShape
      // 精确宽度要 measureText，这里按平均字宽估算，画高亮框够用
      return {
        x: s.x,
        y: s.y - s.fontSize,
        width: Math.max(12, s.text.length * s.fontSize * 0.62),
        height: s.fontSize * 1.3
      }
    }
    case 'number': {
      const s = shape as NumberShape
      return { x: s.x - s.radius, y: s.y - s.radius, width: s.radius * 2, height: s.radius * 2 }
    }
    default:
      return { x: 0, y: 0, width: 0, height: 0 }
  }
}

/* ---------------- 滚轮可调的参数 ---------------- */

export type ParamKey = 'strokeWidth' | 'fontSize' | 'radius' | 'alpha' | 'blockSize'

export interface ShapeParam {
  key: ParamKey
  label: string
  min: number
  max: number
  /** 每滚一格的变化量 */
  step: number
  /** 0-1 的参数按百分比显示 */
  percent?: boolean
}

/** 每种图形滚轮调什么。加新工具只改这一处。 */
export function shapeParam(shape: Shape): ShapeParam {
  switch (shape.type) {
    case 'text':
      return { key: 'fontSize', label: '字号', min: 10, max: 96, step: 1 }
    case 'number':
      return { key: 'radius', label: '大小', min: 8, max: 64, step: 1 }
    case 'marker':
      return { key: 'alpha', label: '深浅', min: 0.06, max: 0.9, step: 0.05, percent: true }
    case 'mosaic':
      return { key: 'blockSize', label: '颗粒', min: 3, max: 48, step: 1 }
    default:
      return { key: 'strokeWidth', label: '粗细', min: 1, max: 24, step: 1 }
  }
}

export function getParam(shape: Shape, param: ShapeParam): number {
  const raw = (shape as unknown as Record<string, unknown>)[param.key]
  if (typeof raw === 'number') return raw
  // 老数据没有这些字段，给个和绘制时一致的默认值
  if (param.key === 'alpha') return 0.34
  if (param.key === 'blockSize') return 12
  if (param.key === 'radius') return 14
  if (param.key === 'fontSize') return 18
  return 3
}

export function withParam(shape: Shape, param: ShapeParam, value: number): Shape {
  const v = Math.min(param.max, Math.max(param.min, value))
  return { ...shape, [param.key]: v } as Shape
}

function distToSegment(px: number, py: number, x1: number, y1: number, x2: number, y2: number): number {
  const dx = x2 - x1
  const dy = y2 - y1
  const lenSq = dx * dx + dy * dy
  if (lenSq === 0) return Math.hypot(px - x1, py - y1)
  let t = ((px - x1) * dx + (py - y1) * dy) / lenSq
  t = Math.max(0, Math.min(1, t))
  return Math.hypot(px - (x1 + t * dx), py - (y1 + t * dy))
}

/** 归一化矩形，保证宽高为正 */
export function normalizeBox(a: { x: number; y: number }, b: { x: number; y: number }) {
  return {
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(b.x - a.x),
    height: Math.abs(b.y - a.y)
  }
}
