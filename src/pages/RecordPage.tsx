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

  const start = async (region: boolean) => {
    setBusy(true)
    try {
      // 后端会先把主窗口收起来（否则会被录进去），再进选区 / 直接开录
      if (region) await api.recordPickRegion()
      else await api.recordStartFullscreen()
      toast.push(region ? '拖拽选择要录制的区域' : '录制已开始，控制条在屏幕底部', 'success')
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
          快捷键 <kbd>{settings.hotkeys.toggleRecording}</kbd>：没在录制时进区域选择，录制中则停止。
          录制时底部会出现控制条，可以暂停、标注、开放大镜。
        </div>
      </div>

      <div className="setting-card">
        <header>画面</header>
        <Row title="帧率" desc="越高越流畅，文件也越大">
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
          控制条本身被排除在捕获之外，所以它放在屏幕哪个位置都不会出现在视频里。
        </div>
      </div>
    </div>
  )
}
