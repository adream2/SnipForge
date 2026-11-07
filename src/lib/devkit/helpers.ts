/** 工具箱公用：UTF-8 / Base64 / 十六进制 / MD5 / 剪贴板。 */

export function bytesToBin(b: Uint8Array): string {
  // 一次性展开几 MB 会爆栈（apply 参数上限），按 8K 分块拼
  let s = ''
  const CHUNK = 0x2000
  for (let i = 0; i < b.length; i += CHUNK) {
    s += String.fromCharCode(...b.subarray(i, i + CHUNK))
  }
  return s
}

export function binToBytes(s: string): Uint8Array {
  const out = new Uint8Array(s.length)
  for (let i = 0; i < s.length; i++) out[i] = s.charCodeAt(i)
  return out
}

export const utf8Encode = (s: string): Uint8Array => new TextEncoder().encode(s)

export function utf8Decode(b: Uint8Array): string {
  // 非 UTF-8 字节不抛错，直接给替换符 —— 解码工具崩掉比解出问号更烦人
  return new TextDecoder('utf-8', { fatal: false }).decode(b)
}

export function bytesToHex(b: Uint8Array, upper = false): string {
  let s = ''
  for (const x of b) s += x.toString(16).padStart(2, '0')
  return upper ? s.toUpperCase() : s
}

export function hexToBytes(hex: string): Uint8Array {
  // 容忍空格、0x 前缀、逗号、换行：从各种地方粘进来的十六进制都能解
  const clean = hex.replace(/0x/gi, '').replace(/[^0-9a-fA-F]/g, '')
  if (clean.length % 2 !== 0) throw new Error('十六进制位数是奇数，可能少了一半')
  const out = new Uint8Array(clean.length / 2)
  for (let i = 0; i < out.length; i++) out[i] = parseInt(clean.slice(i * 2, i * 2 + 2), 16)
  return out
}

/* ---------------- MD5 ---------------- */

const MD5_S = [7, 12, 17, 22, 5, 9, 14, 20, 4, 11, 16, 23, 6, 10, 15, 21]
const MD5_K = (() => {
  const k = new Uint32Array(64)
  for (let i = 0; i < 64; i++) k[i] = Math.floor(Math.abs(Math.sin(i + 1)) * 2 ** 32)
  return k
})()

const rotl = (x: number, c: number): number => (x << c) | (x >>> (32 - c))

/**
 * MD5（RFC 1321）。Web Crypto 不提供 MD5，而校验老接口签名基本都要它，
 * 所以手写一份 —— 别拿它做任何安全用途，它早就不安全了。
 */
export function md5(input: string): string {
  const msg = utf8Encode(input)
  const len = msg.length
  const buf = new Uint8Array((((len + 8) >> 6) + 1) * 64)
  buf.set(msg)
  buf[len] = 0x80
  const dv = new DataView(buf.buffer)
  const bits = len * 8
  dv.setUint32(buf.length - 8, bits >>> 0, true)
  dv.setUint32(buf.length - 4, Math.floor(bits / 2 ** 32), true)

  let a0 = 0x67452301
  let b0 = 0xefcdab89
  let c0 = 0x98badcfe
  let d0 = 0x10325476
  const m = new Uint32Array(16)

  for (let off = 0; off < buf.length; off += 64) {
    for (let i = 0; i < 16; i++) m[i] = dv.getUint32(off + i * 4, true)
    let a = a0
    let b = b0
    let c = c0
    let d = d0
    for (let i = 0; i < 64; i++) {
      let f: number
      let g: number
      if (i < 16) {
        f = (b & c) | (~b & d)
        g = i
      } else if (i < 32) {
        f = (d & b) | (~d & c)
        g = (5 * i + 1) % 16
      } else if (i < 48) {
        f = b ^ c ^ d
        g = (3 * i + 5) % 16
      } else {
        f = c ^ (b | ~d)
        g = (7 * i) % 16
      }
      f = (f + a + MD5_K[i] + m[g]) >>> 0
      a = d
      d = c
      c = b
      b = (b + rotl(f, MD5_S[(i >> 4) * 4 + (i % 4)])) >>> 0
    }
    a0 = (a0 + a) >>> 0
    b0 = (b0 + b) >>> 0
    c0 = (c0 + c) >>> 0
    d0 = (d0 + d) >>> 0
  }

  return [a0, b0, c0, d0]
    .map((w) => {
      let s = ''
      for (let i = 0; i < 4; i++) s += ((w >>> (i * 8)) & 0xff).toString(16).padStart(2, '0')
      return s
    })
    .join('')
}

/** SHA-1/256/384/512，走系统实现。环境不支持时抛错，由工具页显示原因 */
export async function sha(algo: 'SHA-1' | 'SHA-256' | 'SHA-384' | 'SHA-512', text: string): Promise<string> {
  const subtle = globalThis.crypto?.subtle
  if (!subtle) throw new Error('当前环境没有 Web Crypto，无法计算哈希')
  // TextEncoder 给的是 Uint8Array<ArrayBufferLike>，TS 5.9 起和 BufferSource 对不上，
  // 只有 .buffer 是确定的 ArrayBuffer
  const bytes = utf8Encode(text)
  const buf = await subtle.digest(algo, bytes.buffer as ArrayBuffer)
  return bytesToHex(new Uint8Array(buf))
}

/** 当前时间戳字符串（毫秒）。写在这里是为了让工具页和未来别处共用一套格式 */
export const nowMs = (): number => Date.now()

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(2)} KB`
  return `${(n / 1024 / 1024).toFixed(2)} MB`
}
