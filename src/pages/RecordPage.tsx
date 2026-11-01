import { useCallback, useEffect, useState } from 'react'
import { api } from '../lib/api'
import type { AppSettings, CameraDevice, RecordFile } from '../lib/types'
import { useToast } from '../components/Toast'
import { IconLoader } from '../components/Icons'

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

const FPS = [15, 24, 30, 60]

/** 画中画宽度占画面的比例 */
const CAM_SIZES = [
  { v: 15, label: '小' },
  { v: 20, label: '中' },
  { v: 28, label: '大' }
]

const CAM_CORNERS = [
  { v: 'tl', label: '左上' },
  { v: 'tr', label: '右上' },
  { v: 'bl', label: '左下' },
  { v: 'br', label: '右下' }
]
const BITRATES = [
  { v: 4_000_000, label: '小' },
  { v: 8_000_000, label: '标准' },
  { v: 16_000_000, label: '高清' }
]
const COUNTDOWNS = [0, 3, 5]
const GIF_FPS = [8, 10, 12, 15, 20]
const GIF_WIDTHS = [
  { v: 600, label: '600' },
  { v: 900, label: '900' },
  { v: 1280, label: '1280' },
  { v: 0, label: '原尺寸' }
]

function kb(n: number): string {
  if (!n) return '—'
  if (n >= 1024 * 1024) return `${(n / 1048576).toFixed(1)} MB`
  return `${(n / 1024).toFixed(0)} KB`
}

function fmtTime(ms: number): string {
  if (!ms) return ''
  const d = new Date(ms)
  const now = new Date()
  const hm = `${String(d.getHours()).padStart(2, '0')}:${String(d.getMinutes()).padStart(2, '0')}`
  if (d.toDateString() === now.toDateString()) return hm
  const md = `${d.getMonth() + 1}-${String(d.getDate()).padStart(2, '0')}`
  return d.getFullYear() === now.getFullYear() ? `${md} ${hm}` : `${d.getFullYear()}-${md}`
}

type Tab = 'record' | 'settings'

/**
 * 录制入口的卡片：格式写在头上，参数只放摘要，点进去改在「设置」。
 *
 * 以前五种参数（帧率/画质/GIF 帧率/GIF 宽度/倒计时）和两个录制入口堆在一屏，
 * 想按一下"开始录"得先越过一堆开关 —— 这里把"开始"和"怎么录"分开。
 */
function StartCard({
  title,
  summary,
  hotkey,
  note,
  busy,
  onStart
}: {
  title: string
  summary: string
  hotkey?: string
  note: string
  busy: boolean
  onStart: (region: boolean) => void
}) {
  return (
    <div className="setting-card">
      <div className="rec-head">
        <div>
          <strong>{title}</strong>
          <span className="rec-sum">{summary}</span>
        </div>
        {hotkey && (
          <span className="rec-hotkey">
            <kbd>{hotkey}</kbd>
          </span>
        )}
      </div>

      <div className="rec-btns">
        <button className="btn" disabled={busy} onClick={() => onStart(true)}>
          录制区域…
        </button>
        <button className="btn" disabled={busy} onClick={() => onStart(false)}>
          录制全屏
        </button>
      </div>

      <div className="rec-note">{note}</div>
    </div>
  )
}

export default function RecordPage({
  settings,
  onChange
}: {
  settings: AppSettings
  onChange: (patch: unknown) => Promise<AppSettings | null>
}) {
  const toast = useToast()
  const [tab, setTab] = useState<Tab>('record')
  const [busy, setBusy] = useState(false)
  const [recent, setRecent] = useState<RecordFile[]>([])
  const [loadingRecent, setLoadingRecent] = useState(false)
  const rec = settings.record
  // null = 还没检测完（界面上显示"正在检测"）
  const [cams, setCams] = useState<CameraDevice[] | null>(null)

  const bitrateLabel = BITRATES.find((b) => b.v === rec.videoBitsPerSecond)?.label ?? '标准'
  const gifWidthLabel = GIF_WIDTHS.find((w) => w.v === rec.gifWidth)?.label ?? '900'

  const loadRecent = useCallback(async () => {
    setLoadingRecent(true)
    try {
      setRecent(await api.recordRecent(12))
    } catch {
      setRecent([])
    } finally {
      setLoadingRecent(false)
    }
  }, [])

  // 切回「录制」tab 时刷新：刚录完回来就能看到新文件
  useEffect(() => {
    if (tab === 'record') void loadRecent()
  }, [tab, loadRecent])

  // 只在用户想看摄像头时才枚举：枚举本身很快，但没必要每次进页面都问一遍系统
  useEffect(() => {
    if (tab !== 'settings' || !rec.camera) return
    let alive = true
    void api
      .recordCameraList()
      .then((list) => alive && setCams(list))
      .catch(() => alive && setCams([]))
    return () => {
      alive = false
    }
  }, [tab, rec.camera])

  /** 唯一的录制入口：格式由按钮决定，不再先去设置里选 */
  const start = async (format: 'mp4' | 'gif', region: boolean) => {
    setBusy(true)
    try {
      // 后端会先把主窗口收起来（否则会被录进去），再进选区 / 直接开录
      if (region) await api.recordPickRegion(format)
      else await api.recordStartFullscreen(format)
      const what = format === 'gif' ? 'GIF' : '视频'
      toast.push(
        `${what}录制已开始${region ? '，拖拽选择要录制的区域' : ''}，控制条在屏幕底部`,
        'success'
      )
    } catch (e) {
      toast.push(String(e), 'error')
    } finally {
      setBusy(false)
    }
  }

  return (
    <>
      <div className="page-head">
        <h1>录屏</h1>
        <div className="page-tabs">
          <button className={tab === 'record' ? 'active' : ''} onClick={() => setTab('record')}>
            录制
          </button>
          <button
            className={tab === 'settings' ? 'active' : ''}
            onClick={() => setTab('settings')}
          >
            设置
          </button>
        </div>
        <div className="spacer" />
        <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>
          视频 <kbd>{settings.hotkeys.toggleRecording}</kbd> · GIF{' '}
          <kbd>{settings.hotkeys.toggleRecordingGif}</kbd>
        </span>
      </div>

      <div className="page-body">
        {tab === 'record' ? (
          <div className="settings-wrap">
            <StartCard
              title="录制视频"
              summary={`${rec.fps} fps · ${bitrateLabel}画质 · ${rec.countdown > 0 ? `${rec.countdown} 秒倒计时` : '立即开始'}`}
              hotkey={settings.hotkeys.toggleRecording}
              note="H.264 / MP4，体积小、兼容性好，剪辑软件能直接吃。区域里鼠标移到窗口上单击即可选中整个窗口。"
              busy={busy}
              onStart={(region) => void start('mp4', region)}
            />

            <StartCard
              title="录制 GIF"
              summary={`${rec.gifFps} fps · 宽 ${gifWidthLabel} · ${rec.countdown > 0 ? `${rec.countdown} 秒倒计时` : '立即开始'}`}
              hotkey={settings.hotkeys.toggleRecordingGif}
              note="直接从原始帧编码，不经过「先录 MP4 再转」；静止画面自动合并，所以录桌面操作体积通常很小。GIF 没有声音。"
              busy={busy}
              onStart={(region) => void start('gif', region)}
            />

            {/* 录完不知道文件在哪是真实痛点，直接列出来 */}
            <div className="setting-card">
              <div className="rec-head" style={{ alignItems: 'center', paddingBottom: 10 }}>
                <div>
                  <strong>最近录制</strong>
                  <span className="rec-sum">
                    {settings.save.dir.trim() || '默认：图片\\SnipForge'}
                  </span>
                </div>
                <button className="btn sm" onClick={() => void loadRecent()} disabled={loadingRecent}>
                  {loadingRecent ? <IconLoader /> : null} 刷新
                </button>
              </div>

              {recent.length === 0 ? (
                <div style={{ padding: '2px 16px 16px', fontSize: 11.5, color: 'var(--text-3)' }}>
                  这个目录里还没有录制文件。
                </div>
              ) : (
                recent.map((f) => (
                  <div className="rec-file" key={f.path}>
                    <span className={`rec-badge ${f.format}`}>
                      {f.format === 'gif' ? 'GIF' : 'MP4'}
                    </span>
                    <div className="rec-info">
                      <div className="rec-name" title={f.path}>
                        {f.name}
                      </div>
                      <div className="rec-meta">
                        {kb(f.bytes)} <span className="dot">·</span> {fmtTime(f.createdAt)}
                      </div>
                    </div>
                    <button
                      className="btn sm"
                      title="用默认程序打开"
                      onClick={() => void api.openPath(f.path)}
                    >
                      播放
                    </button>
                    <button
                      className="btn sm"
                      title="在资源管理器中显示"
                      onClick={() => void api.openImageFolder(f.path)}
                    >
                      定位
                    </button>
                  </div>
                ))
              )}
            </div>
          </div>
        ) : (
          <div className="settings-wrap">
            <div className="setting-card">
              <header>画面</header>
              <Row title="帧率" desc="越高越流畅，文件也越大（视频）">
                <div className="seg">
                  {FPS.map((f) => (
                    <button
                      key={f}
                      className={rec.fps === f ? 'active' : ''}
                      onClick={() => void onChange({ record: { fps: f } })}
                    >
                      {f}
                    </button>
                  ))}
                </div>
              </Row>
              <Row title="画质" desc="对应码率，屏幕内容越花越吃码率（只影响视频）">
                <div className="seg">
                  {BITRATES.map((b) => (
                    <button
                      key={b.v}
                      className={rec.videoBitsPerSecond === b.v ? 'active' : ''}
                      onClick={() => void onChange({ record: { videoBitsPerSecond: b.v } })}
                    >
                      {b.label}
                    </button>
                  ))}
                </div>
              </Row>
            </div>

            <div className="setting-card">
              <header>GIF</header>
              <Row title="帧率" desc="GIF 以 1/100 秒计时，超过 15 基本只剩体积在涨">
                <div className="seg">
                  {GIF_FPS.map((f) => (
                    <button
                      key={f}
                      className={rec.gifFps === f ? 'active' : ''}
                      onClick={() => void onChange({ record: { gifFps: f } })}
                    >
                      {f}
                    </button>
                  ))}
                </div>
              </Row>
              <Row title="宽度" desc="贴在聊天窗口里 900 够用；原尺寸很容易几十 MB">
                <div className="seg">
                  {GIF_WIDTHS.map((w) => (
                    <button
                      key={w.v}
                      className={rec.gifWidth === w.v ? 'active' : ''}
                      onClick={() => void onChange({ record: { gifWidth: w.v } })}
                    >
                      {w.label}
                    </button>
                  ))}
                </div>
              </Row>
            </div>

            <div className="setting-card">
              <header>录制时</header>
              <Row title="倒计时" desc="给操作留出准备时间，这段不会录进去">
                <div className="seg">
                  {COUNTDOWNS.map((c) => (
                    <button
                      key={c}
                      className={rec.countdown === c ? 'active' : ''}
                      onClick={() => void onChange({ record: { countdown: c } })}
                    >
                      {c === 0 ? '关闭' : `${c} 秒`}
                    </button>
                  ))}
                </div>
              </Row>
              <Row
                title="录制系统声音"
                desc="抓默认播放设备的输出（WASAPI loopback），不用装虚拟声卡"
              >
                <button
                  className={`switch ${rec.audio ? 'on' : ''}`}
                  onClick={() => void onChange({ record: { audio: !rec.audio } })}
                />
              </Row>
              <Row
                title="录制麦克风"
                desc="和系统声音自动混音，两路都有声时不会互相吞掉"
              >
                <button
                  className={`switch ${rec.mic ? 'on' : ''}`}
                  onClick={() => void onChange({ record: { mic: !rec.mic } })}
                />
              </Row>
              <Row
                title="摄像头画中画"
                desc={
                  cams === null
                    ? '正在检测摄像头…'
                    : cams.length === 0
                      ? '这台机器没有检测到摄像头'
                      : '把摄像头画面合成到录像的角落'
                }
              >
                <button
                  className={`switch ${rec.camera ? 'on' : ''}`}
                  disabled={cams !== null && cams.length === 0}
                  onClick={() => void onChange({ record: { camera: !rec.camera } })}
                />
              </Row>
              {rec.camera && cams && cams.length > 0 && (
                <>
                  {cams.length > 1 && (
                    <Row title="摄像头设备" desc="多台设备时选一个">
                      <select
                        value={rec.cameraDevice}
                        onChange={(e) =>
                          void onChange({ record: { cameraDevice: e.target.value } })
                        }
                      >
                        <option value="">第一个可用设备</option>
                        {cams.map((c) => (
                          <option key={c.id} value={c.id}>
                            {c.name}
                          </option>
                        ))}
                      </select>
                    </Row>
                  )}
                  <Row title="画中画大小" desc="占画面宽度的比例，换分辨率不用重调">
                    <div className="seg">
                      {CAM_SIZES.map((s) => (
                        <button
                          key={s.v}
                          className={rec.cameraSize === s.v ? 'active' : ''}
                          onClick={() => void onChange({ record: { cameraSize: s.v } })}
                        >
                          {s.label}
                        </button>
                      ))}
                    </div>
                  </Row>
                  <Row title="画中画位置">
                    <div className="seg">
                      {CAM_CORNERS.map((c) => (
                        <button
                          key={c.v}
                          className={rec.cameraCorner === c.v ? 'active' : ''}
                          onClick={() => void onChange({ record: { cameraCorner: c.v } })}
                        >
                          {c.label}
                        </button>
                      ))}
                    </div>
                  </Row>
                  <Row title="水平镜像" desc="像照镜子，自拍视角更自然">
                    <button
                      className={`switch ${rec.cameraMirror ? 'on' : ''}`}
                      onClick={() =>
                        void onChange({ record: { cameraMirror: !rec.cameraMirror } })
                      }
                    />
                  </Row>
                </>
              )}
              <Row title="录制鼠标光标" desc="把系统光标位图合进画面，和屏幕上看到的一致">
                <button
                  className={`switch ${rec.showCursor ? 'on' : ''}`}
                  onClick={() => void onChange({ record: { showCursor: !rec.showCursor } })}
                />
              </Row>
              <Row title="鼠标高亮环" desc="光标外面一圈紫色描边，录像里更容易找到视线焦点">
                <button
                  className={`switch ${rec.cursorHighlight ? 'on' : ''}`}
                  onClick={() =>
                    void onChange({ record: { cursorHighlight: !rec.cursorHighlight } })
                  }
                />
              </Row>
              <Row title="放大镜" desc="跟进鼠标，把周围一小块放大显示（录制时也能开关）">
                <button
                  className={`switch ${rec.magnifier ? 'on' : ''}`}
                  onClick={() => void onChange({ record: { magnifier: !rec.magnifier } })}
                />
              </Row>
              <Row title="允许标注" desc="录制中可以在画面上画箭头、框重点，6 秒后自动淡出">
                <button
                  className={`switch ${rec.annotate ? 'on' : ''}`}
                  onClick={() => void onChange({ record: { annotate: !rec.annotate } })}
                />
              </Row>
            </div>

            <div className="setting-card">
              <header>保存与编码</header>
              <Row title="保存目录" desc={settings.save.dir || '默认：图片\\SnipForge'}>
                <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>在「设置」页可改</span>
              </Row>
              <div
                style={{
                  padding: '10px 16px 16px',
                  fontSize: 11.5,
                  color: 'var(--text-3)',
                  lineHeight: 1.8
                }}
              >
                编码走 Windows 自带的 H.264 编码器（Media Foundation），不占安装体积。
                <br />
                控制条本身被排除在捕获之外，放在屏幕哪个位置都不会出现在画面里。
                <br />
                录完如果开了「保存后复制」，文件已经在剪贴板里，可以直接粘进聊天窗口。
              </div>
            </div>
          </div>
        )}
      </div>
    </>
  )
}
