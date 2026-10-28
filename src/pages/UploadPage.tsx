import { useState } from 'react'
import type { AppSettings } from '../lib/types'
import UploadTab from './upload/UploadTab'
import ListTab from './upload/ListTab'
import ConfigTab from './upload/ConfigTab'

/**
 * 图床 = 一个入口 + 三个 tab：上传 / 列表 / 配置。
 *
 * 以前「图床」和「图床配置」是左侧导航的两个并列项，同一件事被拆在两处，
 * 用户得在页面之间来回跳；而且"配置"占了跟"图床"一样的导航权重，
 * 实际一年也点不了两次。同一件事的东西就该收在一个入口里。
 */
type Tab = 'upload' | 'list' | 'config'

const TABS: Array<{ id: Tab; label: string }> = [
  { id: 'upload', label: '上传' },
  { id: 'list', label: '列表' },
  { id: 'config', label: '配置' }
]

export default function UploadPage({
  settings,
  onChange
}: {
  settings: AppSettings | null
  onChange: (patch: unknown) => Promise<AppSettings>
}) {
  const [tab, setTab] = useState<Tab>('upload')

  return (
    <>
      <div className="page-head">
        <h1>图床</h1>
        <div className="page-tabs">
          {TABS.map((t) => (
            <button
              key={t.id}
              className={tab === t.id ? 'active' : ''}
              onClick={() => setTab(t.id)}
            >
              {t.label}
            </button>
          ))}
        </div>
        <div className="spacer" />
        <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
          {tab === 'upload' && '传图前按「配置」里的规则压缩'}
          {tab === 'list' && '点图片即复制链接'}
          {tab === 'config' && '账号只存本地'}
        </span>
      </div>

      <div className="page-body">
        {tab === 'upload' && <UploadTab settings={settings} onChange={onChange} />}
        {tab === 'list' && <ListTab settings={settings} />}
        {tab === 'config' && <ConfigTab settings={settings} onChange={onChange} />}
      </div>
    </>
  )
}
