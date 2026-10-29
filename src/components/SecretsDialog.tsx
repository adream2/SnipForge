import { useCallback, useEffect, useRef, useState } from 'react'
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
  /** 右键菜单 */
  const [menu, setMenu] = useState<{ x: number; y: number; id: string; armed?: boolean } | null>(
    null
  )
  /** 就地改名。名字错了想改一个词，不该为此打开整个编辑表单 */
  const [renaming, setRenaming] = useState<{ id: string; name: string } | null>(null)
  /** Esc 取消时别让 blur 又把它提交上去 */
  const cancelRename = useRef(false)

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

  const startRename = (s: Secret) => {
    setMenu(null)
    cancelRename.current = false
    setRenaming({ id: s.id, name: s.name })
  }

  /** 提交改名。空名字当取消处理，别把名称变成空的。 */
  const commitRename = async () => {
    const cur = renaming
    if (!cur) return
    setRenaming(null)
    if (cancelRename.current) return
    const name = cur.name.trim()
    const old = list.find((x) => x.id === cur.id)
    if (!old || old.name === name) return
    if (!name) {
      toast.push('名称不能为空', 'error')
      return
    }
    try {
      await api.secretsRename(cur.id, name)
      // 本地改一处就行：后端改名不排序，重新 load 反而是白跑一趟
      setList((prev) => prev.map((x) => (x.id === cur.id ? { ...x, name } : x)))
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const openMenu = (e: React.MouseEvent, s: Secret) => {
    e.preventDefault()
    e.stopPropagation()
    setMenu({
      // 靠右/靠下时往回收，别让菜单跑到窗口外面去
      x: Math.min(e.clientX, window.innerWidth - 208),
      y: Math.min(e.clientY, window.innerHeight - 220),
      id: s.id
    })
  }

  // 菜单开着时 Esc 关掉
  useEffect(() => {
    if (!menu) return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setMenu(null)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [menu])

  const menuTarget = menu ? list.find((x) => x.id === menu.id) : undefined

  return (
    <>
      <div className="ctx-shade" onClick={onClose} />
      <div className="secrets-dialog">
        <header>
          <div>
            <strong>收藏</strong>
            <span className="secrets-sub">
              密钥、密码、常用私密文本 · 只存本地 · 右键可改名
            </span>
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
            <div
              className="secret-row"
              key={s.id}
              onContextMenu={(e) => openMenu(e, s)}
              onDoubleClick={() => startRename(s)}
              title="右键：重命名 / 编辑 / 删除（双击也能改名）"
            >
              <div className="secret-info">
                {renaming?.id === s.id ? (
                  <input
                    className="input secret-rename"
                    value={renaming.name}
                    autoFocus
                    spellCheck={false}
                    onFocus={(e) => e.currentTarget.select()}
                    onChange={(e) => setRenaming({ id: s.id, name: e.target.value })}
                    onKeyDown={(e) => {
                      // 这里的按键不能冒泡到对话框/页面上去
                      e.stopPropagation()
                      if (e.key === 'Enter') {
                        e.preventDefault()
                        void commitRename()
                      } else if (e.key === 'Escape') {
                        e.preventDefault()
                        cancelRename.current = true
                        setRenaming(null)
                      }
                    }}
                    onBlur={() => void commitRename()}
                  />
                ) : (
                  <div className="secret-name">
                    {s.name}
                    {s.note && <span className="secret-note">{s.note}</span>}
                  </div>
                )}
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

      {/* 右键菜单：和图片工具页同一套样式 */}
      {menu && menuTarget && (
        <>
          <div
            className="ctx-shade"
            style={{ zIndex: 70 }}
            onClick={() => setMenu(null)}
            onContextMenu={(e) => {
              e.preventDefault()
              setMenu(null)
            }}
          />
          <div className="ctx-menu" style={{ left: menu.x, top: menu.y, zIndex: 71 }}>
            <button onClick={() => startRename(menuTarget)}>
              <span>重命名</span>
            </button>
            <button
              onClick={() => {
                setMenu(null)
                setDraft({
                  id: menuTarget.id,
                  name: menuTarget.name,
                  value: menuTarget.value,
                  note: menuTarget.note
                })
              }}
            >
              <span>编辑内容…</span>
            </button>
            <button
              onClick={() => {
                setMenu(null)
                void copy(menuTarget)
              }}
            >
              <span>复制内容</span>
            </button>
            <button
              onClick={() => {
                setMenu(null)
                setRevealed((r) => ({ ...r, [menuTarget.id]: !r[menuTarget.id] }))
              }}
            >
              <span>{revealed[menuTarget.id] ? '隐藏内容' : '显示内容'}</span>
            </button>
            <div className="ctx-sep" />
            {/* 收藏里都是密钥，误删真的会想捶自己：删除也走两段式，
                但确认就在菜单里完成，免得菜单一关就看不到"刚才那下做了什么" */}
            <button
              onClick={() => {
                if (!menu.armed) {
                  setMenu({ ...menu, armed: true })
                  window.setTimeout(
                    () => setMenu((m) => (m && m.id === menuTarget.id ? { ...m, armed: false } : m)),
                    3000
                  )
                  return
                }
                setMenu(null)
                void (async () => {
                  try {
                    await api.secretsRemove(menuTarget.id)
                    await load()
                    toast.push('已删除', 'success')
                  } catch (e) {
                    toast.push(String(e), 'error')
                  }
                })()
              }}
            >
              <span style={{ color: 'var(--danger)' }}>
                {menu.armed ? '确认删除？' : '删除'}
              </span>
            </button>
          </div>
        </>
      )}
    </>
  )
}
