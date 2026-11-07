/**
 * 开发者工具箱页面。
 *
 * 工具目录来自插件清单（`plugins/devkit.plugin.json` 的 groups），
 * 具体算法来自前端注册表（`lib/devkit`）。页面本身不含任何业务逻辑，
 * 只负责：分组列表、通用转换 UI、把面板类工具挂上去。
 *
 * 关掉插件页里的「开发者工具箱」后，这个入口会提示已停用 —— 不是白屏，
 * 否则用户会以为功能丢了。
 */

import { useEffect, useMemo, useState } from 'react'
import { api } from '../lib/api'
import type { PluginInfo } from '../lib/types'
import { defaultsOf, optBool, optNum, optStr } from '../lib/devkit/types'
import type { OptionValues, PanelTool, TransformTool } from '../lib/devkit/types'
import { isMissing, resolve } from '../lib/devkit/registry'
import { CopyBtn, OutBox } from '../lib/devkit/ui'
import { IconPlugin } from '../components/Icons'

/** 面板类工具是组件类型，直接 <active.Panel /> 在 TSX 里不认，转一手 */
function PanelHost({ tool }: { tool: PanelTool }) {
  const Panel = tool.Panel
  return <Panel />
}

/* ---------------- 通用转换视图 ---------------- */

function TransformView({ tool }: { tool: TransformTool }) {
  const [opts, setOpts] = useState<OptionValues>(() => defaultsOf(tool))
  const [dir, setDir] = useState<'encode' | 'decode'>('encode')
  const [input, setInput] = useState('')

  const { out, error } = useMemo(() => {
    if (!input) return { out: '', error: null as string | null }
    try {
      return { out: tool.run(input, opts, dir), error: null as string | null }
    } catch (e) {
      return { out: '', error: e instanceof Error ? e.message : String(e) }
    }
  }, [input, opts, dir, tool])

  const setOpt = (key: string, value: string | boolean | number) =>
    setOpts((cur) => ({ ...cur, [key]: value }))

  /** 切方向时把结果搬回输入：编码后想看能不能解回来，不用手动复制两遍 */
  const switchDir = (next: 'encode' | 'decode') => {
    if (next === dir) return
    setInput(out)
    setDir(next)
  }

  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
        {tool.dirs && (
          <div className="seg">
            <button className={dir === 'encode' ? 'active' : ''} onClick={() => switchDir('encode')}>
              {tool.dirs.encode}
            </button>
            <button className={dir === 'decode' ? 'active' : ''} onClick={() => switchDir('decode')}>
              {tool.dirs.decode}
            </button>
          </div>
        )}
        {tool.options?.map((o) => {
          if (o.kind === 'switch') {
            return (
              <label
                key={o.key}
                style={{ display: 'flex', alignItems: 'center', gap: 7, fontSize: 12 }}
                title={o.hint}
              >
                <span
                  className={`switch ${optBool(opts, o.key) ? 'on' : ''}`}
                  onClick={() => setOpt(o.key, !optBool(opts, o.key))}
                />
                {o.label}
              </label>
            )
          }
          if (o.kind === 'select') {
            return (
              <select
                key={o.key}
                className="input"
                value={optStr(opts, o.key)}
                onChange={(e) => setOpt(o.key, e.target.value)}
                style={{ width: 'auto', height: 30, fontSize: 12 }}
                title={o.label}
              >
                {o.options?.map((v) => (
                  <option key={v.value} value={v.value}>
                    {v.label}
                  </option>
                ))}
              </select>
            )
          }
          return (
            <input
              key={o.key}
              className="input"
              type="number"
              min={o.min}
              max={o.max}
              value={optNum(opts, o.key)}
              onChange={(e) => setOpt(o.key, Number(e.target.value))}
              style={{ width: 88, height: 30 }}
              title={o.label}
            />
          )
        })}
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 10, alignItems: 'start' }}>
        <div className="field" style={{ minWidth: 0 }}>
          <label style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>输入</span>
            <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>{input.length} 字符</span>
          </label>
          <textarea
            className="input"
            rows={tool.rows ?? 10}
            placeholder={tool.placeholder}
            value={input}
            onChange={(e) => setInput(e.target.value)}
            spellCheck={false}
          />
        </div>
        <div className="field" style={{ minWidth: 0 }}>
          <label style={{ display: 'flex', justifyContent: 'space-between' }}>
            <span>输出</span>
            <span style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span style={{ color: 'var(--text-3)', fontWeight: 400 }}>{out.length} 字符</span>
              <CopyBtn text={out} />
            </span>
          </label>
          <OutBox value={out} rows={tool.rows ?? 10} error={error} />
        </div>
      </div>
      {error && (
        <div style={{ fontSize: 12, color: 'var(--danger, #d64545)', whiteSpace: 'pre-wrap' }}>
          {error}
        </div>
      )}
    </div>
  )
}

/* ---------------- 页面 ---------------- */

export default function DevToolsPage() {
  const [plugin, setPlugin] = useState<PluginInfo | null>(null)
  const [loading, setLoading] = useState(true)
  const [activeId, setActiveId] = useState('')
  const [query, setQuery] = useState('')

  useEffect(() => {
    void api
      .pluginsState()
      .then((st) => setPlugin(st.plugins.find((p) => p.id === 'devkit') ?? null))
      .catch(() => setPlugin(null))
      .finally(() => setLoading(false))
  }, [])

  const groups = useMemo(() => {
    if (!plugin) return []
    return plugin.groups.map((g) => ({ ...g, items: resolve(g.tools) }))
  }, [plugin])

  const flat = useMemo(() => groups.flatMap((g) => g.items), [groups])

  const active = useMemo(() => {
    const hit = flat.find((t) => (isMissing(t) ? t.missing : t.id) === activeId)
    if (hit) return hit
    const first = flat.find((t) => !isMissing(t))
    return first ?? null
  }, [flat, activeId])

  useEffect(() => {
    if (!activeId && flat.length > 0) {
      const first = flat.find((t) => !isMissing(t))
      if (first && !isMissing(first)) setActiveId(first.id)
    }
  }, [activeId, flat])

  const q = query.trim().toLowerCase()
  const groupsView = groups
    .map((g) => ({
      ...g,
      items: q
        ? g.items.filter((t) =>
            isMissing(t)
              ? t.missing.toLowerCase().includes(q)
              : `${t.name} ${t.desc} ${t.id}`.toLowerCase().includes(q)
          )
        : g.items
    }))
    .filter((g) => g.items.length > 0)

  if (loading) {
    return (
      <>
        <div className="page-head">
          <h1>开发者工具箱</h1>
        </div>
        <div className="page-body">
          <div className="empty" style={{ height: 240 }}>
            <span>加载中…</span>
          </div>
        </div>
      </>
    )
  }

  if (!plugin) {
    return (
      <>
        <div className="page-head">
          <h1>开发者工具箱</h1>
        </div>
        <div className="page-body">
          <div className="empty" style={{ height: 280 }}>
            <IconPlugin size={34} />
            <span>没有找到 devkit 插件</span>
            <span style={{ fontSize: 11.5, maxWidth: 460, textAlign: 'center', lineHeight: 1.7 }}>
              工具箱靠 <code>plugins/devkit.plugin.json</code> 上架工具。文件被删了，
              从安装目录的 plugins 里找回来，或去插件页点「重新加载」。
            </span>
          </div>
        </div>
      </>
    )
  }

  if (!plugin.enabled) {
    return (
      <>
        <div className="page-head">
          <h1>开发者工具箱</h1>
        </div>
        <div className="page-body">
          <div className="empty" style={{ height: 280 }}>
            <IconPlugin size={34} />
            <span>开发者工具箱已停用</span>
            <span style={{ fontSize: 11.5, maxWidth: 420, textAlign: 'center', lineHeight: 1.7 }}>
              去「插件」页把「{plugin.name}」的开关打开，工具就回来了。
            </span>
          </div>
        </div>
      </>
    )
  }

  return (
    <>
      <div className="page-head">
        <h1>开发者工具箱</h1>
        <span className="tag" style={{ height: 18, fontSize: 10 }}>
          {plugin.tools.length} 个工具
        </span>
        <div className="spacer" />
        <input
          className="input"
          placeholder="搜索工具"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          style={{ width: 200, height: 30 }}
        />
      </div>

      <div className="page-body">
        <div style={{ display: 'grid', gridTemplateColumns: '156px minmax(0, 1fr)', gap: 14, alignItems: 'start' }}>
          <div style={{ display: 'flex', flexDirection: 'column', gap: 14, position: 'sticky', top: 0 }}>
            {groupsView.map((g) => (
              <div key={g.id} style={{ display: 'flex', flexDirection: 'column', gap: 2 }}>
                <div
                  style={{
                    fontSize: 10.5,
                    fontWeight: 600,
                    color: 'var(--text-3)',
                    letterSpacing: '0.04em',
                    padding: '0 8px 4px'
                  }}
                >
                  {g.name}
                </div>
                {g.items.map((t) => {
                  const id = isMissing(t) ? t.missing : t.id
                  return (
                    <button
                      key={id}
                      className={`devkit-tool ${activeId === id ? 'active' : ''} ${isMissing(t) ? 'missing' : ''}`}
                      data-tool={id}
                      onClick={() => setActiveId(id)}
                      title={isMissing(t) ? '清单里有，注册表里没有' : t.desc}
                    >
                      <span
                        style={{
                          overflow: 'hidden',
                          textOverflow: 'ellipsis',
                          whiteSpace: 'nowrap'
                        }}
                      >
                        {isMissing(t) ? t.missing : t.name}
                      </span>
                    </button>
                  )
                })}
              </div>
            ))}
          </div>

          <div>
            {!active ? (
              <div className="empty" style={{ height: 240 }}>
                <span>没有匹配的工具</span>
              </div>
            ) : isMissing(active) ? (
              <div className="setting-card" style={{ padding: 16, fontSize: 12.5, color: 'var(--text-2)' }}>
                清单里声明了 <code>{active.missing}</code>，但代码里没有实现。
                <div style={{ marginTop: 6, color: 'var(--text-3)', fontSize: 11.5 }}>
                  要么把 id 写错了，要么这个工具还没写完。清单只负责上架，实现必须在
                  <code> src/lib/devkit/tools/</code> 里注册。
                </div>
              </div>
            ) : (
              <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
                <div>
                  <div style={{ fontSize: 13, fontWeight: 600 }}>{active.name}</div>
                  <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 2 }}>{active.desc}</div>
                </div>
                {active.kind === 'panel' ? (
                  <PanelHost tool={active} />
                ) : (
                  <TransformView tool={active} />
                )}
              </div>
            )}
          </div>
        </div>
      </div>
    </>
  )
}
