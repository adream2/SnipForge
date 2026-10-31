import { useCallback, useEffect, useRef, useState } from 'react'
import { api } from '../lib/api'
import type { HostsBackup, HostsInfo } from '../lib/types'
import { useToast } from '../components/Toast'
import { IconLoader, IconRefresh, IconSave, IconFolder, IconHistory } from '../components/Icons'

/**
 * hosts 编辑器。
 *
 * 为什么值得单开一页：改 hosts 是开发时的日常动作（本地域名、灰度开关），
 * 但 Windows 上它有两道坎 —— 路径深、默认没有写权限。这里把两道坎都抹平：
 * 路径不用记，保存时自己提权（弹一次 UAC），覆盖前自动备份。
 *
 * 刻意**不做**语法校验：hosts 就是一列 `IP 域名`，写错只会不生效，
 * 不会像配置那样把程序搞坏。加校验反而会拦住用户想写的临时条目。
 */
export default function HostsPage() {
  const toast = useToast()
  const [info, setInfo] = useState<HostsInfo | null>(null)
  const [text, setText] = useState('')
  const [dirty, setDirty] = useState(false)
  const [busy, setBusy] = useState<'' | 'load' | 'save' | 'restore'>('')
  const [backups, setBackups] = useState<HostsBackup[] | null>(null)
  const areaRef = useRef<HTMLTextAreaElement>(null)

  const load = useCallback(async () => {
    setBusy('load')
    try {
      const r = await api.hostsInfo()
      setInfo(r)
      setText(r.text)
      setDirty(false)
    } catch (e) {
      toast.push(String(e), 'error')
    } finally {
      setBusy('')
    }
  }, [toast])

  useEffect(() => {
    void load()
  }, [load])

  // 从手势进来的人下一步就是改字，光标直接落在编辑区，省一次点击
  useEffect(() => {
    if (info) areaRef.current?.focus()
  }, [info])

  const save = async () => {
    if (busy) return
    setBusy('save')
    try {
      const r = await api.hostsSave(text)
      toast.push(
        r.elevated ? '已保存（通过管理员权限写入）' : '已保存',
        'success'
      )
      if (r.backup) toast.push(`原文件已备份为 ${r.backup}`, 'success')
      await load()
    } catch (e) {
      // 用户在 UAC 上点取消也走这里：必须说清"没保存"，不能让人以为改好了
      toast.push(String(e), 'error')
    } finally {
      setBusy('')
    }
  }

  // Ctrl+S 保存：文本编辑器里这是肌肉记忆，没有会很别扭。
  // 用 ref 拿最新的 save，监听只挂一次（否则每敲一个字都要重挂）
  const saveRef = useRef(save)
  saveRef.current = save
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
        e.preventDefault()
        void saveRef.current()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [])

  const reload = async () => {
    if (dirty && !window.confirm('当前修改还没保存，重新载入会丢掉它们。继续？')) return
    await load()
  }

  const openBackups = async () => {
    try {
      setBackups(await api.hostsBackups())
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const restore = async (name: string) => {
    if (!window.confirm(`用备份 ${name} 覆盖当前 hosts？`)) return
    setBusy('restore')
    try {
      const r = await api.hostsRestore(name)
      toast.push(r.elevated ? '已恢复（通过管理员权限写入）' : '已恢复', 'success')
      await load()
    } catch (e) {
      toast.push(String(e), 'error')
    } finally {
      setBusy('')
    }
  }

  const lines = text ? text.split(/\r\n|\r|\n/).length : 0

  return (
    <>
      <div className="page-head">
        <h1>Hosts</h1>
        <div className="spacer" />
        <button className="btn sm" onClick={() => void api.hostsReveal()} title="在资源管理器里选中这个文件">
          <IconFolder size={14} /> 打开位置
        </button>
        <button className="btn sm" onClick={() => void reload()} disabled={!!busy}>
          {busy === 'load' ? <IconLoader /> : <IconRefresh size={14} />} 重新载入
        </button>
        <button className="btn sm primary" onClick={() => void save()} disabled={!!busy}>
          {busy === 'save' ? <IconLoader /> : <IconSave size={14} />} 保存
        </button>
      </div>

      <div style={{ padding: '0 20px 18px', maxWidth: 1000 }}>
        <div className="hosts-path">{info?.path ?? '读取中…'}</div>

        {info && !info.elevated && (
          <div className="hosts-note">
            保存时会弹出 <b>UAC</b> 询问是否允许修改设备 —— 因为 hosts 在
            <code> System32</code> 下，普通权限写不进去。点「是」才会真正写入；
            点「否」会明确告诉你<strong>没有保存</strong>。
          </div>
        )}
        {info?.elevated && (
          <div className="hosts-note ok">
            当前以管理员身份运行，保存直接写入，不会再弹 UAC。
          </div>
        )}
        {info?.nonUtf8 && (
          <div className="hosts-note warn">
            这个文件不是 UTF-8 编码（可能含中文注释）。保存后会转成 UTF-8，
            非 ASCII 字符可能变成乱码 —— 介意的话先备份。
          </div>
        )}
        {info && info.eol !== 'crlf' && (
          <div className="hosts-note warn">
            这个文件现在用的是{' '}
            {info.eol === 'cr'
              ? '纯 CR 换行（老编辑器写的老格式，记事本打开会挤成一整行）'
              : info.eol === 'lf'
                ? 'LF 换行（Unix 风格）'
                : '混着几种换行'}
            ，保存时统一成 Windows 的 CRLF。
          </div>
        )}

        <textarea
          ref={areaRef}
          className="hosts-editor"
          spellCheck={false}
          value={text}
          placeholder="# 格式：IP 域名  （例如 127.0.0.1  dev.local）"
          onChange={(e) => {
            setText(e.target.value)
            setDirty(true)
          }}
        />

        <div className="hosts-foot">
          <span>{lines} 行</span>
          <span>{text.length} 字符</span>
          {dirty && <span className="hosts-dirty">未保存</span>}
          <div style={{ flex: 1 }} />
          <button className="btn sm" onClick={() => void openBackups()}>
            <IconHistory size={14} /> 备份
          </button>
        </div>

        {backups && (
          <div className="hosts-backups">
            <header>
              <strong>备份</strong>
              <span>每次保存前自动存一份，最多显示 50 个</span>
              <div style={{ flex: 1 }} />
              <button className="btn sm" onClick={() => void api.hostsOpenBackupDir()}>
                打开目录
              </button>
              <button className="btn sm" onClick={() => setBackups(null)}>
                收起
              </button>
            </header>
            {backups.length === 0 && <div className="hosts-empty">还没有备份</div>}
            {backups.map((b) => (
              <div className="hosts-backup-row" key={b.name}>
                <span className="name">{b.name}</span>
                <span className="size">{(b.size / 1024).toFixed(1)} KB</span>
                <button
                  className="btn sm"
                  disabled={!!busy}
                  onClick={() => void restore(b.name)}
                >
                  恢复
                </button>
              </div>
            ))}
          </div>
        )}
      </div>
    </>
  )
}
