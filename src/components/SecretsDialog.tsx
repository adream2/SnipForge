import { useCallback, useEffect, useState } from 'react'
import { api } from '../lib/api'
import type { Secret } from '../lib/types'
import { IconCheck, IconCopy, IconLoader, IconTrash } from './Icons'
import { useToast } from './Toast'

/**
 * 密钥 / 密码收藏的管理面板。
 *
 * 为什么单独做一份而不是塞进剪贴板历史：
 * 历史会被容量淘汰、会被「清空」按钮清光，而这些东西是用户主动维护的，
 * 不该跟着滚动消失。
 *
 * 内容默认打码 —— 这类窗口很可能是在旁边有人时打开的。
 */

type Draft = { id?: string; name: string; value: string; note: string }

const EMPTY: Draft = { name: '', value: '', note: '' }

export default function SecretsDialog({ onClose }: { onClose: () => void }) {
  const toast = useToast()
  const [list, setList] = useState<Secret[]>([])
  const [loading, setLoading] = useState(true)
  const [draft, setDraft] = useState<Draft | null>(null)
  const [saving, setSaving] = useState(false)
  /** 哪些条目暂时显示明文 */
  const [revealed, setRevealed] = useState<Record<string, boolean>>({})
  /** 删除的两段式确认 */
  const [confirmId, setConfirmId] = useState<string | null>(null)
  const [copied, setCopied] = useState<string | null>(null)

  const load = useCallback(async () => {
    setLoading(true)
    try {
      setList(await api.secretsList())
    } catch {
      setList([])
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void load()
  }, [load])

  const save = async () => {
    if (!draft) return
    if (!draft.name.trim()) {
      toast.push('给这条收藏起个名字', 'error')
      return
    }
    if (!draft.value) {
      toast.push('内容不能为空', 'error')
      return
    }
    setSaving(true)
    try {
      await api.secretsSave(draft)
      setDraft(null)
      await load()
      toast.push('已保存', 'success')
    } catch (e) {
      toast.push(String(e), 'error')
    } finally {
      setSaving(false)
    }
  }

  const copy = async (s: Secret) => {
    try {
      await api.clipboardCopyText(s.value)
      setCopied(s.id)
      window.setTimeout(() => setCopied((c) => (c === s.id ? null : c)), 1200)
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const remove = async (s: Secret) => {
    if (confirmId !== s.id) {
      setConfirmId(s.id)
      window.setTimeout(() => setConfirmId((c) => (c === s.id ? null : c)), 3000)
      return
    }
    try {
      await api.secretsRemove(s.id)
      setConfirmId(null)
      await load()
      toast.push('已删除', 'success')
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  return (
    <>
      <div className="ctx-shade" onClick={onClose} />
      <div className="secrets-dialog">
        <header>
          <div>
            <strong>收藏</strong>
            <span className="secrets-sub">密钥、密码、常用私密文本 · 只存本地</span>
          </div>
          <div style={{ flex: 1 }} />
          <button
            className="btn sm"
            onClick={() => setDraft({ ...EMPTY })}
            disabled={!!draft}
          >
            ＋ 新增
          </button>
          <button className="btn sm" onClick={onClose}>
            关闭
          </button>
        </header>

        <div className="secrets-body">
          {/* 编辑表单 */}
          {draft && (
            <div className="secret-form">
              <input
                className="input"
                placeholder="名称，例如 GitHub Token / 公司 Wi-Fi"
                value={draft.name}
                autoFocus
                onChange={(e) => setDraft({ ...draft, name: e.target.value })}
              />
              <textarea
                className="input"
                placeholder="内容：真正要粘贴出去的那一串"
                value={draft.value}
                rows={3}
                onChange={(e) => setDraft({ ...draft, value: e.target.value })}
              />
              <input
                className="input"
                placeholder="备注（可选）：用在哪、有什么权限"
                value={draft.note}
                onChange={(e) => setDraft({ ...draft, note: e.target.value })}
              />
              <div className="secret-form-actions">
                <button className="btn primary sm" onClick={() => void save()} disabled={saving}>
                  {saving ? <IconLoader /> : null} 保存
                </button>
                <button className="btn sm" onClick={() => setDraft(null)}>
                  取消
                </button>
              </div>
            </div>
          )}

          {loading && <div className="empty" style={{ height: 120 }}><IconLoader /><span>读取中…</span></div>}

          {!loading && list.length === 0 && !draft && (
            <div className="empty" style={{ height: 140 }}>
              <span>还没有收藏。点「新增」把常用的密钥存进来，之后在面板里一键粘贴。</span>
            </div>
          )}

          {list.map((s) => (
            <div className="secret-row" key={s.id}>
              <div className="secret-info">
                <div className="secret-name">
                  {s.name}
                  {s.note && <span className="secret-note">{s.note}</span>}
                </div>
                <div className="secret-value">
                  {revealed[s.id] ? s.value : '••••••••••••'}
                  <button
                    className="secret-eye"
                    onClick={() => setRevealed((r) => ({ ...r, [s.id]: !r[s.id] }))}
                  >
                    {revealed[s.id] ? '隐藏' : '显示'}
                  </button>
                </div>
              </div>

              <button className="btn sm" title="复制内容" onClick={() => void copy(s)}>
                {copied === s.id ? <IconCheck size={12} /> : <IconCopy size={12} />}
              </button>
              <button
                className="btn sm"
                title="编辑"
                onClick={() =>
                  setDraft({ id: s.id, name: s.name, value: s.value, note: s.note })
                }
              >
                编辑
              </button>
              <button
                className={`btn sm danger ${confirmId === s.id ? 'armed' : ''}`}
                title="删除"
                onClick={() => void remove(s)}
              >
                {confirmId === s.id ? '确认删除' : <IconTrash size={12} />}
              </button>
            </div>
          ))}
        </div>

        <footer className="secrets-foot">
          内容以明文保存在数据目录的 secrets.json —— 本机工具，没做主密码加密。
          能访问这台电脑的人就能读到它。
        </footer>
      </div>
    </>
  )
}
