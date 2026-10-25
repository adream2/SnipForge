import { useEffect, useState } from 'react'
import { api } from '../lib/api'
import type { AppSettings, ConfigField, ProviderMeta } from '../lib/types'
import { IconCheck, IconLoader, IconPlugin } from '../components/Icons'
import { useToast } from '../components/Toast'

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

export default function UploadPage({
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
    <>
      <div className="page-head">
        <h1>图床</h1>
        <div className="spacer" />
        <button className="btn sm" onClick={() => void save()} disabled={!current}>
          {saved ? <IconCheck size={13} /> : null} 保存并设为默认
        </button>
      </div>

      <div className="page-body">
        <div className="provider-tabs">
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
        </div>

        {current ? (
          <div className="provider-form">
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
          </div>
        ) : (
          <div className="empty" style={{ height: 220 }}>
            <IconPlugin size={30} />
            <span>选择一个图床开始配置</span>
          </div>
        )}

        <div style={{ marginTop: 18, maxWidth: 560, fontSize: 12, color: 'var(--text-3)', lineHeight: 1.7 }}>
          没找到合适的图床？到「插件」页放一个 <code>.uploader.json</code> 就能接入任意 HTTP 图床，无需写代码。
        </div>
      </div>
    </>
  )
}
