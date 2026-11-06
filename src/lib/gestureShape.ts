/**
 * 手势缩略图的笔画生成。
 *
 * 手势列表以前用 `↑ ↓ ← →` 拼字符串表示，两划以上的手势（`↓→↑←`）
 * 读起来是一串符号，得在脑子里把它"重放"一遍才知道长什么样。
 * 现在统一画成笔画：**一个起点圆点 + 一个结尾箭头**，一眼看懂方向。
 *
 * 三种来源：
 *   1. 方向串（`DR`）—— 每个字母走一步
 *   2. 形状字符（`S` / `H`）—— 按字面画出来
 *   3. 用户自己画的（32 点，后端归一化过）—— 直接用
 */

export type Pt = [number, number]

const DIRS: Record<string, Pt> = {
  U: [0, -1],
  D: [0, 1],
  L: [-1, 0],
  R: [1, 0],
  // 斜向：和 Rust 侧 `dir8` 同一套四十五度扇区
  UL: [-0.7071, -0.7071],
  UR: [0.7071, -0.7071],
  DL: [-0.7071, 0.7071],
  DR: [0.7071, 0.7071],
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

/** 一笔画出来的 H：左腿下 → 回中线 → 中横 → 右腿上下收笔 */
function hStroke(): Pt[] {
  return fit([
    [-0.5, -1],
    [-0.5, 1],
    [-0.5, 0],
    [0.5, 0],
    [0.5, -1],
    [0.5, 1],
  ])
}

/**
 * 方向串 → 折线。
 *
 * token 用逗号分隔（`D,R`）；连写的（`DRUR`）按两字符优先切 —— 和
 * Rust 的 `normalize` 保持同一套规则，否则缩略图会和实际识别不一致。
 */
/**
 * 位移 → 8 方向。和后端 `dir8` 同一套规则：主方向优先。
 *
 * 一轴投影超过另一轴 1.6 倍（约 ±32°）就算主方向，只有真正接近 45° 才判斜向。
 * 纯 atan2 的 45° 扇区主方向容差只有 ±22.5°，手稍偏一点就成了斜向。
 */
export function dir8(dx: number, dy: number): string {
  const ax = Math.abs(dx)
  const ay = Math.abs(dy)
  const RATIO = 1.6
  if (ax >= RATIO * ay) return dx > 0 ? 'R' : 'L'
  if (ay >= RATIO * ax) return dy > 0 ? 'D' : 'U'
  if (dx > 0) return dy > 0 ? 'DR' : 'UR'
  return dy > 0 ? 'DL' : 'UL'
}

export function patternToStroke(pattern: string): Pt[] {
  const raw = (pattern || '').toUpperCase()
  const list: string[] = []
  for (const t of raw.split(/[,\s]+/).filter(Boolean)) {
    if (DIRS[t]) {
      list.push(t)
      continue
    }
    let i = 0
    while (i < t.length) {
      const two = t.slice(i, i + 2)
      if (two.length === 2 && DIRS[two]) {
        list.push(two)
        i += 2
        continue
      }
      const one = t[i]
      if (DIRS[one]) list.push(one)
      i += 1
    }
  }
  if (!list.length) return []

  let x = 0
  let y = 0
  const pts: Pt[] = [[0, 0]]
  for (const t of list) {
    const d = DIRS[t]
    x += d[0]
    y += d[1]
    pts.push([x, y])
  }
  return pts.length > 1 ? fit(pts) : []
}

/** 按弧长等距重采样（缩略图判重用） */
function resample(pts: Pt[], n: number): Pt[] {
  if (pts.length < 2) return pts.slice()
  const seg: number[] = [0]
  let total = 0
  for (let i = 1; i < pts.length; i++) {
    total += Math.hypot(pts[i][0] - pts[i - 1][0], pts[i][1] - pts[i - 1][1])
    seg.push(total)
  }
  if (total < 1e-6) return [pts[0], pts[0]]
  const step = total / (n - 1)
  const out: Pt[] = []
  let k = 0
  for (let i = 0; i < n; i++) {
    const target = i * step
    while (k + 2 < pts.length && seg[k + 1] < target) k++
    const a = seg[k]
    const b = seg[k + 1]
    const t = b > a ? Math.min(1, Math.max(0, (target - a) / (b - a))) : 0
    out.push([
      pts[k][0] + (pts[k + 1][0] - pts[k][0]) * t,
      pts[k][1] + (pts[k + 1][1] - pts[k][1]) * t,
    ])
  }
  return out
}

/** 判重用的签名：16 点 + 居中 + 等比缩放到长边 1（只忽略大小，不忽略比例/方向） */
function signature(pts: Pt[]): Pt[] {
  const rs = resample(pts, 16)
  const xs = rs.map((p) => p[0])
  const ys = rs.map((p) => p[1])
  const minX = Math.min(...xs)
  const maxX = Math.max(...xs)
  const minY = Math.min(...ys)
  const maxY = Math.max(...ys)
  const s = Math.max(maxX - minX, maxY - minY, 1e-6)
  const cx = (minX + maxX) / 2
  const cy = (minY + maxY) / 2
  return rs.map(([x, y]) => [(x - cx) / s, (y - cy) / s])
}

function meanDist(a: Pt[], b: Pt[]): number {
  let sum = 0
  for (let i = 0; i < a.length; i++) sum += Math.hypot(a[i][0] - b[i][0], a[i][1] - b[i][1])
  return sum / a.length
}

/**
 * 两条笔画"画出来是不是同一个样子"。
 *
 * 只有**闭合**的图形才比较反方向：方框正着画反着画是同一个图形，
 * 而直线 `←` 和 `→` 反过来比也完全重合，那样会把"后退/前进"标成重复。
 */
export function sameStroke(a: Pt[], b: Pt[]): boolean {
  if (a.length < 2 || b.length < 2) return false
  const sa = signature(a)
  const sb = signature(b)
  const closed = (s: Pt[]) => Math.hypot(s[0][0] - s[s.length - 1][0], s[0][1] - s[s.length - 1][1]) < 0.12
  let d = meanDist(sa, sb)
  if (closed(sa) && closed(sb)) {
    d = Math.min(d, meanDist(sa, sb.slice().reverse()))
  }
  return d < 0.07
}

/** 一条绑定该画成什么：有自绘笔画就用它，否则按方向串生成 */
export function strokeOf(b: { pattern?: string; points?: number[][] }): Pt[] {
  if (b.points && b.points.length >= 2) return b.points.map((p) => [p[0], p[1]] as Pt)
  return patternToStroke(b.pattern ?? '')
}
