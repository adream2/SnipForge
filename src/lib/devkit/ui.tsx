/** 工具箱公用 UI：复制按钮、只读输出框、结果行。 */

import { api } from '../api'
import { useToast } from '../../components/Toast'
import { IconCopy } from '../../components/Icons'
import type { ReactNode } from 'react'

export function CopyBtn({ text, label }: { text: string; label?: string }) {
  const toast = useToast()
  return (
    <button
      className={`btn sm ${label ? '' : 'icon'}`}
      title="复制"
      disabled={!text}
      onClick={async (e) => {
        e.stopPropagation()
        if (!text) return
        try {
          await api.clipboardCopyText(text)
          toast.push('已复制', 'success')
        } catch (err) {
          toast.push(String(err), 'error')
        }
      }}
    >
      {/* 行尾的小复制按钮没有文字，得给个图标，否则是个看不出用途的白方块 */}
      {label || <IconCopy size={12} />}
    </button>
  )
}

/** 只读输出框。readOnly 而不是 div：能选中、能 Ctrl+C、能 Ctrl+A */
export function OutBox({
  value,
  rows = 6,
  error
}: {
  value: string
  rows?: number
  error?: string | null
}) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
      <textarea
        className="input"
        readOnly
        rows={rows}
        value={error ? '' : value}
        placeholder={error ?? ''}
        style={{
          color: error ? 'var(--danger, #d64545)' : undefined,
          background: 'var(--surface-2, #fafafa)'
        }}
      />
    </div>
  )
}

/** 一行"标签 → 值 + 复制" */
export function ResultRow({ label, value }: { label: string; value: string }) {
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 10, padding: '3px 0' }}>
      <span style={{ width: 88, flexShrink: 0, fontSize: 11.5, color: 'var(--text-3)' }}>{label}</span>
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
  )
}

export function PanelSection({
  title,
  children,
  right
}: {
  title: string
  children: ReactNode
  right?: ReactNode
}) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', gap: 8 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
        <span style={{ fontSize: 12, fontWeight: 600, color: 'var(--text-2)' }}>{title}</span>
        <div style={{ flex: 1 }} />
        {right}
      </div>
      {children}
    </div>
  )
}
