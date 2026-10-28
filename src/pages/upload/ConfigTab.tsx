import { useEffect, useState } from 'react'
import { api } from '../../lib/api'
import type { AppSettings, ConfigField, ProviderMeta } from '../../lib/types'
import { IconCheck, IconLoader, IconPlugin } from '../../components/Icons'
import { useToast } from '../../components/Toast'

function FieldInput({
  field,
  value,
  onChange,
  values
}: {
  field: ConfigField
  value: unknown
  onChange: (v: unknown) => void
  values: Record<string, unknown>
}) {
  if (field.visibleWhen && values[field.visibleWhen.key] !== field.visibleWhen.equals) return null

  const common = { className: 'input', placeholder: field.placeholder }

  return (
    <div className="field">
      <label>
        {field.label}
        {field.required && <span style={{ color: 'var(--danger)', marginLeft: 2 }}>*</span>}
      </label>
      {field.fieldType === 'textarea' ? (
        <textarea
          {...common}
          value={String(value ?? '')}
          onChange={(e) => onChange(e.target.value)}
        />
      ) : field.fieldType === 'select' ? (
        <select {...common} value={String(value ?? '')} onChange={(e) => onChange(e.target.value)}>
          {field.options?.map((o) => (
            <option key={o.value} value={o.value}>
              {o.label}
            </option>
          ))}
        </select>
      ) : field.fieldType === 'boolean' ? (
        <button
          className={`switch ${value ? 'on' : ''}`}
          onClick={() => onChange(!value)}
          style={{ marginTop: 4 }}
        />
      ) : (
        <input
          {...common}
          type={field.fieldType === 'password' ? 'password' : 'text'}
          value={String(value ?? '')}
          onChange={(e) => onChange(e.target.value)}
        />
      )}
      {field.description && <span className="hint">{field.description}</span>}
    </div>
  )
}

/** 图床「配置」tab：只管"往哪儿传"。传图在「上传」，看已传的在「列表」。 */
export default function ConfigTab({
  settings,
  onChange
}: {
  settings: AppSettings | null
  onChange: (patch: unknown) => Promise<AppSettings>
}) {
  const [providers, setProviders] = useState<ProviderMeta[]>([])
  const [active, setActive] = useState<string>('github')
  const [draft, setDraft] = useState<Record<string, unknown>>({})
  const [testing, setTesting] = useState(false)
  const [saved, setSaved] = useState(false)
  const toast = useToast()

  useEffect(() => {
    void api.uploadProviders().then(setProviders)
  }, [])

  useEffect(() => {
    if (!settings) return
    setActive(settings.uploader.activeProvider)
  }, [settings])

  useEffect(() => {
    if (!settings) return
    const cfg = settings.uploader.providers[active] ?? {}
    const meta = providers.find((p) => p.id === active)
    const merged: Record<string, unknown> = {}
    for (const f of meta?.fields ?? []) {
      merged[f.key] = cfg[f.key] ?? f.default ?? (f.fieldType === 'boolean' ? false : '')
    }
    setDraft(merged)
    setSaved(false)
  }, [active, settings, providers])

  const current = providers.find((p) => p.id === active)

  const save = async () => {
    await api.uploadProviderSave(active, draft)
    await api.uploadSetActive(active)
    await onChange({ uploader: { activeProvider: active } })
    setSaved(true)
    toast.push('配置已保存', 'success')
    setTimeout(() => setSaved(false), 1800)
  }

  const test = async () => {
    setTesting(true)
    try {
      await api.uploadProviderSave(active, draft)
      const out = await api.uploadTest(active)
      toast.push(`连通正常 · ${out.durationMs}ms · ${out.result.url.slice(0, 60)}`, 'success')
    } catch (e) {
      toast.push(String(e), 'error')
    } finally {
      setTesting(false)
    }
  }

  return (
    <div className="settings-wrap">
      <div className="setting-card">
        {/* 图床切换 + 保存，合成一行 —— 配置页只有一个动作，不值得占一个页头 */}
        <div
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            flexWrap: 'wrap',
            padding: '10px 14px',
            borderBottom: '1px solid var(--border)'
          }}
        >
          {providers.map((p) => (
            <button
              key={p.id}
              className={`provider-tab ${active === p.id ? 'active' : ''}`}
              onClick={() => setActive(p.id)}
            >
              {p.name}
              {p.origin !== 'builtin' && (
                <span className="tag accent" style={{ height: 17, fontSize: 10 }}>
                  插件
                </span>
              )}
            </button>
          ))}
          <div style={{ flex: 1 }} />
          <button className="btn sm" onClick={() => void save()} disabled={!current}>
            {saved ? <IconCheck size={13} /> : null} 保存并设为默认
          </button>
        </div>

        {current ? (
          <div className="provider-form" style={{ padding: '14px 16px' }}>
            <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <strong style={{ fontSize: 13.5 }}>{current.name}</strong>
              {active === settings?.uploader.activeProvider && (
                <span className="tag success">当前默认</span>
              )}
            </div>
            {current.description && (
              <p style={{ fontSize: 12, color: 'var(--text-3)', marginTop: -6 }}>
                {current.description}
              </p>
            )}

            {current.fields.map((f) => (
              <FieldInput
                key={f.key}
                field={f}
                value={draft[f.key]}
                values={draft}
                onChange={(v) => setDraft((prev) => ({ ...prev, [f.key]: v }))}
              />
            ))}

            <div style={{ display: 'flex', gap: 8, paddingTop: 4 }}>
              <button className="btn" onClick={() => void test()} disabled={testing}>
                {testing ? <IconLoader /> : null} 测试连通性
              </button>
            </div>

            {/* GitHub 的「自定义域名」光一个输入框谁也看不懂，给一段能照着做的说明 */}
            {current.id === 'github' && (
              <details className="cfg-help">
                <summary>「自定义域名」怎么配？</summary>
                <div className="cfg-help-body">
                  <p>
                    上传永远走 GitHub API，这个域名只决定<strong>生成什么链接</strong>。
                    最终链接 = 前缀 + <code>/存储路径/文件名</code>：
                  </p>
                  <pre>https://img.example.com/snipforge/20261029_001.png</pre>
                  <p>
                    所以要让这个域名能按<strong>同样的路径</strong>访问到仓库里的文件。
                    常见三种做法：
                  </p>

                  <p className="cfg-help-h">1. Cloudflare Worker 反代（推荐：免费、能缓存、国内可访问）</p>
                  <pre>{`export default {
  async fetch(request) {
    const url = new URL(request.url)
    const base = 'https://raw.githubusercontent.com/用户名/仓库/分支'
    const res = await fetch(base + url.pathname, {
      cf: { cacheTtl: 86400, cacheEverything: true }
    })
    return res
  }
}`}</pre>
                  <p>建好 Worker 后在「设置 → 域和路由 → 自定义域」绑定 img.example.com。</p>

                  <p className="cfg-help-h">2. 自己的服务器 Nginx 反代</p>
                  <pre>{`location / {
  proxy_pass https://raw.githubusercontent.com/用户名/仓库/分支/;
  proxy_cache_valid 200 7d;
}`}</pre>

                  <p className="cfg-help-h">3. GitHub Pages</p>
                  <p>
                    仓库开了 Pages（图片放在发布目录下）的话，
                    <code>https://用户名.github.io/仓库名</code> 也能直接当前缀。
                  </p>

                  <p className="cfg-help-h">注意</p>
                  <ul>
                    <li>jsDelivr <strong>不支持</strong>把自己的域名 CNAME 过去，想用自己的域名就得自己反代</li>
                    <li>前缀要带 <code>https://</code>，末尾斜杠可有可无（会自动去掉）</li>
                    <li>图片是 <code>&lt;img&gt;</code> 直接加载，不需要配 CORS</li>
                    <li>域名没配好只会让<strong>链接</strong>打不开，上传本身是成功的</li>
                  </ul>
                </div>
              </details>
            )}
          </div>
        ) : (
          <div className="empty" style={{ height: 220 }}>
            <IconPlugin size={30} />
            <span>选择一个图床开始配置</span>
          </div>
        )}

        <div
          style={{
            padding: '0 16px 16px',
            fontSize: 12,
            color: 'var(--text-3)',
            lineHeight: 1.7
          }}
        >
          没找到合适的图床？到「插件」页放一个 <code>.uploader.json</code>
          就能接入任意 HTTP 图床，无需写代码。
        </div>
      </div>
    </div>
  )
}
