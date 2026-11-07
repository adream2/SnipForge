/** 开发者工具箱 · 文本处理 */

import { useMemo, useState } from 'react'
import type { Tool } from '../types'
import { optBool } from '../types'
import { formatBytes, utf8Encode } from '../helpers'
import { CopyBtn, PanelSection, ResultRow } from '../ui'

/* ---------------- 命名风格转换 ---------------- */

function splitWords(src: string): string[] {
  return src
    .replace(/([a-z0-9])([A-Z])/g, '$1 $2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1 $2')
    .replace(/[_\-.\s]+/g, ' ')
    .trim()
    .split(/\s+/)
    .filter(Boolean)
}

const cap = (w: string): string => w.charAt(0).toUpperCase() + w.slice(1).toLowerCase()

function CasePanel() {
  const [text, setText] = useState('')
  const words = useMemo(() => splitWords(text), [text])

  const rows = useMemo(() => {
    const lower = words.map((w) => w.toLowerCase())
    return [
      ['全大写', text.toUpperCase()],
      ['全小写', text.toLowerCase()],
      ['每词首字母大写', words.map(cap).join(' ')],
      ['camelCase', lower.map((w, i) => (i === 0 ? w : cap(w))).join('')],
      ['PascalCase', lower.map(cap).join('')],
      ['snake_case', lower.join('_')],
      ['kebab-case', lower.join('-')],
      ['CONSTANT_CASE', lower.join('_').toUpperCase()],
      ['dot.case', lower.join('.')]
    ] as Array<[string, string]>
  }, [text, words])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div className="field">
        <label>源文本</label>
        <textarea
          className="input"
          rows={4}
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="user profile name / userProfileName / user_profile_name"
        />
        <span className="hint">识别 camelCase、snake_case、kebab-case、空格分隔</span>
      </div>

      <PanelSection title="结果">
        <div>
          {rows.map(([label, value]) => (
            <div key={label} style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '3px 0' }}>
              <span style={{ width: 130, flexShrink: 0, fontSize: 11.5, color: 'var(--text-3)' }}>
                {label}
              </span>
              <code
                style={{
                  flex: 1,
                  minWidth: 0,
                  fontFamily: 'var(--mono)',
                  fontSize: 12,
                  overflow: 'hidden',
                  textOverflow: 'ellipsis',
                  whiteSpace: 'nowrap'
                }}
                title={value}
              >
                {value || '—'}
              </code>
              <CopyBtn text={value} label="" />
            </div>
          ))}
        </div>
      </PanelSection>
    </div>
  )
}

/* ---------------- 文本 diff ---------------- */

type DiffLine = { t: 'same' | 'add' | 'del'; s: string }

function diffLines(a: string[], b: string[]): DiffLine[] {
  const n = a.length
  const m = b.length
  if (n * m > 4_000_000) throw new Error('两边行数乘积太大（>400 万），先各砍一半再比')

  // dp[i][j] = a[i..] 与 b[j..] 的最长公共子序列长度，从右下往左上填
  const w = m + 1
  const dp = new Uint32Array((n + 1) * w)
  for (let i = n - 1; i >= 0; i--) {
    for (let j = m - 1; j >= 0; j--) {
      dp[i * w + j] =
        a[i] === b[j]
          ? dp[(i + 1) * w + j + 1] + 1
          : Math.max(dp[(i + 1) * w + j], dp[i * w + j + 1])
    }
  }

  const out: DiffLine[] = []
  let i = 0
  let j = 0
  while (i < n && j < m) {
    if (a[i] === b[j]) {
      out.push({ t: 'same', s: a[i] })
      i++
      j++
    } else if (dp[(i + 1) * w + j] >= dp[i * w + j + 1]) {
      out.push({ t: 'del', s: a[i++] })
    } else {
      out.push({ t: 'add', s: b[j++] })
    }
  }
  while (i < n) out.push({ t: 'del', s: a[i++] })
  while (j < m) out.push({ t: 'add', s: b[j++] })
  return out
}

function DiffPanel() {
  const [left, setLeft] = useState('')
  const [right, setRight] = useState('')
  const [onlyChanged, setOnlyChanged] = useState(false)

  const { lines, stats, error } = useMemo(() => {
    if (!left && !right) return { lines: [] as DiffLine[], stats: null, error: null as string | null }
    try {
      const out = diffLines(left.split('\n'), right.split('\n'))
      return {
        lines: out,
        stats: {
          add: out.filter((l) => l.t === 'add').length,
          del: out.filter((l) => l.t === 'del').length,
          same: out.filter((l) => l.t === 'same').length
        },
        error: null as string | null
      }
    } catch (e) {
      return { lines: [], stats: null, error: e instanceof Error ? e.message : String(e) }
    }
  }, [left, right])

  const shown = onlyChanged ? lines.filter((l) => l.t !== 'same') : lines

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10 }}>
        <div className="field">
          <label>原文本</label>
          <textarea
            className="input"
            rows={8}
            value={left}
            onChange={(e) => setLeft(e.target.value)}
            placeholder="粘贴旧版本"
          />
        </div>
        <div className="field">
          <label>新文本</label>
          <textarea
            className="input"
            rows={8}
            value={right}
            onChange={(e) => setRight(e.target.value)}
            placeholder="粘贴新版本"
          />
        </div>
      </div>

      <div style={{ display: 'flex', alignItems: 'center', gap: 12, fontSize: 12 }}>
        {stats && (
          <>
            <span className="tag success" style={{ height: 19, fontSize: 10.5 }}>
              +{stats.add}
            </span>
            <span className="tag danger" style={{ height: 19, fontSize: 10.5 }}>
              −{stats.del}
            </span>
            <span style={{ color: 'var(--text-3)' }}>相同 {stats.same} 行</span>
          </>
        )}
        <div style={{ flex: 1 }} />
        <label style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
          <span
            className={`switch ${onlyChanged ? 'on' : ''}`}
            onClick={() => setOnlyChanged((v) => !v)}
          />
          只看差异
        </label>
        {lines.length > 0 && <CopyBtn text={shown.map((l) => (l.t === 'add' ? '+ ' : l.t === 'del' ? '- ' : '  ') + l.s).join('\n')} label="复制结果" />}
      </div>

      {error && <div style={{ fontSize: 12, color: 'var(--danger, #d64545)' }}>{error}</div>}

      {shown.length > 0 && (
        <div
          style={{
            border: '1px solid var(--border)',
            borderRadius: 'var(--radius-sm)',
            maxHeight: 320,
            overflow: 'auto',
            fontFamily: 'var(--mono)',
            fontSize: 11.5,
            lineHeight: 1.65
          }}
        >
          {shown.map((l, i) => (
            <div
              key={i}
              style={{
                display: 'flex',
                gap: 8,
                padding: '0 10px',
                whiteSpace: 'pre-wrap',
                wordBreak: 'break-all',
                background:
                  l.t === 'add'
                    ? 'rgba(46, 160, 67, 0.12)'
                    : l.t === 'del'
                      ? 'rgba(214, 69, 69, 0.10)'
                      : undefined
              }}
            >
              <span style={{ width: 10, flexShrink: 0, color: 'var(--text-3)' }}>
                {l.t === 'add' ? '+' : l.t === 'del' ? '-' : ' '}
              </span>
              <span style={{ flex: 1 }}>{l.s}</span>
            </div>
          ))}
        </div>
      )}
    </div>
  )
}

/* ---------------- 字数统计 ---------------- */

function CountPanel() {
  const [text, setText] = useState('')

  const s = useMemo(() => {
    const cjk = (text.match(/[\u4e00-\u9fff\u3400-\u4dbf]/g) ?? []).length
    const words = (text.match(/[A-Za-z0-9_'\u00c0-\u024f]+/g) ?? []).length
    const bytes = utf8Encode(text).length
    return [
      ['字符数', String(text.length)],
      ['不含空白', String(text.replace(/\s/g, '').length)],
      ['中文字', String(cjk)],
      ['英文单词', String(words)],
      ['行数', text ? String(text.split('\n').length) : '0'],
      ['段落', text.trim() ? String(text.trim().split(/\n\s*\n/).length) : '0'],
      ['UTF-8 字节', `${bytes}（${formatBytes(bytes)}）`]
    ] as Array<[string, string]>
  }, [text])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <textarea
        className="input"
        rows={10}
        value={text}
        onChange={(e) => setText(e.target.value)}
        placeholder="把文本放进来"
      />
      <div>
        {s.map(([label, value]) => (
          <ResultRow key={label} label={label} value={value} />
        ))}
      </div>
    </div>
  )
}

/* ---------------- 工具表 ---------------- */

export const textTools: Tool[] = [
  {
    kind: 'panel',
    id: 'case',
    name: '命名风格',
    desc: 'camel / snake / kebab / 常量一次全给',
    Panel: CasePanel
  },
  {
    kind: 'transform',
    id: 'dedupe',
    name: '去重排序',
    desc: '按行去重、可排序、可去空行',
    placeholder: '每行一条，去重后输出',
    rows: 12,
    options: [
      { key: 'sort', label: '按字典序排序', kind: 'switch', default: true },
      { key: 'ignoreCase', label: '忽略大小写', kind: 'switch', default: false },
      { key: 'dropEmpty', label: '去掉空行', kind: 'switch', default: true },
      { key: 'trim', label: '去掉每行首尾空白', kind: 'switch', default: true }
    ],
    run: (input, o) => {
      const trim = optBool(o, 'trim')
      let rows = input.split('\n').map((l) => (trim ? l.trim() : l))
      if (optBool(o, 'dropEmpty')) rows = rows.filter((l) => l.length > 0)
      const seen = new Set<string>()
      const key = (l: string) => (optBool(o, 'ignoreCase') ? l.toLowerCase() : l)
      const out: string[] = []
      for (const r of rows) {
        const k = key(r)
        if (seen.has(k)) continue
        seen.add(k)
        out.push(r)
      }
      return (optBool(o, 'sort') ? [...out].sort((a, b) => key(a).localeCompare(key(b), 'zh-CN')) : out).join('\n')
    }
  },
  {
    kind: 'transform',
    id: 'lines',
    name: '行处理',
    desc: '加行号、去空行、trim、反转',
    placeholder: '多行文本',
    rows: 12,
    options: [
      { key: 'number', label: '加行号', kind: 'switch', default: true },
      { key: 'dropEmpty', label: '去掉空行', kind: 'switch', default: false },
      { key: 'trim', label: '去首尾空白', kind: 'switch', default: false },
      { key: 'reverse', label: '反转行序', kind: 'switch', default: false }
    ],
    run: (input, o) => {
      let rows = input.split('\n')
      if (optBool(o, 'trim')) rows = rows.map((l) => l.trim())
      if (optBool(o, 'dropEmpty')) rows = rows.filter((l) => l.trim().length > 0)
      if (optBool(o, 'reverse')) rows = [...rows].reverse()
      const width = String(rows.length).length
      return rows
        .map((l, i) => (optBool(o, 'number') ? `${String(i + 1).padStart(width, ' ')}  ${l}` : l))
        .join('\n')
    }
  },
  {
    kind: 'panel',
    id: 'diff',
    name: '文本 diff',
    desc: '行级对比，看增删行',
    Panel: DiffPanel
  },
  {
    kind: 'panel',
    id: 'count',
    name: '字数统计',
    desc: '字符 / 中文字 / 单词 / 行 / 字节',
    Panel: CountPanel
  }
]
