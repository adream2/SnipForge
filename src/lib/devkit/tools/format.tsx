/** 开发者工具箱 · 格式化 / 校验 */

import { useMemo, useState } from 'react'
import type { OptionValues, Tool } from '../types'
import { optBool, optStr } from '../types'
import { CopyBtn, PanelSection } from '../ui'

/* ---------------- JSON ---------------- */

const sortDeep = (v: unknown): unknown => {
  if (Array.isArray(v)) return v.map(sortDeep)
  if (v && typeof v === 'object') {
    const src = v as Record<string, unknown>
    const out: Record<string, unknown> = {}
    for (const k of Object.keys(src).sort()) out[k] = sortDeep(src[k])
    return out
  }
  return v
}

/** 把 V8 的 "at position 123" 翻成人和编辑器都认的行列 */
function jsonError(e: unknown, src: string): string {
  const raw = e instanceof Error ? e.message : String(e)
  const m = /position (\d+)/i.exec(raw)
  if (!m) return raw
  const pos = Math.min(Number(m[1]), src.length)
  const before = src.slice(0, pos)
  const line = before.split('\n').length
  const col = pos - before.lastIndexOf('\n')
  return `${raw}\n第 ${line} 行第 ${col} 列`
}

const indentOf = (o: OptionValues): number | string => {
  const v = optStr(o, 'indent')
  return v === 'tab' ? '\t' : Number(v)
}

/* ---------------- XML / SQL ---------------- */

function formatXml(src: string): string {
  const tokens =
    src
      .replace(/\r\n/g, '\n')
      .trim()
      .match(/<!--[\s\S]*?-->|<!\[CDATA\[[\s\S]*?\]\]>|<\?[\s\S]*?\?>|<\/?[^>]+>|[^<]+/g) ?? []
  const lines: string[] = []
  let depth = 0
  for (const raw of tokens) {
    const t = raw.trim()
    if (!t) continue
    if (/^<\//.test(t)) {
      depth = Math.max(0, depth - 1)
      lines.push('  '.repeat(depth) + t)
    } else if (/^</.test(t)) {
      lines.push('  '.repeat(depth) + t)
      // 自闭合、注释、CDATA、声明都不进下一层
      if (!/\/>$/.test(t) && !/^<[!?]/.test(t)) depth++
    } else {
      lines.push('  '.repeat(depth) + t)
    }
  }
  return lines.join('\n')
}

/** 长的写在前面：正则交替是"先匹配先赢"，GROUP BY 不能被 BY 抢走 */
const SQL_BREAK = [
  'LEFT OUTER JOIN',
  'RIGHT OUTER JOIN',
  'FULL OUTER JOIN',
  'LEFT JOIN',
  'RIGHT JOIN',
  'INNER JOIN',
  'CROSS JOIN',
  'UNION ALL',
  'GROUP BY',
  'ORDER BY',
  'INSERT INTO',
  'DELETE FROM',
  'SELECT',
  'FROM',
  'WHERE',
  'HAVING',
  'LIMIT',
  'OFFSET',
  'VALUES',
  'UPDATE',
  'JOIN',
  'UNION',
  'SET',
  'ON',
  'AND',
  'OR'
]

const SQL_RE = new RegExp(`\\b(${SQL_BREAK.map((k) => k.replace(/ /g, '\\s+')).join('|')})\\b`, 'gi')

function formatSql(src: string): string {
  // 先把空白折叠掉，再在关键字前断行；引号里的内容不动（够用的近似，
  // 不是完整的 SQL 词法器 —— 工具箱里不该塞一个 parser）
  const flat = src
    .replace(/\r\n/g, '\n')
    .replace(/\s+/g, ' ')
    .trim()
  return flat
    .replace(SQL_RE, (m) => '\n' + m.toUpperCase().replace(/\s+/g, ' '))
    .split('\n')
    .map((l) => l.trim())
    .filter(Boolean)
    .join('\n')
}

/* ---------------- 正则测试器 ---------------- */

interface Hit {
  index: number
  text: string
  /** 按序号排的捕获组；没参与的组在 JS 里是 undefined */
  groups: Array<string | undefined>
  /** 具名组，单独列出来方便对 API 文档 */
  named: Array<{ name: string; value: string | undefined }>
}

const toHit = (m: RegExpExecArray): Hit => ({
  index: m.index,
  text: m[0],
  groups: m.slice(1),
  named: m.groups
    ? Object.keys(m.groups).map((k) => ({ name: k, value: m.groups?.[k] }))
    : []
})

const FLAGS = ['g', 'i', 'm', 's', 'u'] as const

function RegexPanel() {
  const [pattern, setPattern] = useState('')
  const [flags, setFlags] = useState<string>('g')
  const [text, setText] = useState('')
  const [replacement, setReplacement] = useState('')

  const { re, error, hits } = useMemo(() => {
    const p = pattern
    if (!p) return { re: null, error: null as string | null, hits: [] as Hit[] }
    let compiled: RegExp
    try {
      compiled = new RegExp(p, flags)
    } catch (e) {
      return { re: null, error: e instanceof Error ? e.message : String(e), hits: [] as Hit[] }
    }
    const out: Hit[] = []
    if (!text) return { re: compiled, error: null, hits: out }
    if (compiled.global) {
      let m: RegExpExecArray | null
      let guard = 0
      while ((m = compiled.exec(text)) !== null) {
        out.push(toHit(m))
        // 零宽匹配（如 ^ 或 \b）不会自己推进 lastIndex，不手动加就死循环
        if (m[0] === '') compiled.lastIndex++
        if (out.length >= 500 || ++guard > 5000) break
      }
    } else {
      const m = compiled.exec(text)
      if (m) out.push(toHit(m))
    }
    return { re: compiled, error: null, hits: out }
  }, [pattern, flags, text])

  const replaced = useMemo(() => {
    if (!re || !replacement) return ''
    try {
      return text.replace(re, replacement)
    } catch (e) {
      return `替换失败：${e instanceof Error ? e.message : String(e)}`
    }
  }, [re, text, replacement])

  // 高亮：把命中区间切成 "普通 / 命中 / 普通 / ..."
  const segments = useMemo(() => {
    if (!text) return []
    const out: Array<{ s: string; hit: boolean }> = []
    let cursor = 0
    for (const h of hits) {
      if (h.index > cursor) out.push({ s: text.slice(cursor, h.index), hit: false })
      out.push({ s: h.text, hit: true })
      cursor = h.index + h.text.length
    }
    if (cursor < text.length) out.push({ s: text.slice(cursor), hit: false })
    return out
  }, [hits, text])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div style={{ display: 'flex', gap: 8 }}>
        <div style={{ flex: 1 }}>
          <input
            className="input"
            placeholder="正则表达式，例如 (\w+)@(\w+)\.com"
            value={pattern}
            onChange={(e) => setPattern(e.target.value)}
            style={{ fontFamily: 'var(--mono)' }}
          />
        </div>
        <div className="seg" style={{ flexShrink: 0 }}>
          {FLAGS.map((f) => (
            <button
              key={f}
              className={flags.includes(f) ? 'active' : ''}
              title={
                f === 'g'
                  ? '全局匹配'
                  : f === 'i'
                    ? '忽略大小写'
                    : f === 'm'
                      ? '多行：^ $ 匹配每行'
                      : f === 's'
                        ? '. 匹配换行'
                        : 'Unicode 模式'
              }
              onClick={() =>
                setFlags((cur) => (cur.includes(f) ? cur.replace(f, '') : cur + f))
              }
              style={{ fontFamily: 'var(--mono)', minWidth: 30 }}
            >
              {f}
            </button>
          ))}
        </div>
      </div>

      {error && <div style={{ fontSize: 12, color: 'var(--danger, #d64545)' }}>{error}</div>}

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
        <div className="field">
          <label>测试文本</label>
          <textarea
            className="input"
            rows={9}
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="把要匹配的内容放这儿"
          />
        </div>
        <div className="field">
          <label style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>高亮</span>
            <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>
              {hits.length} 处{hits.length >= 500 ? '（已截断）' : ''}
            </span>
          </label>
          <div
            style={{
              minHeight: 178,
              maxHeight: 240,
              overflow: 'auto',
              padding: '9px 11px',
              border: '1px solid var(--border)',
              borderRadius: 'var(--radius-sm)',
              background: 'var(--surface-2, #fafafa)',
              fontFamily: 'var(--mono)',
              fontSize: 12,
              lineHeight: 1.5,
              whiteSpace: 'pre-wrap',
              wordBreak: 'break-word'
            }}
          >
            {segments.length === 0 ? (
              <span style={{ color: 'var(--text-3)' }}>—</span>
            ) : (
              segments.map((seg, i) =>
                seg.hit ? (
                  <mark
                    key={i}
                    style={{
                      background: 'rgba(108, 92, 231, 0.22)',
                      color: 'inherit',
                      borderRadius: 2
                    }}
                  >
                    {seg.s}
                  </mark>
                ) : (
                  <span key={i}>{seg.s}</span>
                )
              )
            )}
          </div>
        </div>
      </div>

      {hits.length > 0 && (
        <PanelSection title="匹配明细">
          <div
            style={{
              maxHeight: 190,
              overflow: 'auto',
              border: '1px solid var(--border)',
              borderRadius: 'var(--radius-sm)'
            }}
          >
            {hits.map((h, i) => (
              <div
                key={i}
                style={{
                  display: 'flex',
                  gap: 10,
                  padding: '5px 10px',
                  fontSize: 11.5,
                  borderTop: i ? '1px solid var(--border)' : undefined,
                  fontFamily: 'var(--mono)'
                }}
              >
                <span style={{ color: 'var(--text-3)', width: 46, flexShrink: 0 }}>#{i + 1}</span>
                <span style={{ color: 'var(--text-3)', width: 54, flexShrink: 0 }}>@{h.index}</span>
                <span style={{ flex: 1, wordBreak: 'break-all' }}>{h.text || '（空匹配）'}</span>
                {(h.groups.length > 0 || h.named.length > 0) && (
                  <span style={{ color: 'var(--text-3)', maxWidth: 260, wordBreak: 'break-all' }}>
                    {[
                      ...h.groups.map((v, gi) => `$${gi + 1}=${v ?? 'undefined'}`),
                      ...h.named.map((g) => `<${g.name}>=${g.value ?? 'undefined'}`)
                    ].join('  ')}
                  </span>
                )}
              </div>
            ))}
          </div>
        </PanelSection>
      )}

      <div className="field">
        <label style={{ display: 'flex', justifyContent: 'space-between' }}>
          <span>替换（支持 $1 $&amp; 等）</span>
          {replaced && <CopyBtn text={replaced} />}
        </label>
        <input
          className="input"
          value={replacement}
          onChange={(e) => setReplacement(e.target.value)}
          placeholder="留空不替换"
          style={{ fontFamily: 'var(--mono)' }}
        />
        {replaced && (
          <textarea
            className="input"
            rows={4}
            readOnly
            value={replaced}
            style={{ background: 'var(--surface-2, #fafafa)' }}
          />
        )}
      </div>
    </div>
  )
}

/* ---------------- 工具表 ---------------- */

export const formatTools: Tool[] = [
  {
    kind: 'transform',
    id: 'json',
    name: 'JSON',
    desc: '格式化 / 压缩 / 键排序，报错给出行列',
    dirs: { encode: '格式化', decode: '压缩' },
    placeholder: '{"hello":"world","list":[1,2,3]}',
    rows: 12,
    options: [
      {
        key: 'indent',
        label: '缩进',
        kind: 'select',
        default: '2',
        options: [
          { label: '2 空格', value: '2' },
          { label: '4 空格', value: '4' },
          { label: 'Tab', value: 'tab' }
        ]
      },
      { key: 'sort', label: '按键名排序', kind: 'switch', default: false }
    ],
    run: (input, o, dir) => {
      if (!input.trim()) return ''
      let value: unknown
      try {
        value = JSON.parse(input)
      } catch (e) {
        throw new Error(jsonError(e, input))
      }
      if (optBool(o, 'sort')) value = sortDeep(value)
      return dir === 'encode' ? JSON.stringify(value, null, indentOf(o)) : JSON.stringify(value)
    }
  },
  {
    kind: 'transform',
    id: 'xml',
    name: 'XML',
    desc: '按标签层级缩进，注释与 CDATA 原样保留',
    dirs: { encode: '美化', decode: '压缩' },
    placeholder: '<root><item id="1">文本</item></root>',
    rows: 12,
    run: (input, _o, dir) =>
      dir === 'encode' ? formatXml(input) : input.replace(/>\s+</g, '><').trim()
  },
  {
    kind: 'transform',
    id: 'sql',
    name: 'SQL',
    desc: '关键字大写 + 断行（轻量美化，不解析语法）',
    dirs: { encode: '美化', decode: '压成一行' },
    placeholder: 'select id,name from users where id=1 and status=1 order by id desc',
    rows: 12,
    run: (input, _o, dir) =>
      dir === 'encode' ? formatSql(input) : input.replace(/\s+/g, ' ').trim()
  },
  {
    kind: 'panel',
    id: 'regex',
    name: '正则测试',
    desc: '实时高亮、分组明细、替换预览',
    Panel: RegexPanel
  }
]
