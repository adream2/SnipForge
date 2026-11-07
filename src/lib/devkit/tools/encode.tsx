/** 开发者工具箱 · 编码转换 */

import { useMemo, useState } from 'react'
import type { Tool } from '../types'
import { optBool, optStr } from '../types'
import {
  binToBytes,
  bytesToBin,
  bytesToHex,
  hexToBytes,
  utf8Decode,
  utf8Encode
} from '../helpers'
import { CopyBtn, OutBox, PanelSection, ResultRow } from '../ui'

const b64Decode = (raw: string): Uint8Array => {
  // 粘进来的 base64 常常带换行、URL-safe 的 -_、或者缺 padding
  const s = raw.replace(/\s+/g, '').replace(/-/g, '+').replace(/_/g, '/')
  const pad = s.length % 4 ? '='.repeat(4 - (s.length % 4)) : ''
  return binToBytes(atob(s + pad))
}

const b64Encode = (b: Uint8Array): string => btoa(bytesToBin(b))

/* ---------------- JWT ---------------- */

function JwtPanel() {
  const [token, setToken] = useState('')

  const parsed = useMemo(() => {
    const t = token.trim()
    if (!t) return null
    const parts = t.split('.')
    if (parts.length < 2) return { error: '不是 JWT：至少要 header.payload 两段' }
    try {
      const dec = (p: string) => JSON.parse(utf8Decode(b64Decode(p))) as Record<string, unknown>
      return { header: dec(parts[0]), payload: dec(parts[1]), sig: parts[2] ?? '', parts: parts.length }
    } catch (e) {
      return { error: `解析失败：${e instanceof Error ? e.message : String(e)}` }
    }
  }, [token])

  const timeClaims = useMemo(() => {
    if (!parsed || 'error' in parsed) return []
    const p = parsed.payload
    const out: Array<{ key: string; value: string; past: boolean }> = []
    const now = Date.now() / 1000
    for (const k of ['iat', 'nbf', 'exp']) {
      const v = p[k]
      if (typeof v !== 'number') continue
      out.push({
        key: k === 'iat' ? '签发' : k === 'nbf' ? '生效' : '过期',
        value: new Date(v * 1000).toLocaleString(),
        past: v < now
      })
    }
    return out
  }, [parsed])

  const expired = useMemo(() => {
    if (!parsed || 'error' in parsed) return null
    const exp = parsed.payload.exp
    return typeof exp === 'number' ? exp * 1000 < Date.now() : null
  }, [parsed])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <textarea
        className="input"
        rows={4}
        placeholder="粘贴 JWT（eyJhbGciOi…）"
        value={token}
        onChange={(e) => setToken(e.target.value)}
      />
      {parsed && 'error' in parsed && (
        <div style={{ fontSize: 12, color: 'var(--danger, #d64545)' }}>{parsed.error}</div>
      )}
      {parsed && !('error' in parsed) && (
        <>
          <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap' }}>
            <span className="tag" style={{ height: 19, fontSize: 10.5 }}>
              {String(parsed.header.alg ?? '未知算法')}
            </span>
            <span className="tag" style={{ height: 19, fontSize: 10.5 }}>
              {parsed.parts} 段{parsed.parts === 3 ? '' : '（无签名）'}
            </span>
            {expired === true && (
              <span className="tag danger" style={{ height: 19, fontSize: 10.5 }}>
                已过期
              </span>
            )}
            {expired === false && (
              <span className="tag success" style={{ height: 19, fontSize: 10.5 }}>
                未过期
              </span>
            )}
            <div style={{ flex: 1 }} />
            <span style={{ fontSize: 11, color: 'var(--text-3)' }}>只解码，不验签</span>
          </div>

          {timeClaims.length > 0 && (
            <div>
              {timeClaims.map((c) => (
                <div key={c.key} style={{ fontSize: 12, color: 'var(--text-2)', lineHeight: 1.9 }}>
                  <span style={{ color: 'var(--text-3)', marginRight: 8 }}>{c.key}</span>
                  {c.value}
                </div>
              ))}
            </div>
          )}

          <PanelSection title="Header" right={<CopyBtn text={JSON.stringify(parsed.header, null, 2)} />}>
            <pre
              style={{
                margin: 0,
                padding: 10,
                borderRadius: 7,
                background: 'var(--surface-3, #f5f5f7)',
                fontFamily: 'var(--mono)',
                fontSize: 11.5,
                lineHeight: 1.6,
                overflowX: 'auto'
              }}
            >
              {JSON.stringify(parsed.header, null, 2)}
            </pre>
          </PanelSection>

          <PanelSection title="Payload" right={<CopyBtn text={JSON.stringify(parsed.payload, null, 2)} />}>
            <pre
              style={{
                margin: 0,
                padding: 10,
                borderRadius: 7,
                background: 'var(--surface-3, #f5f5f7)',
                fontFamily: 'var(--mono)',
                fontSize: 11.5,
                lineHeight: 1.6,
                overflowX: 'auto'
              }}
            >
              {JSON.stringify(parsed.payload, null, 2)}
            </pre>
          </PanelSection>

          {parsed.sig && (
            <PanelSection title="Signature" right={<CopyBtn text={parsed.sig} />}>
              <code style={{ fontFamily: 'var(--mono)', fontSize: 11, color: 'var(--text-3)', wordBreak: 'break-all' }}>
                {parsed.sig}
              </code>
            </PanelSection>
          )}
        </>
      )}
    </div>
  )
}

/* ---------------- 工具表 ---------------- */

export const encodeTools: Tool[] = [
  {
    kind: 'transform',
    id: 'base64',
    name: 'Base64',
    desc: '文本与 Base64 互转，支持 URL-safe 与按 76 字符折行',
    dirs: { encode: '编码', decode: '解码' },
    placeholder: '输入要编码的文本，或粘贴要解码的 Base64',
    options: [
      { key: 'urlSafe', label: 'URL-safe（+/ 换成 -_，去掉 =）', kind: 'switch', default: true },
      { key: 'wrap', label: '每 76 字符换行', kind: 'switch', default: false }
    ],
    run: (input, o, dir) => {
      if (dir === 'encode') {
        let out = b64Encode(utf8Encode(input))
        if (optBool(o, 'urlSafe')) out = out.replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '')
        if (optBool(o, 'wrap')) out = out.replace(/(.{76})/g, '$1\n').trim()
        return out
      }
      return utf8Decode(b64Decode(input))
    }
  },
  {
    kind: 'transform',
    id: 'url',
    name: 'URL 编解码',
    desc: 'encodeURIComponent / encodeURI 两种粒度',
    dirs: { encode: '编码', decode: '解码' },
    placeholder: 'https://example.com/?q=中文 或 %E4%B8%AD%E6%96%87',
    options: [
      {
        key: 'mode',
        label: '粒度',
        kind: 'select',
        default: 'component',
        options: [
          { label: '组件（连 / ? # 一起编）', value: 'component' },
          { label: '整段 URL（保留 : / ? # &）', value: 'full' }
        ]
      }
    ],
    run: (input, o, dir) => {
      const component = optStr(o, 'mode') === 'component'
      try {
        if (dir === 'encode') return component ? encodeURIComponent(input) : encodeURI(input)
        return component ? decodeURIComponent(input) : decodeURI(input)
      } catch {
        throw new Error('百分号后面的两位不是合法十六进制，这段不是有效的 URL 编码')
      }
    }
  },
  {
    kind: 'transform',
    id: 'hex',
    name: 'Hex',
    desc: '文本与十六进制互转（按 UTF-8 字节）',
    dirs: { encode: '转 Hex', decode: '还原' },
    placeholder: '文本，或 48 65 6c 6c 6f / 0x48656c6c6f',
    options: [
      { key: 'upper', label: '大写字母', kind: 'switch', default: false },
      {
        key: 'sep',
        label: '分隔',
        kind: 'select',
        default: '',
        options: [
          { label: '无', value: '' },
          { label: '空格', value: ' ' },
          { label: '0x 前缀', value: '0x' }
        ]
      }
    ],
    run: (input, o, dir) => {
      if (dir === 'encode') {
        const hex = bytesToHex(utf8Encode(input), optBool(o, 'upper'))
        const sep = optStr(o, 'sep')
        if (sep === '0x') return (hex.match(/.{2}/g) ?? []).map((p) => '0x' + p).join(' ')
        if (sep) return (hex.match(/.{2}/g) ?? []).join(sep)
        return hex
      }
      return utf8Decode(hexToBytes(input))
    }
  },
  {
    kind: 'transform',
    id: 'html',
    name: 'HTML 实体',
    desc: '转义 < > & " \'，或把实体还原成字符',
    dirs: { encode: '转义', decode: '还原' },
    placeholder: '<div class="a">中文 & 符号</div>',
    run: (input, _o, dir) => {
      if (dir === 'encode') {
        return input
          .replace(/&/g, '&amp;')
          .replace(/</g, '&lt;')
          .replace(/>/g, '&gt;')
          .replace(/"/g, '&quot;')
          .replace(/'/g, '&#39;')
      }
      // 用未挂载的 textarea 解码：浏览器自带的实体表比手写映射全，
      // 而且这个节点不进 DOM，不会执行任何东西
      const el = document.createElement('textarea')
      el.innerHTML = input
      return el.value
    }
  },
  {
    kind: 'transform',
    id: 'unicode',
    name: 'Unicode 转义',
    desc: '中文 / emoji 与 \\uXXXX、\\u{...}、\\xXX 互转',
    dirs: { encode: '转义', decode: '还原' },
    placeholder: '含中文或 emoji 的文本，或 \\u4e2d\\u6587',
    options: [
      { key: 'nonAsciiOnly', label: '只转义非 ASCII（保留英文和符号）', kind: 'switch', default: true }
    ],
    run: (input, o, dir) => {
      if (dir === 'encode') {
        const only = optBool(o, 'nonAsciiOnly')
        let out = ''
        for (const ch of input) {
          const code = ch.codePointAt(0) ?? 0
          if (code < 128 && only) {
            out += ch
            continue
          }
          if (code > 0xffff) {
            // 星平面字符要写成代理对，\u{1F600} 不是所有语言都认
            for (let i = 0; i < ch.length; i++) {
              out += '\\u' + ch.charCodeAt(i).toString(16).padStart(4, '0')
            }
          } else {
            out += '\\u' + code.toString(16).padStart(4, '0')
          }
        }
        return out
      }
      return input
        .replace(/\\u\{([0-9a-fA-F]+)\}/g, (_m, h: string) => String.fromCodePoint(parseInt(h, 16)))
        .replace(/\\u([0-9a-fA-F]{4})/g, (_m, h: string) => String.fromCharCode(parseInt(h, 16)))
        .replace(/\\x([0-9a-fA-F]{2})/g, (_m, h: string) => String.fromCharCode(parseInt(h, 16)))
    }
  },
  {
    kind: 'panel',
    id: 'jwt',
    name: 'JWT 解析',
    desc: '拆 header / payload，标出签发、过期时间。不做验签',
    Panel: JwtPanel
  }
]

/** 供其它工具复用（hash 面板要对字节做编码时用得到） */
export { b64Decode, b64Encode }
