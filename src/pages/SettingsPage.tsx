import { useCallback, useEffect, useState } from 'react'
import { api, events } from '../lib/api'
import type {
  AppInfo,
  AppSettings,
  OcrDownloadProgress,
  OcrEngineInfo,
  UpdateInfo,
  VaultStatus
} from '../lib/types'
import { useToast } from '../components/Toast'
import { HotkeyInput } from '../components/HotkeyInput'
import { PasswordDialog } from '../components/PasswordDialog'
import { LANGUAGES, PROVIDERS } from '../lib/translate'

function Row({
  title,
  desc,
  children
}: {
  title: string
  desc?: string
  children: React.ReactNode
}) {
  return (
    <div className="setting-row">
      <div className="label">
        <strong>{title}</strong>
        {desc && <span>{desc}</span>}
      </div>
      <div className="control">{children}</div>
    </div>
  )
}

/**
 * 逗号分隔的字符串列表输入。
 *
 * 本地缓冲、失焦（或回车）才提交：如果用「受控 + 实时 split/join」，
 * 用户刚敲一个空格就会被归一化掉，第二个词根本打不进去。
 */
function ListInput({
  value,
  onCommit,
  placeholder
}: {
  value: string[]
  onCommit: (next: string[]) => void
  placeholder?: string
}) {
  const [draft, setDraft] = useState<string | null>(null)
  const commit = () => {
    if (draft === null) return
    onCommit(
      draft
        .split(/[,，;；]+/)
        .map((s) => s.trim())
        .filter(Boolean)
    )
    setDraft(null)
  }
  return (
    <input
      className="input"
      placeholder={placeholder}
      value={draft ?? value.join(', ')}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === 'Enter') e.currentTarget.blur()
      }}
    />
  )
}

/** `2026-11-01T10:00:00Z` → `2026-11-01`。取不到就返回空串，别显示 Invalid Date */
function fmtDate(iso: string): string {
  if (!iso) return ''
  const d = new Date(iso)
  return Number.isNaN(d.getTime()) ? '' : d.toISOString().slice(0, 10)
}

const HOTKEY_FIELDS: Array<{ key: string; label: string }> = [
  // 截图只有一个键：全屏 / 窗口 / 滚动都在选区里选（Ctrl+A / 单击 / S）
  { key: 'regionCapture', label: '截图' },
  { key: 'clipboardPanel', label: '剪贴板面板' },
  { key: 'pinFromClipboard', label: '剪贴板贴图' },
  { key: 'toggleRecording', label: '录屏（视频）' },
  { key: 'toggleRecordingGif', label: '录屏（GIF）' }
]

export default function SettingsPage({
  settings,
  onChange
}: {
  settings: AppSettings | null
  onChange: (patch: unknown) => Promise<AppSettings | null>
}) {
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [engines, setEngines] = useState<OcrEngineInfo[]>([])
  const [sysHistory, setSysHistory] = useState<boolean | null>(null)
  /** 注册表里的真实状态：和开关值不一致就说明写失败了 */
  const [autostart, setAutostart] = useState<boolean | null>(null)
  const [downloading, setDownloading] = useState(false)
  /** 设置项太多，按主题分 tab：通用 / 快捷键 / 输出与识别 */
  const [tab, setTab] = useState<'general' | 'hotkeys' | 'output'>('general')
  const [dl, setDl] = useState<OcrDownloadProgress | null>(null)
  /** 翻译"试一下"的状态：结果直接显示在按钮旁边 */
  const [transTest, setTransTest] = useState<{ busy: boolean; ok: boolean; text: string }>({
    busy: false,
    ok: false,
    text: ''
  })
  const toast = useToast()
  /** 主密码保险箱状态 */
  const [vault, setVault] = useState<VaultStatus | null>(null)
  const [pwDialog, setPwDialog] = useState<null | 'enable' | 'unlock' | 'change' | 'disable'>(null)
  /** "忘记密码"的二次确认：清空保险箱是不可逆的，必须点两次 */
  const [resetArmed, setResetArmed] = useState(false)
  /** 版本检查结果：启动时后端已经查过一次，这里先读缓存 */
  const [upd, setUpd] = useState<UpdateInfo | null>(null)
  const [checking, setChecking] = useState(false)

  // 启动时那次自动检查的结果，读一次就够（不联网）
  useEffect(() => {
    let alive = true
    void api
      .updateCached()
      .then((r) => alive && setUpd(r))
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [])

  const checkUpdate = async () => {
    setChecking(true)
    try {
      const r = await api.updateCheck()
      setUpd(r)
      toast.push(
        r.hasUpdate ? `发现新版本 ${r.latest}（当前 ${r.current}）` : `已是最新版本 ${r.current}`,
        r.hasUpdate ? 'success' : 'info'
      )
    } catch (e) {
      toast.push(`检查更新失败：${e}`, 'error')
    } finally {
      setChecking(false)
    }
  }

  // 下载进度由后端推过来，stage/百分比都在里面
  useEffect(() => {
    let alive = true
    const unsub = events.onOcrDownload((p) => {
      if (alive) setDl(p)
    })
    return () => {
      alive = false
      void unsub.then((u) => u())
    }
  }, [])

  const downloadPaddle = async () => {
    setDownloading(true)
    setDl({ stage: 'resolving', downloaded: 0, total: 0, message: '准备下载…' })
    try {
      await api.ocrDownloadPaddle()
      await loadEngines()
      toast.push('PaddleOCR 安装完成', 'success')
    } catch (e) {
      toast.push(`下载失败：${e}`, 'error')
      setDl(null)
    } finally {
      setDownloading(false)
    }
  }

  const loadEngines = useCallback(async () => {
    try {
      setEngines(await api.ocrEngines())
    } catch {
      /* 引擎探测失败不影响其它设置项 */
    }
  }, [])

  useEffect(() => {
    void api.appInfo().then(setInfo)
    void loadEngines()
    void api.clipboardSystemHistoryGet().then(setSysHistory)
    void api.autostartStatus().then(setAutostart)
    void api.vaultStatus().then(setVault)
  }, [settings, loadEngines])

  /* ---------------- 主密码 ---------------- */

  /**
   * 保险箱状态变了之后要做的两件事：
   * 同步本页状态 + 让 App 重新拉一次配置（解锁/锁定都会改变内存里的凭据）。
   * 空 patch 就是"纯读"：后端合并个空对象，把最新配置返回回来。
   */
  const afterVaultChange = async (st: VaultStatus) => {
    setVault(st)
    await onChange({})
  }

  const vaultLock = async () => {
    try {
      await afterVaultChange(await api.vaultLock())
      toast.push('已锁定，凭据已从内存清除', 'success')
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  if (!settings) return <div className="empty">载入中…</div>

  // 按配置字段名匹配。以前按 action 文案匹配，"长截图" vs "滚动长截图" 对不上，
  // 那一行就永远显示"未生效"——文案一改就失联，这种设计本身是错的。
  const hotkeyState = (id: string) => info?.hotkeyResults.find((h) => h.id === id)

  return (
    <>
      <div className="page-head">
        <h1>设置</h1>
        <div className="page-tabs">
          <button
            className={tab === 'general' ? 'active' : ''}
            onClick={() => setTab('general')}
          >
            通用
          </button>
          <button
            className={tab === 'hotkeys' ? 'active' : ''}
            onClick={() => setTab('hotkeys')}
          >
            快捷键
          </button>
          <button className={tab === 'output' ? 'active' : ''} onClick={() => setTab('output')}>
            输出与识别
          </button>
        </div>
        <div className="spacer" />
        <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
          v{info?.version ?? '0.1.0'} · 数据目录 {info?.dataDir}
        </span>
      </div>

      <div className="page-body">
        <div className="settings-wrap">
          {tab === 'general' && (
            <>
          {/* 通用 */}
          <div className="setting-card">
            <header>通用</header>
            <Row title="关闭时收进托盘" desc="截图工具的常规行为，托盘里随时可用">
              <button
                className={`switch ${settings.general.closeToTray ? 'on' : ''}`}
                onClick={() =>
                  void onChange({ general: { closeToTray: !settings.general.closeToTray } })
                }
              />
            </Row>
            <Row title="开机自启" desc="登录 Windows 后自动运行（写当前用户注册表，不需要管理员）">
              <span
                className={`hotkey-chip ${
                  autostart === false && settings.general.launchAtLogin ? 'fail' : ''
                }`}
                title={
                  autostart === false && settings.general.launchAtLogin
                    ? '配置里是开的，但注册表里没有这一项（可能被安全软件清掉了）'
                    : '注册表状态'
                }
              >
                {autostart === null ? '…' : autostart ? '已登记' : '未登记'}
              </span>
              <button
                className={`switch ${settings.general.launchAtLogin ? 'on' : ''}`}
                onClick={async () => {
                  await onChange({ general: { launchAtLogin: !settings.general.launchAtLogin } })
                  // 写完回读一次：让界面显示的是系统里的事实，不是我们的乐观假设
                  try {
                    setAutostart(await api.autostartStatus())
                  } catch {
                    setAutostart(null)
                  }
                }}
              />
            </Row>
            <Row title="启用插件" desc="允许加载插件目录中的扩展">
              <button
                className={`switch ${settings.general.enablePlugins ? 'on' : ''}`}
                onClick={() =>
                  void onChange({ general: { enablePlugins: !settings.general.enablePlugins } })
                }
              />
            </Row>
          </div>

          {/* 安全：主密码 */}
          <div className="setting-card">
            <header>安全 · 主密码</header>
            <Row
              title="凭证加密"
              desc={
                vault?.enabled
                  ? vault.unlocked
                    ? '磁盘上是密文，当前已解锁，凭据可用'
                    : '磁盘上是密文。解锁后才能使用收藏的密码和图床凭据'
                  : '当前是明文存储：收藏的密码、图床 token、翻译 key 都能被直接读走'
              }
            >
              <span
                className={`hotkey-chip ${vault?.enabled && !vault.unlocked ? 'fail' : ''}`}
                title="Argon2id 派生 + AES-256-GCM 加密"
              >
                {!vault ? '…' : !vault.enabled ? '明文' : vault.unlocked ? '已解锁' : '已锁定'}
              </span>
            </Row>
            <Row
              title={vault?.enabled ? '主密码' : '设置主密码'}
              desc={
                vault?.enabled
                  ? '改密码会用新盐重新加密；锁定后凭据立即从内存清除'
                  : '设好后敏感数据在磁盘上只有密文。注意：这防的是"翻你文件的人"，防不住已在你机器上以你的身份运行的恶意程序'
              }
            >
              {!vault?.enabled && (
                <button className="btn sm primary" onClick={() => setPwDialog('enable')}>
                  设置主密码
                </button>
              )}
              {vault?.enabled && !vault.unlocked && (
                <>
                  <button className="btn sm primary" onClick={() => setPwDialog('unlock')}>
                    解锁
                  </button>
                  <button className="btn sm ghost" onClick={() => setResetArmed(true)}>
                    忘记密码
                  </button>
                </>
              )}
              {vault?.enabled && vault.unlocked && (
                <>
                  <button className="btn sm" onClick={() => setPwDialog('change')}>
                    修改
                  </button>
                  <button className="btn sm" onClick={() => void vaultLock()}>
                    立即锁定
                  </button>
                  <button className="btn sm danger" onClick={() => setPwDialog('disable')}>
                    关闭
                  </button>
                </>
              )}
            </Row>
            {resetArmed && (
              <div
                className="vault-banner"
                style={{
                  flexDirection: 'column',
                  alignItems: 'flex-start',
                  color: 'var(--danger)',
                  background: 'var(--danger-soft)',
                  borderColor: '#f3c9c9'
                }}
              >
                <span>
                  清空保险箱会永久删除收藏的密码、图床 token 和翻译 key，无法恢复。
                  确认要清空吗？（清空后可重新设置新的主密码）
                </span>
                <div style={{ display: 'flex', gap: 8 }}>
                  <button className="btn sm" onClick={() => setResetArmed(false)}>
                    取消
                  </button>
                  <button
                    className="btn sm danger"
                    onClick={async () => {
                      try {
                        await afterVaultChange(await api.vaultReset())
                        setResetArmed(false)
                        toast.push('保险箱已清空', 'success')
                      } catch (e) {
                        toast.push(String(e), 'error')
                      }
                    }}
                  >
                    确认清空
                  </button>
                </div>
              </div>
            )}
          </div>


          {/* 剪贴板 */}
          <div className="setting-card">
            <header>剪贴板</header>
            <Row title="启用历史记录" desc="后台监听剪贴板变化">
              <button
                className={`switch ${settings.clipboard.enabled ? 'on' : ''}`}
                onClick={() =>
                  void onChange({ clipboard: { enabled: !settings.clipboard.enabled } })
                }
              />
            </Row>
            <Row title="记录图片" desc="图片会存到本地磁盘，可能占用较多空间">
              <button
                className={`switch ${settings.clipboard.captureImages ? 'on' : ''}`}
                onClick={() =>
                  void onChange({
                    clipboard: { captureImages: !settings.clipboard.captureImages }
                  })
                }
              />
            </Row>
            <Row
              title="不记录这些程序"
              desc="逗号分隔，如 1password, keepass；带不带 .exe 都认。密码管理器最该填这里"
            >
              <ListInput
                value={settings.clipboard.ignoreApps}
                placeholder="1password, keepass"
                onCommit={(ignoreApps) => void onChange({ clipboard: { ignoreApps } })}
              />
            </Row>
            <Row title="过滤敏感内容" desc="默认关闭。开了会跳过形似密钥的内容（可能误伤正常文本）">
              <button
                className={`switch ${settings.clipboard.skipSensitive ? 'on' : ''}`}
                onClick={() =>
                  void onChange({
                    clipboard: { skipSensitive: !settings.clipboard.skipSensitive }
                  })
                }
              />
            </Row>
            <Row title="最多保留" desc="超出后自动淘汰最旧的（置顶不淘汰）">
              <input
                className="input"
                type="number"
                value={settings.clipboard.maxItems}
                onChange={(e) =>
                  void onChange({ clipboard: { maxItems: Number(e.target.value) || 500 } })
                }
              />
            </Row>
            <Row title="保留天数" desc="0 表示永久保留">
              <input
                className="input"
                type="number"
                value={settings.clipboard.retentionDays}
                onChange={(e) =>
                  void onChange({ clipboard: { retentionDays: Number(e.target.value) || 0 } })
                }
              />
            </Row>
            <Row
              title="接管 Win+V"
              desc="按 Win+V 弹出本工具的剪贴板面板，替代系统那个"
            >
              <button
                className={`switch ${settings.clipboard.takeoverWinV ? 'on' : ''}`}
                onClick={async () => {
                  const next = !settings.clipboard.takeoverWinV
                  try {
                    await api.clipboardSetTakeoverWinV(next)
                    await onChange({ clipboard: { takeoverWinV: next } })
                    toast.push(next ? '已接管 Win+V' : '已交还 Win+V', 'success')
                  } catch (e) {
                    toast.push(String(e), 'error')
                  }
                }}
              />
            </Row>
            <Row
              title="关闭系统剪贴板历史"
              desc="避免系统再存一份（需要重启 explorer，任务栏会闪一下）"
            >
              <button
                className={`switch ${sysHistory === false ? 'on' : ''}`}
                onClick={async () => {
                  const disableSystem = !(sysHistory === false)
                  try {
                    const applied = await api.clipboardSystemHistorySet(!disableSystem)
                    setSysHistory(applied)
                    toast.push(
                      disableSystem ? '已关闭系统剪贴板历史' : '已恢复系统剪贴板历史',
                      'success'
                    )
                  } catch (e) {
                    toast.push(String(e), 'error')
                  }
                }}
              />
            </Row>
            <div
              style={{
                padding: '0 16px 14px',
                fontSize: 11.5,
                color: 'var(--text-3)',
                lineHeight: 1.8
              }}
            >
              Win+V 是系统保留热键，<code>RegisterHotKey</code> 注册必然失败、改注册表也不释放。
              所以「接管」用的是低级键盘钩子：在按键到达系统之前截住并吞掉，然后弹自己的面板。
              立即生效，不需要重启任何东西。
            </div>
          </div>


          {/* 编辑器 */}
          <div className="setting-card">
            <header>编辑器</header>
            <Row title="标注颜色" desc="新建标注的默认颜色">
              <input
                type="color"
                value={settings.editor.strokeColor}
                onChange={(e) => void onChange({ editor: { strokeColor: e.target.value } })}
                style={{ width: 40, height: 30, border: 'none', background: 'none' }}
              />
            </Row>
            <Row title="贴图不透明度">
              <input
                className="input"
                type="number"
                step={0.1}
                min={0.2}
                max={1}
                value={settings.editor.pinOpacity}
                onChange={(e) =>
                  void onChange({ editor: { pinOpacity: Number(e.target.value) || 1 } })
                }
              />
            </Row>
            <Row title="水印文字" desc="留空则不加水印">
              <input
                className="input"
                value={settings.editor.watermarkText}
                onChange={(e) => void onChange({ editor: { watermarkText: e.target.value } })}
              />
            </Row>
          </div>


          {/* 关于 */}
          <div className="setting-card">
            <header>关于</header>
            <Row
              title={`SnipForge ${info?.version ?? '—'}`}
              desc={
                upd?.hasUpdate
                  ? `有新版本 ${upd.latest}（${upd.prerelease ? '预览版' : '正式版'}）· ${fmtDate(upd.publishedAt)}`
                  : '当前版本。检查更新只提示，不会自动下载安装'
              }
            >
              <div style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
                {upd?.hasUpdate && <span className="hotkey-chip fail">新版 {upd.latest}</span>}
                <button className="btn sm" disabled={checking} onClick={() => void checkUpdate()}>
                  {checking ? '检查中…' : '检查更新'}
                </button>
                {upd?.url && (
                  <button
                    className="btn sm"
                    title="在浏览器里打开发布页，自己下载安装包"
                    onClick={() => {
                      void api.updateOpen(upd.url).catch((e) => toast.push(String(e), 'error'))
                    }}
                  >
                    打开发布页
                  </button>
                )}
              </div>
            </Row>
            <Row title="自动检查新版本" desc="启动后问一次 GitHub，只提示不安装；关掉就不再联网">
              <button
                className={`switch ${settings.general.checkUpdates ? 'on' : ''}`}
                onClick={() =>
                  void onChange({ general: { checkUpdates: !settings.general.checkUpdates } })
                }
              />
            </Row>
            {upd?.hasUpdate && upd.notes && (
              <div
                style={{
                  margin: '2px 14px 14px',
                  padding: 10,
                  maxHeight: 180,
                  overflow: 'auto',
                  fontSize: 11.5,
                  lineHeight: 1.75,
                  color: 'var(--text-2)',
                  background: 'var(--surface-2)',
                  border: '1px solid var(--border)',
                  borderRadius: 'var(--radius-sm)',
                  whiteSpace: 'pre-wrap',
                  wordBreak: 'break-word'
                }}
              >
                {upd.notes}
              </div>
            )}
          </div>

          <div style={{ display: 'flex', gap: 8, paddingBottom: 20 }}>
            <button
              className="btn danger"
              onClick={async () => {
                try {
                  await api.settingsReset()
                  toast.push('已恢复默认设置（图床凭据保留）', 'success')
                  void api.settingsGet().then((r) => onChange(r.settings))
                } catch (e) {
                  toast.push(String(e), 'error')
                }
              }}
            >
              恢复默认设置
            </button>
            <button className="btn" onClick={() => void api.openDir(info?.dataDir ?? '')}>
              打开数据目录
            </button>
          </div>
            </>
          )}

          {tab === 'hotkeys' && (
            <>
          {/* 快捷键 */}
          <div className="setting-card">
            <header>快捷键</header>
            {HOTKEY_FIELDS.map((f) => {
              const st = hotkeyState(f.key)
              return (
                <Row
                  key={f.key}
                  title={f.label}
                  desc={st && !st.ok ? st.reason : undefined}
                >
                  <HotkeyInput
                    value={settings.hotkeys[f.key] ?? ''}
                    ok={st?.ok}
                    onChange={(v) => void onChange({ hotkeys: { [f.key]: v } })}
                  />
                  <span className={`hotkey-chip ${st && !st.ok ? 'fail' : ''}`}>
                    {st?.ok ? '已生效' : st?.reason === '未设置' ? '已停用' : '未生效'}
                  </span>
                </Row>
              )
            })}
            <div style={{ padding: '4px 16px 14px', fontSize: 11.5, color: 'var(--text-3)', lineHeight: 1.7 }}>
              点一下方框，直接按下想用的组合键即可，改完立即生效。
              <br />
              <kbd>Esc</kbd> 取消，<kbd>Delete</kbd> 清空（等于不用快捷键）。
              显示"未生效"通常是被别的软件占用了，换一个组合即可。
            </div>
          </div>




            </>
          )}

          {tab === 'output' && (
            <>
          {/* 截图 */}
          <div className="setting-card">
            <header>截图</header>
            <Row
              title="取完之后"
              desc="选「直接复制」就不再打开编辑器：截图 → 粘贴一步到位。区域 / 窗口 / 滚动都按这个走"
            >
              <select
                className="input"
                value={settings.capture.after}
                onChange={(e) => void onChange({ capture: { after: e.target.value } })}
              >
                <option value="editor">打开编辑器（可标注/打码/取字）</option>
                <option value="copy">直接复制到剪贴板</option>
                <option value="save">直接保存到文件</option>
                <option value="pin">钉在屏幕上</option>
              </select>
            </Row>
          </div>

          {/* 保存 */}
          <div className="setting-card">
            <header>保存</header>
            <Row title="保存目录" desc={settings.save.dir || '默认：图片\\SnipForge'}>
              <input
                className="input"
                value={settings.save.dir}
                placeholder="使用默认目录"
                onChange={(e) => void onChange({ save: { dir: e.target.value } })}
              />
              <button
                className="btn sm"
                onClick={async () => {
                  const picked = await api.pickDir(settings.save.dir)
                  if (picked) await onChange({ save: { dir: picked } })
                }}
              >
                选择
              </button>
            </Row>
            <Row title="文件名模板" desc="{yyyy} {MM} {dd} {HH} {mm} {ss} {rand}">
              <input
                className="input"
                value={settings.save.nameTemplate}
                onChange={(e) => void onChange({ save: { nameTemplate: e.target.value } })}
              />
            </Row>
            <Row title="保存后" desc="保存完成后的动作">
              <select
                className="input"
                value={settings.save.afterSave}
                onChange={(e) => void onChange({ save: { afterSave: e.target.value } })}
              >
                <option value="none">不处理</option>
                <option value="copy">复制到剪贴板</option>
                <option value="open-folder">打开所在文件夹</option>
              </select>
            </Row>
          </div>


          {/* OCR */}
          <div className="setting-card">
            <header>OCR 引擎</header>
            {engines.map((e) => {
              const active = settings.ocr?.engine === e.id
              return (
                <div className="setting-row" key={e.id}>
                  <div className="label">
                    <strong>
                      {e.name}
                      {e.builtin && (
                        <span className="tag" style={{ marginLeft: 6, height: 17, fontSize: 10 }}>
                          内置
                        </span>
                      )}
                    </strong>
                    <span style={{ wordBreak: 'break-all' }}>{e.detail}</span>
                  </div>
                  <div className="control">
                    {!e.available && e.id === 'paddle' && (
                      <button
                        className="btn sm primary"
                        disabled={downloading}
                        onClick={() => void downloadPaddle()}
                      >
                        {downloading ? '下载中…' : '下载'}
                      </button>
                    )}
                    {!e.available && e.id !== 'paddle' && (
                      <span className="tag danger" style={{ height: 20 }}>
                        不可用
                      </span>
                    )}
                    <button
                      className={`switch ${active ? 'on' : ''}`}
                      disabled={!e.available}
                      style={{ opacity: e.available ? 1 : 0.4 }}
                      onClick={() => void onChange({ ocr: { engine: e.id } })}
                    />
                  </div>
                </div>
              )
            })}

            <Row
              title="下载源"
              desc="国内直连 GitHub 常常几十 KB/s。自动模式会依次试内置镜像，全失败才回落直连"
            >
              <select
                className="input"
                value={settings.ocr.downloadMode || 'auto'}
                onChange={(e) => void onChange({ ocr: { downloadMode: e.target.value } })}
              >
                <option value="auto">自动（镜像优先）</option>
                <option value="direct">仅 GitHub 直连</option>
                <option value="custom">自定义镜像优先</option>
              </select>
            </Row>
            {settings.ocr.downloadMode === 'custom' && (
              <Row
                title="自定义镜像"
                desc="填前缀即可，如 https://ghfast.top/ —— 会自动拼上原始 GitHub 地址"
              >
                <input
                  className="input"
                  // 用 key 强制跟随外部值重建：失焦才提交，避免边打字边落盘
                  key={settings.ocr.mirrorPrefix}
                  defaultValue={settings.ocr.mirrorPrefix}
                  placeholder="https://ghfast.top/"
                  onBlur={(e) => {
                    const v = e.target.value.trim()
                    if (v !== settings.ocr.mirrorPrefix) {
                      void onChange({ ocr: { mirrorPrefix: v } })
                    }
                  }}
                  onKeyDown={(e) => e.key === 'Enter' && e.currentTarget.blur()}
                />
              </Row>
            )}
            <Row
              title="离线安装"
              desc="已经把 Windows 版 .7z 下好了？直接选文件，不联网、不校验网络"
            >
              <button
                className="btn sm"
                disabled={downloading}
                onClick={async () => {
                  try {
                    const picked = await api.pickFile('选择 PaddleOCR 的 7z 压缩包', ['7z'])
                    if (!picked) return
                    setDownloading(true)
                    setDl({ stage: 'extracting', downloaded: 0, total: 0, message: '准备解压…' })
                    await api.ocrInstallLocal(picked)
                    await loadEngines()
                    toast.push('已安装 PaddleOCR 引擎', 'success')
                  } catch (e) {
                    toast.push(`安装失败：${e}`, 'error')
                  } finally {
                    setDownloading(false)
                    setDl(null)
                  }
                }}
              >
                选择压缩包…
              </button>
            </Row>

            <Row title="引擎目录" desc="把 PaddleOCR-json 解压到这里即可自动识别">
              <button
                className="btn sm"
                onClick={async () => {
                  try {
                    const dir = await api.ocrOpenEngineDir()
                    toast.push(`已打开：${dir}`, 'success')
                  } catch (err) {
                    toast.push(String(err), 'error')
                  }
                }}
              >
                打开目录
              </button>
              <button
                className="btn sm"
                onClick={async () => {
                  await loadEngines()
                  toast.push('已重新检测', 'success')
                }}
              >
                刷新
              </button>
            </Row>

            {dl && (
              <div style={{ padding: '0 16px 14px' }}>
                <div className="progress-track">
                  <div
                    className="progress-fill"
                    style={{
                      width:
                        dl.stage === 'extracting' || dl.stage === 'done'
                          ? '100%'
                          : dl.total > 0
                            ? `${Math.round((dl.downloaded / dl.total) * 100)}%`
                            : '6%'
                    }}
                  />
                </div>
                <div style={{ fontSize: 11.5, color: 'var(--text-3)', marginTop: 6 }}>
                  {dl.message}
                  {dl.stage === 'downloading' && dl.total > 0 && (
                    <span style={{ marginLeft: 6 }}>
                      {Math.round((dl.downloaded / dl.total) * 100)}%
                    </span>
                  )}
                </div>
              </div>
            )}

            {engines.some((e) => e.id === 'paddle' && !e.available) && !downloading && (
              <div
                style={{
                  padding: '0 16px 14px',
                  fontSize: 11.5,
                  color: 'var(--text-3)',
                  lineHeight: 1.8
                }}
              >
                PaddleOCR 中文识别明显优于系统自带。点「下载」会自动获取官方最新版
                （约 88MB，解压后约 150MB）并设为当前引擎，无需手动处理 7z 压缩包。
                下载源是 GitHub，网络不通时会失败。
              </div>
            )}

            {!engines.find((e) => e.builtin)?.available && (
              <div style={{ padding: '0 16px 14px', fontSize: 11.5, color: 'var(--warn)' }}>
                Windows OCR 语言包缺失。前往「设置 → 时间和语言 → 语言和区域」添加中文/英文，
                并在语言选项中勾选「光学字符识别」。
              </div>
            )}
          </div>

          {/* 翻译 */}
          <div className="setting-card">
            <header>翻译服务</header>
            {(() => {
              const t = settings?.translate
              const provider = t?.provider ?? 'tencent'
              const hint = PROVIDERS.find((p) => p.id === provider)?.hint ?? ''
              const patchTr = (v: Record<string, string>) => void onChange({ translate: v })
              return (
                <>
                  <Row title="服务" desc="取字窗口里的翻译走这里；换完立刻生效，不用重启">
                    <select
                      className="input"
                      value={provider}
                      onChange={(e) => patchTr({ provider: e.target.value })}
                    >
                      {PROVIDERS.map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.label}
                        </option>
                      ))}
                    </select>
                  </Row>
                  <div style={{ padding: '0 16px 10px', fontSize: 11.5, color: 'var(--text-3)' }}>
                    {hint}
                  </div>

                  {provider === 'deepl' && (
                    <Row title="DeepL Key" desc="以 :fx 结尾的是免费版，会自动走 free 域名">
                      <input
                        className="input"
                        style={{ width: 320 }}
                        type="password"
                        spellCheck={false}
                        placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx:fx"
                        value={t?.deeplKey ?? ''}
                        onChange={(e) => patchTr({ deeplKey: e.target.value })}
                      />
                    </Row>
                  )}

                  {provider === 'custom' && (
                    <>
                      <Row title="URL" desc="可用 {text} / {to} 占位，需自己编码好查询参数">
                        <input
                          className="input"
                          style={{ width: 380 }}
                          spellCheck={false}
                          placeholder="https://api.example.com/translate?q={text}&to={to}"
                          value={t?.customUrl ?? ''}
                          onChange={(e) => patchTr({ customUrl: e.target.value })}
                        />
                      </Row>
                      <Row title="方法">
                        <select
                          className="input"
                          value={t?.customMethod ?? 'POST'}
                          onChange={(e) => patchTr({ customMethod: e.target.value })}
                        >
                          <option value="POST">POST</option>
                          <option value="GET">GET</option>
                        </select>
                      </Row>
                      <Row title="请求头" desc="每行一个 Header: value">
                        <textarea
                          className="input"
                          style={{ width: 380, height: 58, paddingTop: 6, fontSize: 11.5 }}
                          spellCheck={false}
                          value={t?.customHeaders ?? ''}
                          onChange={(e) => patchTr({ customHeaders: e.target.value })}
                        />
                      </Row>
                      <Row title="请求体" desc="POST 用；{text} / {to} 会被替换">
                        <textarea
                          className="input"
                          style={{ width: 380, height: 58, paddingTop: 6, fontSize: 11.5 }}
                          spellCheck={false}
                          value={t?.customBody ?? ''}
                          onChange={(e) => patchTr({ customBody: e.target.value })}
                        />
                      </Row>
                      <Row title="结果路径" desc="如 data.translations.0.text；留空则整个响应当译文">
                        <input
                          className="input"
                          style={{ width: 280 }}
                          spellCheck={false}
                          placeholder="data.translated"
                          value={t?.customResultPath ?? ''}
                          onChange={(e) => patchTr({ customResultPath: e.target.value })}
                        />
                      </Row>
                    </>
                  )}

                  <Row title="默认目标语言" desc="取字窗口顶栏里也能随时改，改了会记住">
                    <select
                      className="input"
                      value={t?.target ?? 'zh-CN'}
                      onChange={(e) => patchTr({ target: e.target.value })}
                    >
                      {LANGUAGES.map((l) => (
                        <option key={l.id} value={l.id}>
                          {l.label}
                        </option>
                      ))}
                    </select>
                  </Row>

                  <Row title="试一下" desc="用当前配置翻一句示例，确认网络/key 是否正常">
                    <button
                      className="btn sm"
                      disabled={transTest.busy}
                      onClick={() => {
                        setTransTest({ busy: true, ok: false, text: '' })
                        void api
                          .translateTest(t?.target)
                          .then((r) => setTransTest({ busy: false, ok: true, text: r }))
                          .catch((e) =>
                            setTransTest({ busy: false, ok: false, text: String(e) })
                          )
                      }}
                    >
                      {transTest.busy ? '翻译中…' : '测试'}
                    </button>
                    {transTest.text && (
                      <span
                        style={{
                          marginLeft: 8,
                          fontSize: 11.5,
                          color: transTest.ok ? 'var(--text-2)' : 'var(--danger)',
                          wordBreak: 'break-all'
                        }}
                      >
                        {transTest.text}
                      </span>
                    )}
                  </Row>
                </>
              )
            })()}
          </div>


            </>
          )}
        </div>
      </div>

      {pwDialog === 'enable' && (
        <PasswordDialog
          title="设置主密码"
          desc="收藏的密码、图床 token、翻译 key 会用它在本地加密。没有找回后门 —— 忘了就再也解不开，请自己存好。"
          submitLabel="加密"
          confirm
          onSubmit={async (pw) => {
            await afterVaultChange(await api.vaultEnable(pw))
            toast.push('已启用加密', 'success')
          }}
          onClose={() => setPwDialog(null)}
        />
      )}

      {pwDialog === 'unlock' && (
        <PasswordDialog
          title="解锁保险箱"
          desc="解锁后凭据进入内存；关闭应用或点「立即锁定」会重新清掉。"
          submitLabel="解锁"
          onSubmit={async (pw) => {
            await afterVaultChange(await api.vaultUnlock(pw))
            toast.push('已解锁', 'success')
          }}
          onClose={() => setPwDialog(null)}
        />
      )}

      {pwDialog === 'change' && (
        <PasswordDialog
          title="修改主密码"
          desc="会用新的随机盐重新派生密钥，并把所有槽重新加密。旧密码立即失效。"
          submitLabel="修改"
          confirm
          onSubmit={async (pw) => {
            setVault(await api.vaultChangePassword(pw))
            toast.push('主密码已更换', 'success')
          }}
          onClose={() => setPwDialog(null)}
        />
      )}

      {pwDialog === 'disable' && (
        <PasswordDialog
          title="关闭主密码"
          desc="输入当前密码确认。关闭后凭据会以明文写回本地文件，任何能读你磁盘的程序都能看到。"
          submitLabel="确认关闭"
          onSubmit={async (pw) => {
            // 先验密码（后端 vault_disable 只要求已解锁，这里补一次真实校验）
            await api.vaultUnlock(pw)
            await afterVaultChange(await api.vaultDisable())
            toast.push('已关闭主密码，现在是明文存储', 'success')
          }}
          onClose={() => setPwDialog(null)}
        />
      )}
    </>
  )
}
