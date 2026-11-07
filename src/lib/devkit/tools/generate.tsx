/** 开发者工具箱 · 生成器与计算 */

import { useEffect, useMemo, useState } from 'react'
import type { Tool } from '../types'
import { bytesToHex, formatBytes, md5, sha, utf8Encode } from '../helpers'
import { CopyBtn, PanelSection, ResultRow } from '../ui'

/* ---------------- UUID ---------------- */

function uuidv4(): string {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID()
  const b = crypto.getRandomValues(new Uint8Array(16))
  b[6] = (b[6] & 0x0f) | 0x40
  b[8] = (b[8] & 0x3f) | 0x80
  const h = bytesToHex(b)
  return `${h.slice(0, 8)}-${h.slice(8, 12)}-${h.slice(12, 16)}-${h.slice(16, 20)}-${h.slice(20)}`
}

function UuidPanel() {
  const [count, setCount] = useState(5)
  const [upper, setUpper] = useState(false)
  const [noDash, setNoDash] = useState(false)
  const [tick, setTick] = useState(0)

  const list = useMemo(() => {
    void tick
    return Array.from({ length: count }, () => {
      const u = uuidv4()
      const s = upper ? u.toUpperCase() : u
      return noDash ? s.replace(/-/g, '') : s
    })
  }, [count, upper, noDash, tick])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div style={{ display: 'flex', gap: 10, alignItems: 'flex-end', flexWrap: 'wrap' }}>
        <div className="field" style={{ width: 110 }}>
          <label>数量</label>
          <input
            className="input"
            type="number"
            min={1}
            max={200}
            value={count}
            onChange={(e) => setCount(Math.min(200, Math.max(1, Number(e.target.value) || 1)))}
          />
        </div>
        <label style={{ display: 'flex', alignItems: 'center', gap: 7, fontSize: 12 }}>
          <span className={`switch ${upper ? 'on' : ''}`} onClick={() => setUpper((v) => !v)} />
          大写
        </label>
        <label style={{ display: 'flex', alignItems: 'center', gap: 7, fontSize: 12 }}>
          <span className={`switch ${noDash ? 'on' : ''}`} onClick={() => setNoDash((v) => !v)} />
          去连字符
        </label>
        <div style={{ flex: 1 }} />
        <button className="btn sm" onClick={() => setTick((t) => t + 1)}>
          重新生成
        </button>
        <CopyBtn text={list.join('\n')} label="复制全部" />
      </div>

      <div
        style={{
          border: '1px solid var(--border)',
          borderRadius: 'var(--radius-sm)',
          maxHeight: 320,
          overflow: 'auto'
        }}
      >
        {list.map((u, i) => (
          <div
            key={i}
            style={{
              display: 'flex',
              alignItems: 'center',
              gap: 10,
              padding: '5px 10px',
              borderTop: i ? '1px solid var(--border)' : undefined
            }}
          >
            <code style={{ flex: 1, fontFamily: 'var(--mono)', fontSize: 12 }}>{u}</code>
            <CopyBtn text={u} label="" />
          </div>
        ))}
      </div>
    </div>
  )
}

/* ---------------- 时间戳 ---------------- */

const rtf = new Intl.RelativeTimeFormat('zh-CN', { numeric: 'auto' })

function relative(target: number): string {
  let v = (target - Date.now()) / 1000
  const steps: Array<[number, Intl.RelativeTimeFormatUnit]> = [
    [60, 'second'],
    [60, 'minute'],
    [24, 'hour'],
    [30, 'day'],
    [12, 'month'],
    [Infinity, 'year']
  ]
  for (const [size, unit] of steps) {
    if (Math.abs(v) < size) return rtf.format(Math.round(v), unit)
    v /= size
  }
  return ''
}

function parseTime(raw: string): Date | null {
  const s = raw.trim()
  if (!s) return null
  if (/^-?\d+$/.test(s)) {
    const n = Number(s)
    if (!Number.isFinite(n)) return null
    // 10 位按秒，13 位按毫秒，其它位数量级猜一下（1900~2200 之间取最近的）
    if (s.replace('-', '').length <= 10) return new Date(n * 1000)
    return new Date(n)
  }
  const d = new Date(s)
  return Number.isNaN(d.getTime()) ? null : d
}

function TimestampPanel() {
  const [now, setNow] = useState(Date.now())
  const [input, setInput] = useState('')

  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 200)
    return () => window.clearInterval(t)
  }, [])

  const picked = useMemo(() => (input.trim() ? parseTime(input) : null), [input])
  const d = picked ?? new Date(now)
  const bad = Boolean(input.trim()) && !picked

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 16 }}>
      <div
        style={{
          padding: '12px 14px',
          border: '1px solid var(--border)',
          borderRadius: 'var(--radius)',
          background: 'var(--surface)'
        }}
      >
        <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginBottom: 6 }}>
          {picked ? '按输入解析' : '当前时间（实时）'}
        </div>
        <div style={{ display: 'flex', alignItems: 'baseline', gap: 10, flexWrap: 'wrap' }}>
          <span style={{ fontFamily: 'var(--mono)', fontSize: 22, letterSpacing: '-0.02em' }}>
            {picked ? d.getTime() : now}
          </span>
          <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>毫秒</span>
          <div style={{ flex: 1 }} />
          <CopyBtn text={String(d.getTime())} label="复制毫秒" />
          <CopyBtn text={String(Math.floor(d.getTime() / 1000))} label="复制秒" />
        </div>
      </div>

      <div className="field">
        <label>解析任意时间</label>
        <input
          className="input"
          placeholder="1667000000 / 1667000000000 / 2023-01-01 12:00:00 / 2023-01-01T04:00:00Z"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          style={{ fontFamily: 'var(--mono)' }}
        />
        <span className="hint">
          {bad ? '认不出来，试试 10 位秒、13 位毫秒，或标准日期格式' : '10 位当秒，13 位当毫秒'}
        </span>
      </div>

      {!bad && (
        <div>
          <ResultRow label="秒" value={String(Math.floor(d.getTime() / 1000))} />
          <ResultRow label="毫秒" value={String(d.getTime())} />
          <ResultRow label="本地时间" value={d.toLocaleString()} />
          <ResultRow label="ISO 8601" value={d.toISOString()} />
          <ResultRow label="UTC" value={d.toUTCString()} />
          <ResultRow label="相对现在" value={relative(d.getTime())} />
          <ResultRow label="星期" value={d.toLocaleDateString('zh-CN', { weekday: 'long' })} />
        </div>
      )}
    </div>
  )
}

/* ---------------- 哈希 ---------------- */

function HashPanel() {
  const [text, setText] = useState('')
  const [hashes, setHashes] = useState<Array<{ algo: string; value: string; error?: string }>>([])

  useEffect(() => {
    let alive = true
    const md5v = md5(text)
    setHashes([{ algo: 'MD5', value: md5v }])
    void Promise.all(
      (['SHA-1', 'SHA-256', 'SHA-384', 'SHA-512'] as const).map(async (algo) => {
        try {
          return { algo, value: await sha(algo, text) }
        } catch (e) {
          return { algo, value: '', error: e instanceof Error ? e.message : String(e) }
        }
      })
    ).then((list) => {
      if (alive) setHashes([{ algo: 'MD5', value: md5v }, ...list])
    })
    return () => {
      alive = false
    }
  }, [text])

  const bytes = utf8Encode(text).length

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div className="field">
        <label style={{ display: 'flex', justifyContent: 'space-between' }}>
          <span>文本</span>
          <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>
            {text.length} 字符 · {formatBytes(bytes)} UTF-8
          </span>
        </label>
        <textarea
          className="input"
          rows={6}
          value={text}
          onChange={(e) => setText(e.target.value)}
          placeholder="粘贴要算哈希的内容，实时出结果"
        />
      </div>

      <div style={{ display: 'flex', flexDirection: 'column', gap: 10 }}>
        {hashes.map((h) => (
          <div key={h.algo} className="field">
            <label style={{ display: 'flex', justifyContent: 'space-between' }}>
              <span style={{ fontFamily: 'var(--mono)' }}>{h.algo}</span>
              <CopyBtn text={h.value} />
            </label>
            <code
              style={{
                fontFamily: 'var(--mono)',
                fontSize: 11.5,
                padding: '7px 10px',
                border: '1px solid var(--border)',
                borderRadius: 'var(--radius-sm)',
                background: 'var(--surface-2, #fafafa)',
                wordBreak: 'break-all',
                color: h.error ? 'var(--text-3)' : undefined
              }}
            >
              {h.error ?? h.value ?? '—'}
            </code>
          </div>
        ))}
      </div>
      <span className="hint">MD5 只用于老接口签名校验，别拿它存密码。</span>
    </div>
  )
}

/* ---------------- 密码生成 ---------------- */

const CHARSETS = {
  lower: 'abcdefghijklmnopqrstuvwxyz',
  upper: 'ABCDEFGHIJKLMNOPQRSTUVWXYZ',
  digit: '0123456789',
  symbol: '!@#$%^&*()-_=+[]{};:,.?/'
}
const CONFUSING = '0OoIl1|`'

/** 拒绝采样：直接 % 长度会让前几个字符出现得更频繁，密码强度打折扣 */
function pick(pool: string): string {
  const limit = 256 - (256 % pool.length)
  const buf = new Uint8Array(1)
  for (;;) {
    crypto.getRandomValues(buf)
    if (buf[0] < limit) return pool[buf[0] % pool.length]
  }
}

function PasswordPanel() {
  const [len, setLen] = useState(20)
  const [sets, setSets] = useState({ lower: true, upper: true, digit: true, symbol: true })
  const [noConfusing, setNoConfusing] = useState(true)
  const [count, setCount] = useState(3)
  const [tick, setTick] = useState(0)

  const pools = useMemo(() => {
    return (Object.keys(CHARSETS) as Array<keyof typeof CHARSETS>)
      .filter((k) => sets[k])
      .map((k) => (noConfusing ? [...CHARSETS[k]].filter((c) => !CONFUSING.includes(c)).join('') : CHARSETS[k]))
      .filter((p) => p.length > 0)
  }, [sets, noConfusing])

  const entropy = useMemo(() => {
    const total = pools.reduce((n, p) => n + p.length, 0)
    return total > 0 ? Math.round(len * Math.log2(total)) : 0
  }, [pools, len])

  const list = useMemo(() => {
    void tick
    const all = pools.join('')
    if (!all) return []
    return Array.from({ length: count }, () => {
      // 先每类取一个，保证"包含数字"这类要求一定满足，剩下随机，最后洗牌
      const chars: string[] = pools.map((p) => pick(p))
      while (chars.length < len) chars.push(pick(all))
      for (let i = chars.length - 1; i > 0; i--) {
        const j = crypto.getRandomValues(new Uint8Array(1))[0] % (i + 1)
        ;[chars[i], chars[j]] = [chars[j], chars[i]]
      }
      return chars.slice(0, len).join('')
    })
  }, [pools, len, count, tick])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div style={{ display: 'flex', gap: 16, alignItems: 'flex-end', flexWrap: 'wrap' }}>
        <div className="field" style={{ width: 200 }}>
          <label style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>长度</span>
            <span style={{ color: 'var(--text-3)' }}>{len}</span>
          </label>
          <input
            className="slider"
            type="range"
            min={6}
            max={64}
            value={len}
            onChange={(e) => setLen(Number(e.target.value))}
          />
        </div>
        <div className="field" style={{ width: 90 }}>
          <label>条数</label>
          <input
            className="input"
            type="number"
            min={1}
            max={20}
            value={count}
            onChange={(e) => setCount(Math.min(20, Math.max(1, Number(e.target.value) || 1)))}
          />
        </div>
        <span className="tag accent" style={{ height: 20, fontSize: 10.5 }}>
          约 {entropy} bit 熵
        </span>
      </div>

      <div style={{ display: 'flex', gap: 14, flexWrap: 'wrap', fontSize: 12 }}>
        {(
          [
            ['lower', '小写 a-z'],
            ['upper', '大写 A-Z'],
            ['digit', '数字 0-9'],
            ['symbol', '符号 !@#$']
          ] as Array<[keyof typeof CHARSETS, string]>
        ).map(([k, label]) => (
          <label key={k} style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
            <span
              className={`switch ${sets[k] ? 'on' : ''}`}
              onClick={() => setSets((s) => ({ ...s, [k]: !s[k] }))}
            />
            {label}
          </label>
        ))}
        <label style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
          <span
            className={`switch ${noConfusing ? 'on' : ''}`}
            onClick={() => setNoConfusing((v) => !v)}
          />
          排除易混 0OoIl1
        </label>
      </div>

      <div style={{ display: 'flex', gap: 8 }}>
        <button className="btn sm" onClick={() => setTick((t) => t + 1)}>
          重新生成
        </button>
        <CopyBtn text={list.join('\n')} label="复制全部" />
      </div>

      <div
        style={{
          border: '1px solid var(--border)',
          borderRadius: 'var(--radius-sm)'
        }}
      >
        {list.length === 0 ? (
          <div style={{ padding: 14, fontSize: 12, color: 'var(--text-3)' }}>
            至少选一类字符
          </div>
        ) : (
          list.map((p, i) => (
            <div
              key={i}
              style={{
                display: 'flex',
                alignItems: 'center',
                gap: 10,
                padding: '6px 10px',
                borderTop: i ? '1px solid var(--border)' : undefined
              }}
            >
              <code style={{ flex: 1, fontFamily: 'var(--mono)', fontSize: 13, wordBreak: 'break-all' }}>
                {p}
              </code>
              <CopyBtn text={p} label="" />
            </div>
          ))
        )}
      </div>
    </div>
  )
}

/* ---------------- 进制转换 ---------------- */

const DIGITS = '0123456789abcdefghijklmnopqrstuvwxyz'

function parseBig(raw: string, radix: number): bigint {
  let s = raw.trim().toLowerCase().replace(/[\s_,]/g, '')
  if (!s) throw new Error('还没有输入数字')
  let neg = false
  if (s.startsWith('-')) {
    neg = true
    s = s.slice(1)
  }
  if (radix === 0) {
    if (s.startsWith('0x')) [radix, s] = [16, s.slice(2)]
    else if (s.startsWith('0b')) [radix, s] = [2, s.slice(2)]
    else if (s.startsWith('0o')) [radix, s] = [8, s.slice(2)]
    else radix = 10
  }
  let n = 0n
  for (const ch of s) {
    const d = DIGITS.indexOf(ch)
    if (d < 0 || d >= radix) throw new Error(`"${ch}" 不是 ${radix} 进制里的数字`)
    n = n * BigInt(radix) + BigInt(d)
  }
  return neg ? -n : n
}

function RadixPanel() {
  const [value, setValue] = useState('255')
  const [from, setFrom] = useState('0')

  const { result, error } = useMemo(() => {
    try {
      const n = parseBig(value, Number(from))
      const out: Array<[string, string]> = [
        ['2 进制', n.toString(2)],
        ['8 进制', n.toString(8)],
        ['10 进制', n.toString(10)],
        ['16 进制', n.toString(16)],
        ['36 进制', n.toString(36)]
      ]
      return { result: out, error: null as string | null }
    } catch (e) {
      return { result: [], error: e instanceof Error ? e.message : String(e) }
    }
  }, [value, from])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div style={{ display: 'flex', gap: 10, alignItems: 'flex-end' }}>
        <div className="field" style={{ flex: 1 }}>
          <label>数字</label>
          <input
            className="input"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            style={{ fontFamily: 'var(--mono)' }}
            placeholder="255 / 0xff / 0b11111111"
          />
        </div>
        <div className="field" style={{ width: 150 }}>
          <label>源进制</label>
          <select className="input" value={from} onChange={(e) => setFrom(e.target.value)}>
            <option value="0">自动识别</option>
            <option value="2">2</option>
            <option value="8">8</option>
            <option value="10">10</option>
            <option value="16">16</option>
            <option value="36">36</option>
          </select>
        </div>
      </div>

      {error ? (
        <div style={{ fontSize: 12, color: 'var(--danger, #d64545)' }}>{error}</div>
      ) : (
        <div>
          {result.map(([label, v]) => (
            <ResultRow key={label} label={label} value={v} />
          ))}
        </div>
      )}
      <span className="hint">用 BigInt 计算，超过 2^53 也不会丢精度。</span>
    </div>
  )
}

/* ---------------- Cron ---------------- */

const FIELDS: Array<[string, number, number]> = [
  ['分', 0, 59],
  ['时', 0, 23],
  ['日', 1, 31],
  ['月', 1, 12],
  ['周', 0, 6]
]

function parseCronField(raw: string, min: number, max: number): Set<number> {
  const out = new Set<number>()
  for (const part of raw.split(',')) {
    const [base, stepRaw] = part.split('/')
    const step = stepRaw ? Number(stepRaw) : 1
    if (!Number.isInteger(step) || step < 1) throw new Error(`步长 "${stepRaw}" 不合法`)
    let lo = min
    let hi = max
    if (base !== '*') {
      const [a, b] = base.split('-')
      lo = Number(a)
      hi = b !== undefined ? Number(b) : stepRaw ? max : lo
      if (Number.isNaN(lo) || Number.isNaN(hi)) throw new Error(`"${base}" 不是数字`)
      if (lo < min || hi > max) throw new Error(`"${base}" 超出范围 ${min}-${max}`)
      if (lo > hi) throw new Error(`"${base}" 起点比终点大`)
    }
    for (let v = lo; v <= hi; v += step) out.add(max === 6 && v === 7 ? 0 : v)
  }
  if (out.size === 0) throw new Error('这个字段没有匹配到任何值')
  return out
}

function cronNext(expr: string, count = 5): Date[] {
  const parts = expr.trim().split(/\s+/)
  if (parts.length !== 5) throw new Error('需要 5 段：分 时 日 月 周')
  if (parts[0].startsWith('@')) throw new Error('不支持 @daily 这类宏，请写 5 段式')
  const minutes = [...parseCronField(parts[0], 0, 59)].sort((a, b) => a - b)
  const hours = [...parseCronField(parts[1], 0, 23)].sort((a, b) => a - b)
  const doms = parseCronField(parts[2], 1, 31)
  const months = parseCronField(parts[3], 1, 12)
  const dows = parseCronField(parts[4], 0, 6)
  const domStar = parts[2] === '*'
  const dowStar = parts[4] === '*'

  const now = Date.now()
  const start = new Date()
  const out: Date[] = []
  // 5 年窗口：像 "0 0 1 1 *"（每年一次）这种稀疏表达式也能列满 5 条
  for (let i = 0; i < 1900 && out.length < count; i++) {
    const d = new Date(start.getFullYear(), start.getMonth(), start.getDate() + i)
    if (!months.has(d.getMonth() + 1)) continue
    // 标准 cron：日和周都被限定时取"或"，只限定一个时取"与"
    const domOk = doms.has(d.getDate())
    const dowOk = dows.has(d.getDay())
    const dayOk = domStar && dowStar ? true : domStar ? dowOk : dowStar ? domOk : domOk || dowOk
    if (!dayOk) continue
    for (const h of hours) {
      for (const mi of minutes) {
        const t = new Date(d.getFullYear(), d.getMonth(), d.getDate(), h, mi).getTime()
        if (t <= now) continue
        out.push(new Date(t))
        if (out.length >= count) break
      }
      if (out.length >= count) break
    }
  }
  if (out.length === 0) throw new Error('未来 5 年里都不会触发，检查一下「月」和「日」的组合')
  return out
}

/** 只说人话那一层：分/时能确定就报时间，日期部分按 日/周 哪个被限定来描述 */
function cronHuman(expr: string): string {
  const p = expr.trim().split(/\s+/)
  if (p.length !== 5) return ''
  const [mi, ho, dom, mon, dow] = p
  const two = (s: string) => s.padStart(2, '0')
  const time = /^\d+$/.test(mi) && /^\d+$/.test(ho) ? `${two(ho)}:${two(mi)}` : ''
  const week = ['周日', '周一', '周二', '周三', '周四', '周五', '周六']
  let when = ''
  if (dom === '*' && dow === '*') when = '每天'
  else if (dow !== '*' && /^\d+$/.test(dow)) when = `每${week[Number(dow) % 7]}`
  else if (dom !== '*') when = `每月 ${dom} 日`
  else when = '按表达式'
  if (mon !== '*') when = `${mon} 月的${when}`
  if (time) return `${when} ${time}`
  if (mi !== '*' && ho === '*') return `${when}每小时第 ${mi} 分`
  if (mi === '*' && ho === '*') return `${when}每分钟`
  return `${when}按表达式`
}

function CronPanel() {
  const [expr, setExpr] = useState('*/5 9-18 * * 1-5')

  const { runs, error } = useMemo(() => {
    try {
      return { runs: cronNext(expr), error: null as string | null }
    } catch (e) {
      return { runs: [], error: e instanceof Error ? e.message : String(e) }
    }
  }, [expr])

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 14 }}>
      <div className="field">
        <label>表达式（分 时 日 月 周）</label>
        <input
          className="input"
          value={expr}
          onChange={(e) => setExpr(e.target.value)}
          style={{ fontFamily: 'var(--mono)', fontSize: 13 }}
        />
        <span className="hint">
          {error ?? cronHuman(expr)}
          {'\n'}
          例：*/5 * * * * 每 5 分钟；0 9 * * 1-5 工作日 9 点；0 0 1 * * 每月 1 号
        </span>
      </div>

      {!error && (
        <PanelSection title="接下来 5 次">
          <div style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
            {runs.map((r, i) => (
              <div
                key={i}
                style={{
                  display: 'flex',
                  gap: 12,
                  fontSize: 12,
                  fontFamily: 'var(--mono)',
                  padding: '3px 0'
                }}
              >
                <span style={{ color: 'var(--text-3)' }}>
                  {r.toLocaleString()} {['日', '一', '二', '三', '四', '五', '六'][r.getDay()]}
                </span>
                <span style={{ color: 'var(--text-3)' }}>{relative(r.getTime())}</span>
              </div>
            ))}
          </div>
        </PanelSection>
      )}
    </div>
  )
}

/* ---------------- 工具表 ---------------- */

export const generateTools: Tool[] = [
  { kind: 'panel', id: 'uuid', name: 'UUID', desc: '批量生成 v4 UUID', Panel: UuidPanel },
  {
    kind: 'panel',
    id: 'timestamp',
    name: '时间戳',
    desc: '实时时间戳，秒 / 毫秒 / 日期互转',
    Panel: TimestampPanel
  },
  {
    kind: 'panel',
    id: 'hash',
    name: '哈希',
    desc: 'MD5 / SHA-1 / SHA-256 / SHA-384 / SHA-512',
    Panel: HashPanel
  },
  {
    kind: 'panel',
    id: 'password',
    name: '密码生成',
    desc: '拒绝采样 + 保证每类字符至少一个',
    Panel: PasswordPanel
  },
  { kind: 'panel', id: 'radix', name: '进制转换', desc: '2 / 8 / 10 / 16 / 36 互转，BigInt 不丢精度', Panel: RadixPanel },
  { kind: 'panel', id: 'cron', name: 'Cron 表达式', desc: '看接下来 5 次触发时间', Panel: CronPanel }
]
