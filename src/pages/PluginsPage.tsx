import { useCallback, useEffect, useState } from 'react'
import { api } from '../lib/api'
import type { PluginState } from '../lib/types'
import { IconPlugin, IconSearch } from '../components/Icons'
import { useToast } from '../components/Toast'

export default function PluginsPage() {
  const [state, setState] = useState<PluginState | null>(null)
  const toast = useToast()

  const reload = useCallback(async () => {
    try {
      setState(await api.pluginsState())
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }, [toast])

  useEffect(() => {
    void reload()
  }, [reload])

  return (
    <>
      <div className="page-head">
        <h1>插件</h1>
        <div className="spacer" />
        <button className="btn sm" onClick={() => void api.pluginsOpenDir('user')}>
          打开插件目录
        </button>
        <button
          className="btn sm"
          onClick={async () => {
            try {
              setState(await api.pluginsReload())
              toast.push('已重新加载', 'success')
            } catch (e) {
              toast.push(String(e), 'error')
            }
          }}
        >
          重新加载
        </button>
      </div>

      <div className="page-body">
        {!state || state.plugins.length === 0 ? (
          <div className="empty" style={{ height: 280 }}>
            <IconPlugin size={34} />
            <span>还没有安装插件</span>
            <span style={{ fontSize: 11.5, maxWidth: 420, textAlign: 'center', lineHeight: 1.7 }}>
              把 <code>.uploader.json</code> 丢进插件目录即可新增图床，不需要写代码也不需要重启
            </span>
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 8, maxWidth: 680 }}>
            {state.plugins.map((p) => (
              <div
                key={p.dir}
                className="setting-card"
                style={{ display: 'flex', alignItems: 'center', gap: 14, padding: '12px 16px' }}
              >
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 7 }}>
                    <strong style={{ fontSize: 13 }}>{p.name}</strong>
                    <span className="tag" style={{ height: 17, fontSize: 10 }}>
                      v{p.version}
                    </span>
                    <span className={`tag ${p.origin === 'builtin' ? '' : 'accent'}`} style={{ height: 17, fontSize: 10 }}>
                      {p.origin === 'builtin' ? '内置' : '用户'}
                    </span>
                    {p.error && (
                      <span className="tag danger" style={{ height: 17, fontSize: 10 }}>
                        {p.error}
                      </span>
                    )}
                  </div>
                  <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 2 }}>
                    {p.description ?? p.id}
                    {p.providers.length > 0 && ` · 图床：${p.providers.join(', ')}`}
                  </div>
                </div>
                <button
                  className={`switch ${p.enabled ? 'on' : ''}`}
                  onClick={async () => {
                    try {
                      setState(await api.pluginsToggle(p.id, !p.enabled))
                    } catch (e) {
                      toast.push(String(e), 'error')
                    }
                  }}
                />
              </div>
            ))}
          </div>
        )}

        <div
          style={{
            marginTop: 22,
            maxWidth: 680,
            padding: 16,
            background: 'var(--surface)',
            border: '1px solid var(--border)',
            borderRadius: 'var(--radius)',
            fontSize: 12,
            color: 'var(--text-2)',
            lineHeight: 1.8
          }}
        >
          <strong style={{ fontSize: 12.5, color: 'var(--text)' }}>写一个图床插件</strong>
          <p style={{ marginTop: 6, color: 'var(--text-3)' }}>
            在插件目录新建 <code>my-host.uploader.json</code>：
          </p>
          <pre
            style={{
              marginTop: 8,
              padding: 10,
              borderRadius: 7,
              background: 'var(--surface-3)',
              fontFamily: 'var(--mono)',
              fontSize: 11,
              overflowX: 'auto',
              lineHeight: 1.6
            }}
          >{`{
  "id": "my-host",
  "name": "我的图床",
  "version": "1.0.0",
  "provider": {
    "id": "my-host",
    "name": "我的图床",
    "url": "https://api.example.com/upload",
    "fieldName": "file",
    "headers": { "Authorization": "Bearer {{token}}" },
    "responsePath": "data.url",
    "config_fields": [
      { "key": "token", "label": "访问令牌", "type": "password", "required": true }
    ]
  }
}`}</pre>
          <p style={{ marginTop: 8, color: 'var(--text-3)' }}>
            <code>{'{{token}}'}</code> 会被替换成你在配置里填的值。配置项会自动生表单。
          </p>
        </div>
      </div>
    </>
  )
}
