import { useEffect, useRef, useState } from 'react'

/**
 * 主密码输入框。
 *
 * 不用 `window.prompt`：WebView 里的 prompt 在一些版本会被静默忽略，
 * 而且没法显示"两次输入不一致"这类即时错误。
 */
export function PasswordDialog({
  title,
  desc,
  submitLabel = '确定',
  confirm = false,
  onSubmit,
  onClose
}: {
  title: string
  desc?: string
  submitLabel?: string
  /** 需要输入两次（设置 / 修改密码） */
  confirm?: boolean
  /** 提交；抛错则把错误显示在框里，不关闭 */
  onSubmit: (password: string) => Promise<void>
  onClose: () => void
}) {
  const [pw, setPw] = useState('')
  const [pw2, setPw2] = useState('')
  const [err, setErr] = useState('')
  const [busy, setBusy] = useState(false)
  const first = useRef<HTMLInputElement>(null)

  useEffect(() => {
    first.current?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [onClose])

  const submit = async () => {
    if (busy) return
    if (pw.length < 8) {
      setErr('至少 8 个字符')
      return
    }
    if (confirm && pw !== pw2) {
      setErr('两次输入不一致')
      return
    }
    setBusy(true)
    setErr('')
    try {
      await onSubmit(pw)
      onClose()
    } catch (e) {
      setErr(e instanceof Error ? e.message : String(e))
      setBusy(false)
    }
  }

  return (
    <div className="pw-mask" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="pw-box">
        <h3>{title}</h3>
        {desc && <p className="pw-desc">{desc}</p>}
        <input
          ref={first}
          className="input"
          type="password"
          autoComplete="new-password"
          placeholder="主密码"
          value={pw}
          onChange={(e) => setPw(e.target.value)}
          onKeyDown={(e) => e.key === 'Enter' && !confirm && void submit()}
        />
        {confirm && (
          <input
            className="input"
            type="password"
            autoComplete="new-password"
            placeholder="再输一次"
            value={pw2}
            onChange={(e) => setPw2(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && void submit()}
          />
        )}
        {err && <div className="pw-err">{err}</div>}
        <div className="pw-actions">
          <button className="btn sm" onClick={onClose} disabled={busy}>
            取消
          </button>
          <button className="btn sm primary" onClick={() => void submit()} disabled={busy}>
            {busy ? '处理中…' : submitLabel}
          </button>
        </div>
      </div>
    </div>
  )
}
