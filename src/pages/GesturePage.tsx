import { useRef, useState } from 'react'
import { api } from '../lib/api'
import type { AppSettings, GestureAction, GestureBinding, GestureConfig } from '../lib/types'
import { useToast } from '../components/Toast'
import { IconPlus, IconTrash } from '../components/Icons'

/**
 * 鼠标手势（独立功能页）。
 *
 * 定位：这是一个**通用**的手势工具 —— 手势能发任意快捷键、开程序、开网址、
 * 跑命令、操作窗口、按媒体键。SnipForge 自己的截图录屏只是其中一类动作，
 * 和别的平级，不占特殊位置。
 *
 * 手势有两种：方向串（`DR` = 先下后右）和形状（直接画一个 `S`）。
 * 两者都是精确匹配，所以这页最重要的事情是让手势**看得见**：
 * 列表里直接画箭头 / 形状字符，新增时让用户真的划一遍。
 *
 * 形状识别不在这页做：录制框的轨迹要交给后端（`gesture_recognize`）判断，
 * 否则"这里认得出、实际划没反应"就成了新 bug。
 */

const ARROW: Record<string, string> = { U: '↑', D: '↓', L: '←', R: '→' }
/** 形状手势（曲线画的），后端 `shape` 模块认，目前只有 S */
const SHAPE: Record<string, string> = { S: 'S' }

function arrows(pattern: string): string {
  return pattern
    .toUpperCase()
    .split('')
    .map((c) => ARROW[c] ?? SHAPE[c] ?? c)
    .join(' ')
}

/* ---------------- 动作类型表 ---------------- */

type ActionType = GestureAction['type']

const TYPES: Array<{ id: ActionType; label: string; hint: string }> = [
  { id: 'keys', label: '发送按键', hint: '最常用：把快捷键绑到手势上' },
  { id: 'window', label: '窗口操作', hint: '对当前前台窗口做一件事' },
  { id: 'app', label: '打开程序', hint: '程序、文件、文件夹都行' },
  { id: 'url', label: '打开网址', hint: '用默认浏览器打开' },
  { id: 'cmd', label: '运行命令', hint: '直接交给系统执行，谨慎使用' },
  { id: 'media', label: '媒体键', hint: '音量、播放控制' },
  { id: 'text', label: '输入文字', hint: '常用回复、邮箱、命令片段' },
  { id: 'scroll', label: '滚动', hint: '向上/向下滚几格' },
  { id: 'snip', label: 'SnipForge 功能', hint: '截图、录屏、剪贴板面板' }
]

const WINDOW_OPS = [
  { id: 'close', label: '关闭窗口' },
  { id: 'minimize', label: '最小化' },
  { id: 'maximize', label: '最大化' },
  { id: 'restore', label: '还原' },
  { id: 'topmost', label: '置顶 / 取消置顶' }
]

const MEDIA_KEYS = [
  { id: 'vol-up', label: '音量 +' },
  { id: 'vol-down', label: '音量 −' },
  { id: 'mute', label: '静音' },
  { id: 'play', label: '播放 / 暂停' },
  { id: 'next', label: '下一首' },
  { id: 'prev', label: '上一首' }
]

const SNIP_IDS = [
  { id: 'region', label: '区域截图' },
  { id: 'fullscreen', label: '全屏截图' },
  { id: 'window', label: '窗口截图' },
  { id: 'scroll', label: '滚动长截图' },
  { id: 'panel', label: '剪贴板面板' },
  { id: 'pin', label: '剪贴板贴图' },
  { id: 'record', label: '录屏（视频）' },
  { id: 'record-gif', label: '录屏（GIF）' },
  { id: 'pin-region', label: '框选贴图' },
  { id: 'ocr-region', label: '框选取字' }
]

const TRIGGERS = [
  { id: 'right', label: '按住右键' },
  { id: 'middle', label: '按住中键' },
  { id: 'x1', label: '按住侧键' }
]

/** 一条动作在列表里的摘要文字 */
function actionSummary(a: GestureAction): string {
  switch (a.type) {
    case 'keys':
      return a.keys.trim() || '还没填按键'
    case 'window':
      return WINDOW_OPS.find((w) => w.id === a.op)?.label ?? a.op
    case 'app':
      return a.path.trim() || '还没选程序'
    case 'url':
      return a.url.trim() || '还没填网址'
    case 'cmd':
      return a.cmd.trim() || '还没填命令'
    case 'media':
      return MEDIA_KEYS.find((m) => m.id === a.key)?.label ?? a.key
    case 'text':
      return a.text.trim() ? a.text.trim().slice(0, 30) : '还没填文字'
    case 'scroll':
      return `${a.dir === 'down' ? '向下' : '向上'} ${a.amount} 格`
    case 'snip':
      return SNIP_IDS.find((s) => s.id === a.id)?.label ?? a.id
    default:
      return '未知动作'
  }
}

function typeLabel(t: ActionType): string {
  return TYPES.find((x) => x.id === t)?.label ?? t
}

/** 换类型时给一组合理的默认参数，免得用户面对一堆空框 */
function defaultAction(t: ActionType): GestureAction {
  switch (t) {
    case 'keys':
      return { type: 'keys', keys: '' }
    case 'window':
      return { type: 'window', op: 'minimize' }
    case 'app':
      return { type: 'app', path: '' }
    case 'url':
      return { type: 'url', url: 'https://' }
    case 'cmd':
      return { type: 'cmd', cmd: '' }
    case 'media':
      return { type: 'media', key: 'vol-up' }
    case 'text':
      return { type: 'text', text: '' }
    case 'scroll':
      return { type: 'scroll', dir: 'down', amount: 3 }
    case 'snip':
      return { type: 'snip', id: 'region' }
  }
}

/* ---------------- 划手势的小控件 ---------------- */

function Recorder({
  onDone,
  onCancel
}: {
  onDone: (pattern: string) => void
  onCancel: () => void
}) {
  const box = useRef<HTMLDivElement>(null)
  const st = useRef({
    down: false,
    anchor: { x: 0, y: 0 },
    last: '',
    pat: '',
    path: [] as Array<{ x: number; y: number }>
  })
  const [pts, setPts] = useState<Array<{ x: number; y: number }>>([])
  const [preview, setPreview] = useState('')

  const rel = (e: React.PointerEvent) => {
    const r = box.current!.getBoundingClientRect()
    return { x: e.clientX - r.left, y: e.clientY - r.top }
  }

  /**
   * 松手收尾：先问后端"这一笔是不是形状手势"（画 S），不是才退回方向串。
   *
   * 顺序不能反：S 一路划下来方向串会糊成 DLD 之类，先看方向串的话
   * 用户永远录不到自己想要的那个 S。
   */
  const finish = async () => {
    if (!st.current.down) return
    st.current.down = false
    const { path, pat } = st.current
    setPts([])
    setPreview('')
    if (path.length >= 12) {
      try {
        const shape = await api.gestureRecognize(
          path.map((p) => [Math.round(p.x), Math.round(p.y)] as [number, number])
        )
        if (shape) {
          onDone(shape)
          return
        }
      } catch {
        // 后端不可用（比如页面单独跑在浏览器里）：退回方向串，别把功能卡死
      }
    }
    if (pat) onDone(pat)
    else onCancel()
  }

  return (
    <div className="gesture-rec">
      <div
        ref={box}
        className="gesture-rec-box"
        onPointerDown={(e) => {
          e.currentTarget.setPointerCapture(e.pointerId)
          const p = rel(e)
          st.current = { down: true, anchor: p, last: '', pat: '', path: [p] }
          setPts([p])
          setPreview('')
        }}
        onPointerMove={(e) => {
          if (!st.current.down) return
          const p = rel(e)
          setPts((prev) => (prev.length > 220 ? prev : [...prev, p]))
          const s = st.current
          if (s.path.length < 400) s.path.push(p)
          const dx = p.x - s.anchor.x
          const dy = p.y - s.anchor.y
          if (Math.abs(dx) < 34 && Math.abs(dy) < 34) return
          const d = Math.abs(dx) > Math.abs(dy) ? (dx > 0 ? 'R' : 'L') : dy > 0 ? 'D' : 'U'
          if (d !== s.last) {
            s.last = d
            s.pat += d
            setPreview(s.pat)
          }
          s.anchor = p
        }}
        onPointerUp={() => {
          void finish()
        }}
        onPointerCancel={() => {
          st.current.down = false
          setPts([])
          setPreview('')
        }}
      >
        {pts.length > 1 ? (
          <svg className="gesture-rec-svg" width="100%" height="100%">
            <polyline
              points={pts.map((p) => `${p.x},${p.y}`).join(' ')}
              fill="none"
              stroke="var(--accent)"
              strokeWidth="3"
              strokeLinecap="round"
              strokeLinejoin="round"
            />
          </svg>
        ) : (
          <span className="gesture-rec-hint">
            {preview ? arrows(preview) : '在这里按住鼠标划一下：方向串（↑↓←→）或者直接画个 S'}
          </span>
        )}
      </div>
      <div className="gesture-rec-side">
        <span className="gesture-rec-preview">{preview ? arrows(preview) : '—'}</span>
        <button className="btn sm" onClick={onCancel}>
          取消
        </button>
      </div>
    </div>
  )
}

/* ---------------- 动作编辑器 ---------------- */

function ActionEditor({
  action,
  onChange
}: {
  action: GestureAction
  onChange: (a: GestureAction) => void
}) {
  const a = action
  return (
    <div className="ga-editor">
      <select
        className="input"
        style={{ width: 148 }}
        value={a.type}
        onChange={(e) => onChange(defaultAction(e.target.value as ActionType))}
      >
        {TYPES.map((t) => (
          <option key={t.id} value={t.id}>
            {t.label}
          </option>
        ))}
      </select>

      {a.type === 'keys' && (
        <>
          <input
            className="input"
            style={{ flex: 1, minWidth: 160 }}
            placeholder="例如 Ctrl+Shift+T（多组用逗号分隔）"
            value={a.keys}
            spellCheck={false}
            onChange={(e) => onChange({ type: 'keys', keys: e.target.value })}
          />
          <div className="ga-quick">
            {['Alt+Left', 'Alt+Right', 'Ctrl+W', 'Ctrl+T', 'F5', 'Win+D', 'Alt+Tab'].map((k) => (
              <button key={k} className="ga-chip" onClick={() => onChange({ type: 'keys', keys: k })}>
                {k}
              </button>
            ))}
          </div>
        </>
      )}

      {a.type === 'window' && (
        <select
          className="input"
          style={{ width: 160 }}
          value={a.op}
          onChange={(e) => onChange({ type: 'window', op: e.target.value })}
        >
          {WINDOW_OPS.map((w) => (
            <option key={w.id} value={w.id}>
              {w.label}
            </option>
          ))}
        </select>
      )}

      {a.type === 'app' && (
        <>
          <input
            className="input"
            style={{ flex: 1, minWidth: 200 }}
            placeholder="例如 C:\\Windows\\notepad.exe，或某个文件夹"
            value={a.path}
            spellCheck={false}
            onChange={(e) => onChange({ type: 'app', path: e.target.value })}
          />
          <button
            className="btn sm"
            onClick={async () => {
              const p = await api.pickDir(a.path)
              if (p) onChange({ type: 'app', path: p })
            }}
          >
            选文件夹
          </button>
        </>
      )}

      {a.type === 'url' && (
        <input
          className="input"
          style={{ flex: 1, minWidth: 200 }}
          placeholder="https://..."
          value={a.url}
          spellCheck={false}
          onChange={(e) => onChange({ type: 'url', url: e.target.value })}
        />
      )}

      {a.type === 'cmd' && (
        <input
          className="input"
          style={{ flex: 1, minWidth: 200 }}
          placeholder="例如 explorer . 或 shutdown /s /t 0"
          value={a.cmd}
          spellCheck={false}
          onChange={(e) => onChange({ type: 'cmd', cmd: e.target.value })}
        />
      )}

      {a.type === 'media' && (
        <select
          className="input"
          style={{ width: 140 }}
          value={a.key}
          onChange={(e) => onChange({ type: 'media', key: e.target.value })}
        >
          {MEDIA_KEYS.map((m) => (
            <option key={m.id} value={m.id}>
              {m.label}
            </option>
          ))}
        </select>
      )}

      {a.type === 'text' && (
        <textarea
          className="input"
          style={{ flex: 1, minWidth: 200, height: 60, padding: '6px 10px', fontFamily: 'inherit' }}
          placeholder="要输入的原文，换行会按回车发送"
          value={a.text}
          onChange={(e) => onChange({ type: 'text', text: e.target.value })}
        />
      )}

      {a.type === 'scroll' && (
        <>
          <select
            className="input"
            style={{ width: 110 }}
            value={a.dir}
            onChange={(e) => onChange({ type: 'scroll', dir: e.target.value, amount: a.amount })}
          >
            <option value="up">向上</option>
            <option value="down">向下</option>
          </select>
          <input
            className="input"
            style={{ width: 64 }}
            type="number"
            min={1}
            max={20}
            value={a.amount}
            onChange={(e) =>
              onChange({ type: 'scroll', dir: a.dir, amount: Number(e.target.value) || 1 })
            }
          />
          <span style={{ fontSize: 11.5, color: 'var(--text-3)' }}>格</span>
        </>
      )}

      {a.type === 'snip' && (
        <select
          className="input"
          style={{ width: 150 }}
          value={a.id}
          onChange={(e) => onChange({ type: 'snip', id: e.target.value })}
        >
          {SNIP_IDS.map((s) => (
            <option key={s.id} value={s.id}>
              {s.label}
            </option>
          ))}
        </select>
      )}
    </div>
  )
}

/* ---------------- 页面 ---------------- */

export default function GesturePage({
  settings,
  onChange
}: {
  settings: AppSettings | null
  onChange: (patch: unknown) => Promise<AppSettings>
}) {
  const toast = useToast()
  const [recording, setRecording] = useState(false)
  const [testing, setTesting] = useState(false)
  /** 正在编辑的行 */
  const [openIdx, setOpenIdx] = useState<number | null>(null)

  const g: GestureConfig | undefined = settings?.gestures

  const patch = (p: Partial<GestureConfig>) => {
    if (!g) return
    void onChange({ gestures: { ...g, ...p } }).catch((e) => toast.push(String(e), 'error'))
  }

  const setBindings = (next: GestureBinding[]) => patch({ bindings: next })

  const addPattern = (pattern: string) => {
    setRecording(false)
    if (!g) return
    const upper = pattern.toUpperCase()
    if (g.bindings.some((b) => b.pattern.toUpperCase() === upper)) {
      toast.push(`${arrows(upper)} 已经有动作了，改那一行就行`, 'error')
      return
    }
    const next = [...g.bindings, { pattern: upper, action: defaultAction('keys'), enabled: true }]
    setBindings(next)
    setOpenIdx(next.length - 1)
  }

  if (!g) {
    return (
      <div className="empty" style={{ height: 200 }}>
        <span>读取配置中…</span>
      </div>
    )
  }

  return (
    <>
      <div className="page-head">
        <h1>鼠标手势</h1>
        <div className="spacer" />
        <span className={`hotkey-chip ${g.enabled ? '' : 'fail'}`}>
          {g.enabled ? '运行中' : '已暂停'}
        </span>
        <button
          className="btn sm"
          onClick={() => patch({ enabled: !g.enabled })}
          title={g.enabled ? '暂停所有手势（右键恢复正常）' : '重新启用手势'}
        >
          {g.enabled ? '暂停' : '启用'}
        </button>
      </div>

      <div className="page-body">
        <div className="settings-wrap">
          {/* 触发方式 */}
          <div className="setting-card">
            <header>触发方式</header>

            <div className="setting-row">
              <div className="label">
                <strong>触发键</strong>
                <span>按住它划出方向，松开即执行；不移动就是普通点击</span>
              </div>
              <div className="control">
                <select
                  className="input"
                  style={{ width: 130 }}
                  value={g.trigger}
                  onChange={(e) => patch({ trigger: e.target.value })}
                >
                  {TRIGGERS.map((t) => (
                    <option key={t.id} value={t.id}>
                      {t.label}
                    </option>
                  ))}
                </select>
              </div>
            </div>

            <div className="setting-row">
              <div className="label">
                <strong>显示轨迹</strong>
                <span>划的时候在屏幕上画一条线</span>
              </div>
              <div className="control">
                <input
                  type="color"
                  className="ga-color"
                  value={g.trailColor}
                  onChange={(e) => patch({ trailColor: e.target.value })}
                  title="轨迹颜色"
                />
                <button
                  className={`switch ${g.showTrail ? 'on' : ''}`}
                  onClick={() => patch({ showTrail: !g.showTrail })}
                />
              </div>
            </div>

            <div className="setting-row">
              <div className="label">
                <strong>全屏时暂停</strong>
                <span>游戏、视频、演示、远程桌面里不响应 —— 那些场景手势只会帮倒忙</span>
              </div>
              <div className="control">
                <button
                  className={`switch ${g.pauseFullscreen ? 'on' : ''}`}
                  onClick={() => patch({ pauseFullscreen: !g.pauseFullscreen })}
                />
              </div>
            </div>

            <div className="setting-row">
              <div className="label">
                <strong>接管右键按下</strong>
                <span>
                  推荐开启：手势期间目标程序收不到任何右键事件，状态最干净。
                  若你的机器上发现右键点不动，关掉它退回老做法（只吞抬起）
                </span>
              </div>
              <div className="control">
                <button
                  className={`switch ${g.swallowDown ? 'on' : ''}`}
                  onClick={() => patch({ swallowDown: !g.swallowDown })}
                />
              </div>
            </div>

            <div className="setting-row">
              <div className="label">
                <strong>例外程序</strong>
                <span>
                  列表里的程序完全不响应手势（右键菜单照常）。
                  想加一个：在那个程序里按住右键画 Z 形（→←→），弹窗确认即可
                </span>
              </div>
              <div className="control">
                <span className="ga-excl-count">
                  {g.excluded.length === 0 ? '没有' : `${g.excluded.length} 个`}
                </span>
              </div>
            </div>

            {g.excluded.length > 0 && (
              <div className="ga-excl-list">
                {g.excluded.map((exe) => (
                  <div className="ga-excl" key={exe}>
                    <span className="ga-excl-name">{exe}</span>
                    <button
                      className="btn sm danger"
                      title="从例外里移除"
                      onClick={() => patch({ excluded: g.excluded.filter((e) => e !== exe) })}
                    >
                      <IconTrash size={12} />
                    </button>
                  </div>
                ))}
              </div>
            )}

            <div className="setting-row">
              <div className="label">
                <strong>灵敏度</strong>
                <span>按下后移动超过这个距离才算手势</span>
              </div>
              <div className="control">
                <input
                  type="range"
                  className="gesture-range"
                  min={16}
                  max={80}
                  step={2}
                  value={g.threshold}
                  onChange={(e) => patch({ threshold: Number(e.target.value) })}
                />
                <span className="gesture-range-val">{g.threshold}px</span>
              </div>
            </div>

            <div className="ga-note">
              按住触发键不移动、或移动幅度小于灵敏度时，这次点击会原样放行 ——
              右键菜单照常弹。所有显示器都能用，轨迹层覆盖整个虚拟屏。
            </div>
          </div>

          {/* 手势列表 */}
          <div className="setting-card" style={{ marginTop: 14 }}>
            <header style={{ display: 'flex', alignItems: 'center', gap: 8 }}>
              <span>手势</span>
              <span style={{ fontWeight: 400, textTransform: 'none' }}>{g.bindings.length}</span>
              <div style={{ flex: 1 }} />
              {!recording && !testing && (
                <button className="btn sm" onClick={() => setRecording(true)}>
                  <IconPlus size={12} /> 划一个
                </button>
              )}
              <button className="btn sm" onClick={() => setTesting((v) => !v)}>
                {testing ? '收起试划' : '试划'}
              </button>
              <button
                className="btn sm"
                onClick={() =>
                  void onChange({ gestures: { ...g, bindings: DEFAULT_BINDINGS() } })
                    .then(() => toast.push('已恢复默认手势', 'success'))
                    .catch((e) => toast.push(String(e), 'error'))
                }
              >
                恢复默认
              </button>
            </header>

            {recording && <Recorder onDone={addPattern} onCancel={() => setRecording(false)} />}

            {testing && (
              <div style={{ borderTop: '1px solid var(--border)' }}>
                <Recorder
                  onDone={(p) => {
                    const hit = g.bindings.find((b) => b.pattern.toUpperCase() === p.toUpperCase())
                    toast.push(
                      hit ? `${arrows(p)} → ${actionSummary(hit.action)}` : `${arrows(p)} 没有绑定`,
                      hit ? 'success' : 'error'
                    )
                  }}
                  onCancel={() => toast.push('没划出方向', 'error')}
                />
                <div style={{ padding: '0 16px 12px', fontSize: 11, color: 'var(--text-3)' }}>
                  只试识别，不会真的执行动作。
                </div>
              </div>
            )}

            {g.bindings.length === 0 && !recording && (
              <div style={{ padding: '4px 16px 16px', fontSize: 11.5, color: 'var(--text-3)' }}>
                还没有手势。点「划一个」在框里划一下就能新建。
              </div>
            )}

            {g.bindings.map((b, i) => {
              const open = openIdx === i
              return (
                <div className="ga-row-wrap" key={`${b.pattern}-${i}`}>
                  <div className={`ga-row ${open ? 'open' : ''}`} onClick={() => setOpenIdx(open ? null : i)}>
                    <span className="ga-arrow">{arrows(b.pattern)}</span>
                    <span className="ga-type">{typeLabel(b.action.type)}</span>
                    <span className="ga-summary">{actionSummary(b.action)}</span>
                    <div style={{ flex: 1 }} />
                    <button
                      className="btn sm danger"
                      title="删掉这个手势"
                      onClick={(e) => {
                        e.stopPropagation()
                        setBindings(g.bindings.filter((_, j) => j !== i))
                        if (openIdx === i) setOpenIdx(null)
                      }}
                    >
                      <IconTrash size={12} />
                    </button>
                  </div>

                  {open && (
                    <div className="ga-expand">
                      <ActionEditor
                        action={b.action}
                        onChange={(a) => {
                          const next = g.bindings.slice()
                          next[i] = { ...b, action: a }
                          setBindings(next)
                        }}
                      />
                      <div className="ga-expand-note">
                        {TYPES.find((t) => t.id === b.action.type)?.hint}
                      </div>
                    </div>
                  )}
                </div>
              )
            })}
          </div>

          <div className="ga-note" style={{ padding: '10px 4px 24px' }}>
            手势会发送到你按下的那个窗口，所以先在目标窗口里划。改完立刻生效，不用重启。
            <br />
            两种手势：方向串（↑↓←→ 依次组合，一笔折线）和形状（真的画出一个 S，
            曲线）。形状目前只认 S 一个 —— 再多会和方向手势抢，误识别一次就够让人不敢用。
          </div>
        </div>
      </div>
    </>
  )
}

/** 和后端 `default_gesture_bindings` 保持一致 */
function DEFAULT_BINDINGS(): GestureBinding[] {
  const b = (pattern: string, action: GestureAction): GestureBinding => ({
    pattern,
    action,
    enabled: true
  })
  return [
    b('L', { type: 'keys', keys: 'Alt+Left' }),
    b('R', { type: 'keys', keys: 'Alt+Right' }),
    b('U', { type: 'scroll', dir: 'up', amount: 3 }),
    b('D', { type: 'scroll', dir: 'down', amount: 3 }),
    b('DR', { type: 'keys', keys: 'Ctrl+W' }),
    b('DL', { type: 'keys', keys: 'Ctrl+T' }),
    b('DU', { type: 'keys', keys: 'Ctrl+Shift+T' }),
    b('UD', { type: 'keys', keys: 'F5' }),
    b('LU', { type: 'window', op: 'minimize' }),
    b('RU', { type: 'window', op: 'maximize' }),
    b('LD', { type: 'keys', keys: 'Win+D' }),
    b('RD', { type: 'keys', keys: 'Alt+Tab' }),
    b('UDR', { type: 'media', key: 'mute' }),
    // 画个 S 截图 / 框选贴图 / 框选取字
    b('S', { type: 'snip', id: 'region' }),
    b('DRUL', { type: 'snip', id: 'pin-region' }),
    b('RDLU', { type: 'snip', id: 'ocr-region' })
  ]
}
