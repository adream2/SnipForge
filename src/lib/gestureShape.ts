/**
 * 手势缩略图的笔画生成。
 *
 * 手势列表以前用 `↑ ↓ ← →` 拼字符串表示，两划以上的手势（`↓→↑←`）
 * 读起来是一串符号，得在脑子里把它"重放"一遍才知道长什么样。
 * 现在统一画成笔画：**一个起点圆点 + 一个结尾箭头**，一眼看懂方向。
 *
 * 三种来源：
 *   1. 方向串（`DR`）—— 每个字母走一步
 *   2. 形状字符（`S`）—— 就是那条曲线
 *   3. 用户自己画的（32 点，后端归一化过）—— 直接用
 */

export type Pt = [number, number]

const DIRS: Record<string, Pt> = {
  U: [0, -1],
  D: [0, 1],
  L: [-1, 0],
  R: [1, 0],
}

/** 把点集塞进 [0,1] 方框，保持宽高比（不拉扁，否则 L 会看成 V） */
function fit(pts: Pt[]): Pt[] {
  if (pts.length === 0) return []
  const xs = pts.map((p) => p[0])
  const ys = pts.map((p) => p[1])
  const minX = Math.min(...xs)
  const maxX = Math.max(...xs)
  const minY = Math.min(...ys)
  const maxY = Math.max(...ys)
  const w = maxX - minX
  const h = maxY - minY
  const s = Math.max(w, h, 1e-6)
  return pts.map(([x, y]) => [
    (x - minX + (s - w) / 2) / s,
    (y - minY + (s - h) / 2) / s,
  ])
}

/** 字体式 S：右上起笔、左下收笔，和 Rust 侧模板同形 */
function sStroke(): Pt[] {
  const p0: Pt = [0.6, -1]
  const p1: Pt = [-1, -0.6]
  const p2: Pt = [1, 0.6]
  const p3: Pt = [-0.6, 1]
  const out: Pt[] = []
  const n = 28
  for (let i = 0; i <= n; i++) {
    const t = i / n
    const u = 1 - t
    const b0 = u * u * u
    const b1 = 3 * u * u * t
    const b2 = 3 * u * t * t
    const b3 = t * t * t
    out.push([
      b0 * p0[0] + b1 * p1[0] + b2 * p2[0] + b3 * p3[0],
      b0 * p0[1] + b1 * p1[1] + b2 * p2[1] + b3 * p3[1],
    ])
  }
  return out
}

export function patternToStroke(pattern: string): Pt[] {
  const p = (pattern || '').toUpperCase()
  if (!p) return []
  if (p === 'S') return fit(sStroke())

  let x = 0
  let y = 0
  const pts: Pt[] = [[0, 0]]
  for (const ch of p) {
    const d = DIRS[ch]
    if (!d) continue
    x += d[0]
    y += d[1]
    pts.push([x, y])
  }
  return pts.length > 1 ? fit(pts) : []
}
