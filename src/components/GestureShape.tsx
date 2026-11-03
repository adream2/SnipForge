import { useMemo } from 'react'
import { patternToStroke, type Pt } from '../lib/gestureShape'

/**
 * 一个手势的缩略图：画出这一笔的轨迹，**起点一个圆点、终点一个箭头**。
 *
 * 为什么不用箭头符号拼：`↓→↑←` 这种四划手势，读起来是四个符号，
 * 得在脑子里重放一遍才想得起来画出来是什么样；而形状本身画出来，
 * 一眼就对得上手。
 */
export function GestureShape({
  pattern,
  points,
  size = 46,
  title,
}: {
  /** 方向串（DR）或形状字符（S） */
  pattern?: string
  /** 用户自绘的笔画（后端归一化过的 32 点） */
  points?: number[][]
  size?: number
  title?: string
}) {
  const stroke = useMemo<Pt[]>(() => {
    if (points && points.length >= 2) return points.map((p) => [p[0], p[1]] as Pt)
    return patternToStroke(pattern ?? '')
  }, [pattern, points])

  const geom = useMemo(() => layout(stroke), [stroke])

  if (!geom) {
    // 没有内置缩略图的形状（@W / @M …）：直接显示字母。
    // 画一条凑数的折线会让人以为那就是识别模板，不如老实显示名字。
    //
    // 兼容两种写法：新数据是 `@W`，老配置里可能只有 `W`（v4 迁移前录的），
    // 以及方向串里恰好只有一个字符的情况（`D` 是"向下划"，不能当字母 D）。
    const raw = (pattern ?? '').trim().toUpperCase()
    const letter = raw.startsWith('@')
      ? raw.slice(1)
      : /^[A-Z]$/.test(raw) && !'UDLR'.includes(raw)
        ? raw
        : ''
    if (letter) {
      return (
        <span
          className="gesture-shape-empty"
          title={title}
          style={{ fontSize: Math.round(size * 0.46), fontWeight: 600 }}
        >
          {letter}
        </span>
      )
    }
    return (
      <span className="gesture-shape-empty" title={title}>
        —
      </span>
    )
  }

  const h = Math.round((size * 72) / 100)
  return (
    <svg
      className="gesture-shape-svg"
      width={size}
      height={h}
      viewBox="0 0 100 72"
      role="img"
      aria-label={title ?? pattern ?? '手势'}
    >
      <polyline
        points={geom.line}
        fill="none"
        stroke="currentColor"
        strokeWidth={2.8}
        strokeLinecap="round"
        strokeLinejoin="round"
      />
      {/* 起点用**绿色**、终点箭头用主题色：同形手势（↓→↑← / →↓←↑ 都是同一个方框）
          只能靠"从哪个角起笔"区分，光靠一个小圆点在同一块底色上根本看不出来 */}
      <circle cx={geom.start[0]} cy={geom.start[1]} r={4.2} fill="#22c55e" />
      {/* 终点箭头：方向就是"最后一笔往哪走" */}
      <path d={geom.arrow} fill="currentColor" />
    </svg>
  )
}

/** 把任意点集铺到 100×72 的画布上（等比、留白、居中），并算出箭头 */
function layout(pts: Pt[]): { line: string; start: Pt; arrow: string } | null {
  if (pts.length < 2) return null
  const W = 100
  const H = 72
  const PAD = 13

  const xs = pts.map((p) => p[0])
  const ys = pts.map((p) => p[1])
  const minX = Math.min(...xs)
  const maxX = Math.max(...xs)
  const minY = Math.min(...ys)
  const maxY = Math.max(...ys)
  const w = maxX - minX
  const h = maxY - minY
  // 只有一个方向的分量时，那一轴不参与缩放（否则会被 0 除）
  const sx = w > 0.5 ? (W - PAD * 2) / w : Number.POSITIVE_INFINITY
  const sy = h > 0.5 ? (H - PAD * 2) / h : Number.POSITIVE_INFINITY
  const s = Math.min(sx, sy)
  if (!Number.isFinite(s)) return null

  const ox = (W - w * s) / 2
  const oy = (H - h * s) / 2
  const m: Pt[] = pts.map(([x, y]) => [ox + (x - minX) * s, oy + (y - minY) * s])

  const end = m[m.length - 1]
  const prev = m[m.length - 2]
  const ang = Math.atan2(end[1] - prev[1], end[0] - prev[0])
  const head = 13
  const segLen = Math.hypot(end[0] - prev[0], end[1] - prev[1])
  // 末段太短就别缩短线条，箭头直接压在终点上
  const base: Pt =
    segLen > head
      ? [end[0] - Math.cos(ang) * head, end[1] - Math.sin(ang) * head]
      : end

  const body = m.slice(0, -1).concat([base])
  const line = body.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(' ')

  const wing = head * 0.46
  const left: Pt = [
    base[0] + Math.cos(ang + Math.PI / 2) * wing,
    base[1] + Math.sin(ang + Math.PI / 2) * wing,
  ]
  const right: Pt = [
    base[0] + Math.cos(ang - Math.PI / 2) * wing,
    base[1] + Math.sin(ang - Math.PI / 2) * wing,
  ]
  const arrow = `M ${end[0].toFixed(1)} ${end[1].toFixed(1)} L ${left[0].toFixed(1)} ${left[1].toFixed(
    1
  )} L ${right[0].toFixed(1)} ${right[1].toFixed(1)} Z`

  return { line, start: m[0], arrow }
}
