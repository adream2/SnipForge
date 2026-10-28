/**
 * 上传前的图片预处理（压缩 / 转格式 / 限边长）。
 *
 * 为什么放在渲染进程而不是后端：
 *   1. **有损 WebP 只有这边能做**。`image` crate 的 WebP 编码器是无损的，
 *      体积优势全丢；浏览器 canvas 的 `toDataURL('image/webp', q)` 是真有损。
 *   2. 顺带借力 WebView 的高质量重采样，比自己写双线性好。
 *   后端只管上传，不掺和像素。
 */

import { api } from './api'

export interface CompressOpts {
  enabled: boolean
  /** keep | webp | jpeg | png */
  format: string
  quality: number
  /** 最长边上限（像素），0 = 不限 */
  maxEdge: number
}

export interface PreparedImage {
  dataUrl: string
  bytes: number
  width: number
  height: number
  /** 实际 mime，例如 image/webp */
  format: string
  /** 原图信息，用来展示"省了多少" */
  srcBytes: number
  srcWidth: number
  srcHeight: number
  srcFormat: string
  /** 是否真的重新编码过（没变就说明没必要） */
  changed: boolean
}

const MIME: Record<string, string> = {
  webp: 'image/webp',
  jpeg: 'image/jpeg',
  png: 'image/png'
}

/** data URL 的实际字节数（base64 膨胀 4/3，减掉那个比例） */
export function dataUrlBytes(dataUrl: string): number {
  const i = dataUrl.indexOf(',')
  return i < 0 ? 0 : Math.round((dataUrl.length - i - 1) * 0.75)
}

export function mimeOf(dataUrl: string): string {
  const m = /^data:([^;,]+)/.exec(dataUrl)
  return m?.[1] ?? 'image/png'
}

export function extOf(mime: string): string {
  if (mime === 'image/jpeg') return 'jpg'
  if (mime === 'image/webp') return 'webp'
  if (mime === 'image/gif') return 'gif'
  if (mime === 'image/bmp') return 'bmp'
  return 'png'
}

function loadImage(src: string): Promise<HTMLImageElement> {
  return new Promise((resolve, reject) => {
    const img = new Image()
    img.onload = () => resolve(img)
    img.onerror = () => reject(new Error('图片解码失败'))
    img.src = src
  })
}

/**
 * 按当前「图床 → 配置」里的压缩设置处理一张图。
 *
 * 给编辑器、图片工具这些"顺手传一下"的入口用 —— 它们拿不到 settings 对象，
 * 与其把配置一层层传下去，不如就地读一次（本地 IPC，几毫秒）。
 */
export async function prepareWithSettings(src: string): Promise<PreparedImage> {
  try {
    const { settings } = await api.settingsGet()
    const u = settings.uploader
    return await prepareForUpload(src, {
      enabled: u.compress,
      format: u.compressFormat,
      quality: u.compressQuality,
      maxEdge: u.compressMaxEdge
    })
  } catch {
    // 读配置失败就别拦着用户上传，按原图走
    return prepareForUpload(src, { enabled: false, format: 'keep', quality: 90, maxEdge: 0 })
  }
}

export async function prepareForUpload(src: string, opt: CompressOpts): Promise<PreparedImage> {
  const img = await loadImage(src)
  const srcW = img.naturalWidth || img.width
  const srcH = img.naturalHeight || img.height
  const srcBytes = dataUrlBytes(src)
  const srcFormat = mimeOf(src)

  const base: PreparedImage = {
    dataUrl: src,
    bytes: srcBytes,
    width: srcW,
    height: srcH,
    format: srcFormat,
    srcBytes,
    srcWidth: srcW,
    srcHeight: srcH,
    srcFormat,
    changed: false
  }

  if (!opt.enabled) return base

  const maxEdge = opt.maxEdge || 0
  const ratio = maxEdge > 0 ? Math.min(1, maxEdge / Math.max(srcW, srcH)) : 1
  const w = Math.max(1, Math.round(srcW * ratio))
  const h = Math.max(1, Math.round(srcH * ratio))
  const targetMime = opt.format === 'keep' ? srcFormat : MIME[opt.format] ?? srcFormat

  // 既不用缩放也不用转格式 → 原样上传，别白折腾一遍编码掉质量
  if (ratio === 1 && targetMime === srcFormat) return base

  const canvas = document.createElement('canvas')
  canvas.width = w
  canvas.height = h
  const ctx = canvas.getContext('2d')
  if (!ctx) return base

  // JPEG 没有透明通道，先铺白底，否则透明区域会变黑
  if (targetMime === 'image/jpeg') {
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(0, 0, w, h)
  }
  ctx.imageSmoothingEnabled = true
  ctx.imageSmoothingQuality = 'high'
  ctx.drawImage(img, 0, 0, w, h)

  // PNG 忽略 quality；webp/jpeg 用 0-1 的质量
  const q = targetMime === 'image/png' ? undefined : Math.min(100, Math.max(1, opt.quality)) / 100
  const out = canvas.toDataURL(targetMime, q)

  // 转成 PNG 反而更大的情况很常见（照片 → PNG），这种情况回退原图更划算
  const outBytes = dataUrlBytes(out)
  if (outBytes >= srcBytes && ratio === 1) return base

  return {
    dataUrl: out,
    bytes: outBytes,
    width: w,
    height: h,
    format: mimeOf(out),
    srcBytes,
    srcWidth: srcW,
    srcHeight: srcH,
    srcFormat,
    changed: true
  }
}
