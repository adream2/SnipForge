/**
 * 生成应用图标与托盘图标，产物全部进 src-tauri/icons/。
 * 手写 PNG 编码器而不是拉 sharp/canvas：只需要几个固定尺寸的小图，
 * 为这点需求引入 native 依赖不值当。跑一次即可，图标改了就重跑。
 *
 *   node scripts/make-icons.mjs
 */
import { deflateSync } from 'node:zlib'
import { writeFileSync, mkdirSync } from 'node:fs'
import { join, dirname } from 'node:path'
import { fileURLToPath } from 'node:url'

const root = join(dirname(fileURLToPath(import.meta.url)), '..')

const crcTable = (() => {
  const t = new Int32Array(256)
  for (let n = 0; n < 256; n++) {
    let c = n
    for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1
    t[n] = c
  }
  return t
})()

function crc32(buf) {
  let c = 0xffffffff
  for (let i = 0; i < buf.length; i++) c = crcTable[(c ^ buf[i]) & 0xff] ^ (c >>> 8)
  return (c ^ 0xffffffff) >>> 0
}

function chunk(type, data) {
  const len = Buffer.alloc(4)
  len.writeUInt32BE(data.length)
  const typeBuf = Buffer.from(type, 'ascii')
  const crcBuf = Buffer.alloc(4)
  crcBuf.writeUInt32BE(crc32(Buffer.concat([typeBuf, data])))
  return Buffer.concat([len, typeBuf, data, crcBuf])
}

function encodePng(width, height, rgba) {
  const raw = Buffer.alloc((width * 4 + 1) * height)
  for (let y = 0; y < height; y++) {
    raw[y * (width * 4 + 1)] = 0 // filter: none
    rgba.copy(raw, y * (width * 4 + 1) + 1, y * width * 4, (y + 1) * width * 4)
  }
  const ihdr = Buffer.alloc(13)
  ihdr.writeUInt32BE(width, 0)
  ihdr.writeUInt32BE(height, 4)
  ihdr[8] = 8 // bit depth
  ihdr[9] = 6 // color type RGBA
  ihdr[10] = 0
  ihdr[11] = 0
  ihdr[12] = 0
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', ihdr),
    chunk('IDAT', deflateSync(raw, { level: 9 })),
    chunk('IEND', Buffer.alloc(0))
  ])
}

/** 有符号距离场：圆角矩形，用来做抗锯齿 */
function sdRoundRect(px, py, cx, cy, hw, hh, r) {
  const qx = Math.abs(px - cx) - (hw - r)
  const qy = Math.abs(py - cy) - (hh - r)
  const ax = Math.max(qx, 0)
  const ay = Math.max(qy, 0)
  return Math.sqrt(ax * ax + ay * ay) + Math.min(Math.max(qx, qy), 0) - r
}

function sdSegment(px, py, ax, ay, bx, by) {
  const pax = px - ax
  const pay = py - ay
  const bax = bx - ax
  const bay = by - ay
  const h = Math.max(0, Math.min(1, (pax * bax + pay * bay) / (bax * bax + bay * bay)))
  const dx = pax - bax * h
  const dy = pay - bay * h
  return Math.sqrt(dx * dx + dy * dy)
}

const lerp = (a, b, t) => a + (b - a) * t
const clamp01 = (v) => Math.max(0, Math.min(1, v))
const smooth = (d, aa) => clamp01(0.5 - d / aa)

/**
 * 图标构图：圆角底 + 渐变 + 白色选区角标 + 斜向剪刀线。
 * 一眼能看出是"框选截图"这件事。
 */
function drawIcon(size) {
  const s = size
  const px = Buffer.alloc(s * s * 4)
  const aa = Math.max(1.1, s / 64)
  const margin = s * 0.055
  const radius = s * 0.235

  for (let y = 0; y < s; y++) {
    for (let x = 0; x < s; x++) {
      const i = (y * s + x) * 4
      const cx = x + 0.5
      const cy = y + 0.5

      const dBase = sdRoundRect(cx, cy, s / 2, s / 2, s / 2 - margin, s / 2 - margin, radius)
      const aBase = smooth(dBase, aa)
      if (aBase <= 0) continue

      // 渐变：左上靛蓝 → 右下亮紫
      const t = clamp01((x / s) * 0.55 + (y / s) * 0.45)
      let r = lerp(92, 168, t)
      let g = lerp(84, 130, t)
      let b = lerp(231, 255, t)

      // 顶部高光
      const gloss = clamp01(1 - y / (s * 0.55)) * 0.14
      r = lerp(r, 255, gloss)
      g = lerp(g, 255, gloss)
      b = lerp(b, 255, gloss)

      // 选区角标：四个 L 形角
      const pad = s * 0.235
      const arm = s * 0.135
      const thick = Math.max(1.4, s * 0.036)
      const l = pad
      const rgt = s - pad
      const tp = pad
      const btm = s - pad

      let dInk = Infinity
      // 左上
      dInk = Math.min(dInk, sdSegment(cx, cy, l, tp + arm, l, tp), sdSegment(cx, cy, l, tp, l + arm, tp))
      // 右上
      dInk = Math.min(dInk, sdSegment(cx, cy, rgt - arm, tp, rgt, tp), sdSegment(cx, cy, rgt, tp, rgt, tp + arm))
      // 左下
      dInk = Math.min(dInk, sdSegment(cx, cy, l, btm - arm, l, btm), sdSegment(cx, cy, l, btm, l + arm, btm))
      // 右下
      dInk = Math.min(dInk, sdSegment(cx, cy, rgt - arm, btm, rgt, btm), sdSegment(cx, cy, rgt, btm, rgt, btm - arm))

      // 中间的斜向"剪刀"切口
      dInk = Math.min(
        dInk,
        sdSegment(cx, cy, s * 0.38, s * 0.62, s * 0.62, s * 0.38) - thick * 0.45
      )

      const aInk = smooth(dInk - thick / 2, aa)
      if (aInk > 0) {
        r = lerp(r, 255, aInk)
        g = lerp(g, 255, aInk)
        b = lerp(b, 255, aInk)
      }

      px[i] = Math.round(r)
      px[i + 1] = Math.round(g)
      px[i + 2] = Math.round(b)
      px[i + 3] = Math.round(aBase * 255)
    }
  }
  return encodePng(s, s, px)
}

mkdirSync(join(root, 'src-tauri', 'icons'), { recursive: true })

const icon256 = drawIcon(256)

/* ---------------- Tauri 需要的图标集 ---------------- */

const icoSizes = [16, 24, 32, 48, 64, 128, 256]
const pngs = icoSizes.map((size) => ({ size, data: drawIcon(size) }))

function encodeIco(images) {
  const count = images.length
  const header = Buffer.alloc(6)
  header.writeUInt16LE(0, 0)
  header.writeUInt16LE(1, 2)
  header.writeUInt16LE(count, 4)
  const entries = []
  let offset = 6 + count * 16
  const blobs = []
  for (const img of images) {
    const e = Buffer.alloc(16)
    e[0] = img.size >= 256 ? 0 : img.size
    e[1] = img.size >= 256 ? 0 : img.size
    e[2] = 0
    e[3] = 0
    e.writeUInt16LE(1, 4)
    e.writeUInt16LE(32, 6)
    e.writeUInt32LE(img.data.length, 8)
    e.writeUInt32LE(offset, 12)
    offset += img.data.length
    entries.push(e)
    blobs.push(img.data)
  }
  return Buffer.concat([header, ...entries, ...blobs])
}

const tauriIcons = join(root, 'src-tauri', 'icons')
writeFileSync(join(tauriIcons, 'icon.ico'), encodeIco(pngs))
writeFileSync(join(tauriIcons, 'icon.png'), icon256)
writeFileSync(join(tauriIcons, '32x32.png'), drawIcon(32))
writeFileSync(join(tauriIcons, '128x128.png'), drawIcon(128))
writeFileSync(join(tauriIcons, '128x128@2x.png'), icon256)
// 托盘图标用彩色版：mono 白色在浅色任务栏上等于隐形
writeFileSync(join(tauriIcons, 'tray.png'), drawIcon(32))
for (const s of [16, 24, 48, 64, 256]) {
  writeFileSync(join(tauriIcons, `${s}x${s}.png`), drawIcon(s))
}

console.log('图标已生成: src-tauri/icons/')
