import { useState } from 'react'
import { api } from '../lib/api'
import type { AppSettings } from '../lib/types'
import { useToast } from '../components/Toast'

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
const BITRATES = [
  { v: 4_000_000, label: '小' },
  { v: 8_000_000, label: '标准' },
  { v: 16_000_000, label: '高清' }
]
const COUNTDOWNS = [0, 3, 5]
const FORMATS = [
  { v: 'mp4', label: 'MP4 视频', desc: '体积小、能带声音，适合发群/存档' },
  { v: 'gif', label: 'GIF 动图', desc: '直接贴进聊天窗口和文档，自动循环' },
  { v: 'both', label: '两个都要', desc: '同时出 MP4 和 GIF，文件名相同' }
]
const GIF_FPS = [8, 10, 12, 15, 20]
const GIF_WIDTHS = [
  { v: 600, label: '600' },
  { v: 900, label: '900' },
  { v: 1280, label: '1280' },
  { v: 0, label: '原尺寸' }
]

export default function RecordPage({
  settings,
  onChange
}: {
  settings: AppSettings
  onChange: (patch: unknown) => Promise<AppSettings>
}) {
  const toast = useToast()
  const [busy, setBusy] = useState(false)
  const rec = settings.record
  const wantGif = rec.format === 'gif' || rec.format === 'both'
  const gifOnly = rec.format === 'gif'

  const start = async (region: boolean) => {
    setBusy(true)
    try {
      // 后端会先把主窗口收起来（否则会被录进去），再进选区 / 直接开录
      if (region) await api.recordPickRegion()
      else await api.recordStartFullscreen()
      const what = rec.format === 'gif' ? 'GIF' : rec.format === 'both' ? 'MP4 + GIF' : 'MP4'
      toast.push(
        `${what} 录制已开始${region ? '，拖拽选择要录制的区域' : ''}，控制条在屏幕底部`,
        'success'
      )
    } catch (e) {
      toast.push(String(e), 'error')
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="settings-wrap">
      <div className="setting-card">
        <header>开始录制</header>
        <div style={{ padding: '14px 16px 18px', display: 'flex', gap: 10 }}>
          <button
            className="btn"
            disabled={busy}
            style={{ flex: 1, height: 62, fontSize: 14 }}
            onClick={() => void start(true)}
          >
            录制区域…
          </button>
          <button
            className="btn"
            disabled={busy}
            style={{ flex: 1, height: 62, fontSize: 14 }}
            onClick={() => void start(false)}
          >
            录制全屏
          </button>
        </div>
        <div style={{ padding: '0 16px 16px', fontSize: 11.5, color: 'var(--text-3)', lineHeight: 1.7 }}>
          当前输出：<b>{FORMATS.find((f) => f.v === rec.format)?.label ?? 'MP4 视频'}</b>
          。快捷键 <kbd>{settings.hotkeys.toggleRecording}</kbd>：没在录制时进区域选择，录制中则停止。
          录制时底部会出现控制条，可以暂停、标注、开放大镜。
        </div>
      </div>

      <div className="setting-card">
        <header>输出格式</header>
        <div style={{ padding: '12px 16px 4px', display: 'flex', gap: 8 }}>
          {FORMATS.map((f) => (
            <button
              key={f.v}
              className={`btn ${rec.format === f.v ? 'primary' : ''}`}
              style={{ flex: 1, height: 54, flexDirection: 'column', gap: 2, lineHeight: 1.3 }}
              onClick={() => void onChange({ record: { format: f.v } })}
            >
              <span style={{ fontSize: 13 }}>{f.label}</span>
            </button>
          ))}
        </div>
        <div style={{ padding: '0 16px 14px', fontSize: 11.5, color: 'var(--text-3)', lineHeight: 1.7 }}>
          {FORMATS.find((f) => f.v === rec.format)?.desc}
          {wantGif && ' · GIF 是录制时直出的，不走二次转码（所以要提前选好）'}
        </div>
        {wantGif && (
          <>
            <Row title="GIF 帧率" desc="GIF 以 1/100 秒计时，超过 15 基本只剩体积在涨">
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
            <Row title="GIF 宽度" desc="贴在聊天窗口里 900 够用；原尺寸很容易几十 MB">
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
          </>
        )}
      </div>

      <div className="setting-card">
        <header>画面</header>
        <Row title="帧率" desc={gifOnly ? '只影响录制采样，GIF 实际帧率看上面的设置' : '越高越流畅，文件也越大'}>
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
        {!gifOnly && (
          <Row title="画质" desc="对应码率，屏幕内容越花越吃码率">
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
        )}
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
      </div>

      <div className="setting-card">
        <header>鼠标与标注</header>
        <Row title="录制鼠标光标" desc="离开系统光标位图合进画面，和屏幕上看到的一致">
          <button
            className={`switch ${rec.showCursor ? 'on' : ''}`}
            onClick={() => void onChange({ record: { showCursor: !rec.showCursor } })}
          />
        </Row>
        <Row title="鼠标高亮环" desc="光标外面一圈紫色描边，录像里更容易找到视线焦点">
          <button
            className={`switch ${rec.cursorHighlight ? 'on' : ''}`}
            onClick={() => void onChange({ record: { cursorHighlight: !rec.cursorHighlight } })}
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
        <header>说明</header>
        <div style={{ padding: '12px 16px 16px', fontSize: 11.5, color: 'var(--text-3)', lineHeight: 1.8 }}>
          编码走 Windows 自带的 H.264 编码器（Media Foundation），不占安装体积，产出标准 MP4，
          Windows 播放器 / 网页 / 剪辑软件都能直接用。
          <br />
          GIF 和 MP4 共用同一次采集，画面内容完全一致；GIF 会按设定宽度缩放、自动合并静止画面，
          所以录桌面操作时体积通常只有"逐帧存图"的几分之一。
          <br />
          控制条本身被排除在捕获之外，所以它放在屏幕哪个位置都不会出现在视频里。
        </div>
      </div>
    </div>
  )
}
