import { useMemo, useRef, useState } from 'react'
import { api } from '../lib/api'
import type {
  AppSettings,
  GestureAction,
  GestureBinding,
  GestureConfig,
  GestureProfile
} from '../lib/types'
import { useToast } from '../components/Toast'
import { IconPlus, IconTrash } from '../components/Icons'
import { GestureShape } from '../components/GestureShape'
import { sameStroke, strokeOf } from '../lib/gestureShape'

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
  /** pattern：方向串/形状字符（自定义手势为空串）；points：后端规范化后的模板；
   *  raw：原始笔画（页面用它问后端"这一划会命中谁"） */
  onDone: (pattern: string, points?: number[][], raw?: Array<[number, number]>) => void
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
    const raw = path.map((p) => [Math.round(p.x), Math.round(p.y)] as [number, number])
    if (raw.length >= 12) {
      try {
        // 内置形状（S）有专用模板，优先用它 —— 比"自定义"更通用
        const shape = await api.gestureRecognize(raw)
        if (shape) {
          onDone(shape, undefined, raw)
          return
        }
        // 其余一律**连笔画一起存**：方向串（UDUD 这种）只是给人和日志看的，
        // M 和 W 的串可能撞，真正认的是这一笔的形状。
        const canon = await api.gestureNormalize(raw)
        if (canon.length >= 8) {
          onDone(pat, canon, raw)
          return
        }
      } catch {
        // 后端不可用（比如页面单独在浏览器里跑）：退回方向串，别把功能卡死
      }
    }
    if (pat) onDone(pat, undefined, raw)
    else onCancel()
  }

  return (
    <div className="gesture-rec">
      <div
        ref={box}
        className="gesture-rec-box"
        onPointerDown={(e) => {
          // 捕获指针：手滑出录制框也还能继续画。
          // 但某些注入/合成的指针事件拿不到有效 pointerId，会直接抛 ——
          // 抛出去就等于整个按下处理没执行，表现是"怎么画都没反应"。
          try {
            e.currentTarget.setPointerCapture(e.pointerId)
          } catch {
            /* 捕获失败不影响正常绘制 */
          }
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
            {preview ? '方向串识别中…' : '按住左键划一下：方向串或任意形状（S / M / 圈…）'}
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

/* ---------------- 绑定列表（通用 / 程序专用共用一个） ---------------- */

function BindingsCard({
  title,
  subtitle,
  bindings,
  onChange,
  scope,
  onRestore,
  emptyHint
}: {
  title: string
  subtitle?: string
  bindings: GestureBinding[]
  onChange: (next: GestureBinding[]) => void
  /** 程序专用集的 id；通用传 null —— 决定"查重"在哪个范围里找 */
  scope: string | null
  onRestore?: () => void
  emptyHint?: string
}) {
  const toast = useToast()
  const [recording, setRecording] = useState(false)
  const [testing, setTesting] = useState(false)
  const [openIdx, setOpenIdx] = useState<number | null>(null)

  /**
   * 哪几行"画出来是一个样子"。
   *
   * 方向串可以只差起笔方向（`↓→↑←` 和 `→↓←↑` 都是同一个方框），列表里看着
   * 就是两个一模一样的手势干着不同的活 —— 与其让用户自己撞南墙，
   * 不如直接把重复标出来。
   */
  const dupOf = useMemo(() => {
    const sigs = bindings.map((b) => strokeOf(b))
    return sigs.map((s, i) => sigs.findIndex((o, j) => j !== i && sameStroke(s, o)))
  }, [bindings])

  /**
   * 新增一条手势。
   *
   * 先问后端"这一划会命中谁"：命中已有的就不新增，直接把那一行展开 ——
   * 否则用户会得到两条几乎一样的手势，划的时候永远只触发靠前那条，
   * 后面那条成了"怎么画都没反应"的幽灵。
   */
  const addGesture = (pattern: string, points?: number[][], raw?: Array<[number, number]>) => {
    setRecording(false)
    const upper = pattern.toUpperCase()
    const insert = () => {
      const next = [
        ...bindings,
        { pattern: upper, points: points ?? [], action: defaultAction('keys'), enabled: true }
      ]
      onChange(next)
      setOpenIdx(next.length - 1)
    }
    if (!raw || raw.length < 12) {
      insert()
      return
    }
    void api
      .gestureMatch(raw, upper, scope ?? undefined)
      .then((idx) => {
        if (idx !== null && bindings[idx]) {
          toast.push(
            `这一划会命中第 ${idx + 1} 行（${actionSummary(bindings[idx].action)}），改那一行就行`,
            'error'
          )
          setOpenIdx(idx)
          return
        }
        insert()
      })
      .catch(insert)
  }

  return (
    <div className="setting-card">
      <header style={{ display: 'flex', alignItems: 'center', gap: 8, flexWrap: 'wrap' }}>
        <span>{title}</span>
        <span style={{ fontWeight: 400, textTransform: 'none' }}>{bindings.length}</span>
        <div style={{ flex: 1 }} />
        {!recording && !testing && (
          <button className="btn sm" onClick={() => setRecording(true)}>
            <IconPlus size={12} /> 划一个
          </button>
        )}
        <button className="btn sm" onClick={() => setTesting((v) => !v)}>
          {testing ? '收起试划' : '试划'}
        </button>
        {onRestore && (
          <button className="btn sm" onClick={onRestore}>
            恢复默认
          </button>
        )}
      </header>

      {subtitle && (
        <div style={{ padding: '0 16px 10px', fontSize: 11.5, color: 'var(--text-3)' }}>{subtitle}</div>
      )}

      {recording && <Recorder onDone={addGesture} onCancel={() => setRecording(false)} />}

      {testing && (
        <div style={{ borderTop: '1px solid var(--border)' }}>
          <Recorder
            onDone={(p, _pts, raw) => {
              // 和真手势同一套匹配顺序（自定义笔画优先，再方向串）
              void api
                .gestureMatch(raw ?? [], p.toUpperCase(), scope ?? undefined)
                .then((idx) => {
                  const hit = idx === null ? undefined : bindings[idx]
                  toast.push(
                    hit ? `这一划 → ${actionSummary(hit.action)}` : '这一划还没有绑定',
                    hit ? 'success' : 'error'
                  )
                })
                .catch(() => toast.push('识别失败', 'error'))
            }}
            onCancel={() => toast.push('没划出东西', 'error')}
          />
          <div style={{ padding: '0 16px 12px', fontSize: 11, color: 'var(--text-3)' }}>
            只试识别，不会真的执行动作。
          </div>
        </div>
      )}

      {bindings.length === 0 && !recording && (
        <div style={{ padding: '4px 16px 16px', fontSize: 11.5, color: 'var(--text-3)' }}>
          {emptyHint ?? '还没有手势。点「划一个」在框里划一下就能新建。'}
        </div>
      )}

      {bindings.map((b, i) => {
        const open = openIdx === i
        return (
          <div className="ga-row-wrap" key={`${b.pattern}-${i}`}>
            <div className={`ga-row ${open ? 'open' : ''}`} onClick={() => setOpenIdx(open ? null : i)}>
              <span className="ga-shape">
                <GestureShape
                  pattern={b.pattern}
                  points={b.points}
                  title={b.pattern ? arrows(b.pattern) : '自定义手势'}
                />
              </span>
              <span className="ga-type">{typeLabel(b.action.type)}</span>
              <span className="ga-summary">{actionSummary(b.action)}</span>
              {dupOf[i] >= 0 && (
                <span
                  className="ga-dup"
                  title={`和第 ${dupOf[i] + 1} 行画出来几乎一样（只是起笔/走向不同），划的时候容易分不清`}
                >
                  同形
                </span>
              )}
              <div style={{ flex: 1 }} />
              <button
                className={`switch ${b.enabled ? 'on' : ''}`}
                title={b.enabled ? '停用这一条（不删，随时可恢复）' : '启用这一条'}
                onClick={(e) => {
                  e.stopPropagation()
                  const next = bindings.slice()
                  next[i] = { ...b, enabled: !b.enabled }
                  onChange(next)
                }}
              />
              <button
                className="btn sm danger"
                title="删掉这个手势"
                onClick={(e) => {
                  e.stopPropagation()
                  onChange(bindings.filter((_, j) => j !== i))
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
                    const next = bindings.slice()
                    next[i] = { ...b, action: a }
                    onChange(next)
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
  )
}

/* ---------------- 程序专用 ---------------- */

function ProfilesView({
  g,
  patch
}: {
  g: GestureConfig
  patch: (p: Partial<GestureConfig>) => void
}) {
  const toast = useToast()
  const profiles = g.profiles ?? []
  const [sel, setSel] = useState<string | null>(profiles[0]?.id ?? null)
  const current = profiles.find((p) => p.id === sel) ?? profiles[0] ?? null

  const save = (next: GestureProfile[]) => patch({ profiles: next })

  const addProfile = () => {
    const id = `p${Date.now().toString(36)}`
    const next = [
      ...profiles,
      { id, name: '新程序', exes: [] as string[], bindings: [] as GestureBinding[] }
    ]
    save(next)
    setSel(id)
  }

  const update = (p: Partial<GestureProfile>) => {
    if (!current) return
    save(profiles.map((x) => (x.id === current.id ? { ...x, ...p } : x)))
  }

  return (
    <div className="ga-profiles">
      <div className="setting-card ga-profile-list">
        <header style={{ display: 'flex', alignItems: 'center' }}>
          <span>程序</span>
          <div style={{ flex: 1 }} />
          <button className="btn sm" onClick={addProfile} title="为一个新程序单独配手势">
            <IconPlus size={12} />
          </button>
        </header>
        {profiles.length === 0 && (
          <div style={{ padding: '10px 16px', fontSize: 11.5, color: 'var(--text-3)' }}>
            还没有专用集
          </div>
        )}
        {profiles.map((p) => (
          <div
            key={p.id}
            className={`ga-profile-item ${current?.id === p.id ? 'on' : ''}`}
            onClick={() => setSel(p.id)}
          >
            <b>{p.name}</b>
            <span>{p.exes.length ? p.exes.join(' / ') : '还没填程序'}</span>
            <em>{p.bindings.length} 条</em>
          </div>
        ))}
        <div style={{ padding: '10px 14px', fontSize: 11, color: 'var(--text-3)', lineHeight: 1.6 }}>
          专用集里的手势优先；这里没定义的，仍然按通用手势执行。
        </div>
      </div>

      <div style={{ minWidth: 0 }}>
        {current ? (
          <>
            <div className="setting-card" style={{ marginBottom: 14 }}>
              <header>基本信息</header>
              <div className="setting-row">
                <div className="label">
                  <strong>名称</strong>
                  <span>只是列表里给人看的</span>
                </div>
                <div className="control">
                  <input
                    className="input"
                    style={{ width: 180 }}
                    value={current.name}
                    onChange={(e) => update({ name: e.target.value })}
                  />
                </div>
              </div>
              <div className="setting-row">
                <div className="label">
                  <strong>程序</strong>
                  <span>
                    进程名，逗号或空格分隔，带不带 .exe 都行。例如 chrome, msedge, firefox
                  </span>
                </div>
                <div className="control" style={{ flex: 1 }}>
                  <input
                    className="input"
                    style={{ width: '100%', maxWidth: 420 }}
                    value={current.exes.join(', ')}
                    spellCheck={false}
                    placeholder="chrome, msedge, firefox"
                    onChange={(e) =>
                      update({
                        exes: e.target.value
                          .split(/[,，\s]+/)
                          .map((x) => x.trim())
                          .filter(Boolean)
                      })
                    }
                  />
                </div>
              </div>
              <div className="setting-row">
                <div className="label">
                  <strong>删除这一套</strong>
                  <span>删掉后这些程序就按通用手势执行</span>
                </div>
                <div className="control">
                  <button
                    className="btn sm danger"
                    onClick={() => {
                      save(profiles.filter((x) => x.id !== current.id))
                      setSel(profiles.find((x) => x.id !== current.id)?.id ?? null)
                      toast.push('已删除', 'success')
                    }}
                  >
                    <IconTrash size={12} /> 删除
                  </button>
                </div>
              </div>
            </div>

            <BindingsCard
              title={`${current.name}的手势`}
              subtitle="在这些程序里生效；同一条手势在这里优先于通用手势"
              bindings={current.bindings}
              onChange={(next) => update({ bindings: next })}
              scope={current.id}
              emptyHint="还没有专用手势 —— 点「划一个」加一条，或按通用手势的习惯来配。"
            />
          </>
        ) : (
          <div className="setting-card">
            <div style={{ padding: 24, fontSize: 12, color: 'var(--text-3)' }}>
              左边新建一个程序，就能给它单独配手势了（浏览器已经预置好一套）。
            </div>
          </div>
        )}
      </div>
    </div>
  )
}

/* ---------------- 页面 ---------------- */

type Tab = 'global' | 'profiles' | 'settings'

export default function GesturePage({
  settings,
  onChange
}: {
  settings: AppSettings | null
  onChange: (patch: unknown) => Promise<unknown>
}) {
  const toast = useToast()
  const [tab, setTab] = useState<Tab>('global')

  const g: GestureConfig | undefined = settings?.gestures

  const patch = (p: Partial<GestureConfig>) => {
    if (!g) return
    void onChange({ gestures: { ...g, ...p } }).catch((e) => toast.push(String(e), 'error'))
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
          <div className="ga-tabs">
            {(
              [
                ['global', '通用手势'],
                ['profiles', '程序专用'],
                ['settings', '手势设置']
              ] as Array<[Tab, string]>
            ).map(([id, label]) => (
              <button key={id} className={tab === id ? 'on' : ''} onClick={() => setTab(id)}>
                {label}
              </button>
            ))}
            <div style={{ flex: 1 }} />
            <span className="ga-tabs-hint">
              手势会发到按下的那个窗口；程序专用集优先，其余回落通用
            </span>
          </div>

          {tab === 'global' && (
            <>
              <BindingsCard
                title="通用手势"
                subtitle="所有程序里都生效（程序专用集里定义过的，那几个程序里优先用专用集）"
                bindings={g.bindings}
                onChange={(next) => patch({ bindings: next })}
                scope={null}
                onRestore={() =>
                  void onChange({ gestures: { ...g, bindings: DEFAULT_BINDINGS() } })
                    .then(() => toast.push('已恢复默认手势', 'success'))
                    .catch((e) => toast.push(String(e), 'error'))
                }
              />
              <div className="ga-note" style={{ padding: '10px 4px 24px' }}>
                手势会发送到你按下的那个窗口，所以先在目标窗口里划。改完立刻生效，不用重启。
                <br />
                三种手势：方向串（↑↓←→ 依次组合）、内置形状（S）和**自定义形状** ——
                点「划一个」，随便画一个 M、W、圈或任何你顺手的形状，它就成了一个手势。
                自定义形状按笔画相似度匹配（阈值很严，只认几乎一样的那一笔），
                列表里统一画成"起点圆点 + 终点箭头"，不用箭头符号去描述方向。
              </div>
            </>
          )}

          {tab === 'profiles' && <ProfilesView g={g} patch={patch} />}

          {tab === 'settings' && (
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
          )}
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
    b('UD', { type: 'keys', keys: 'F5' }),
    b('LU', { type: 'window', op: 'minimize' }),
    b('RU', { type: 'window', op: 'maximize' }),
    b('LD', { type: 'keys', keys: 'Win+D' }),
    b('RD', { type: 'keys', keys: 'Alt+Tab' }),
    b('UDR', { type: 'media', key: 'mute' }),
    // 画个 S 截图 / 框选贴图 / 框选取字（后者是开口框，别和闭合方框撞脸）
    b('S', { type: 'snip', id: 'region' }),
    b('DRUL', { type: 'snip', id: 'pin-region' }),
    b('DRU', { type: 'snip', id: 'ocr-region' })
  ]
}
