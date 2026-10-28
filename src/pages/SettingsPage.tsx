import { useCallback, useEffect, useState } from 'react'
import { api, events } from '../lib/api'
import type { AppInfo, AppSettings, OcrDownloadProgress, OcrEngineInfo } from '../lib/types'
import { useToast } from '../components/Toast'
import { HotkeyInput } from '../components/HotkeyInput'

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

const HOTKEY_FIELDS: Array<{ key: string; label: string }> = [
  { key: 'regionCapture', label: '区域截图' },
  { key: 'fullscreenCapture', label: '全屏截图' },
  { key: 'activeWindowCapture', label: '窗口截图' },
  { key: 'scrollCapture', label: '滚动长截图' },
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
  onChange: (patch: unknown) => Promise<AppSettings>
}) {
  const [info, setInfo] = useState<AppInfo | null>(null)
  const [engines, setEngines] = useState<OcrEngineInfo[]>([])
  const [sysHistory, setSysHistory] = useState<boolean | null>(null)
  const [downloading, setDownloading] = useState(false)
  const [dl, setDl] = useState<OcrDownloadProgress | null>(null)
  const toast = useToast()

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
  }, [settings, loadEngines])

  if (!settings) return <div className="empty">载入中…</div>

  // 按配置字段名匹配。以前按 action 文案匹配，"长截图" vs "滚动长截图" 对不上，
  // 那一行就永远显示"未生效"——文案一改就失联，这种设计本身是错的。
  const hotkeyState = (id: string) => info?.hotkeyResults.find((h) => h.id === id)

  return (
    <>
      <div className="page-head">
        <h1>设置</h1>
        <div className="spacer" />
        <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
          v{info?.version ?? '0.1.0'} · 数据目录 {info?.dataDir}
        </span>
      </div>

      <div className="page-body">
        <div className="settings-wrap">
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
            <Row title="开机自启" desc="登录 Windows 后自动运行">
              <button
                className={`switch ${settings.general.launchAtLogin ? 'on' : ''}`}
                onClick={() =>
                  void onChange({ general: { launchAtLogin: !settings.general.launchAtLogin } })
                }
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
        </div>
      </div>
    </>
  )
}
