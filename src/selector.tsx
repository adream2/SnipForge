import { useCallback, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { api, events } from './lib/api'
import type { DisplayPreview, FramePayload, NativeElement, Rect, RecordFormat } from './lib/types'
import { IconCheck, IconMonitor, IconScroll, IconX } from './components/Icons'
import './styles/selector.css'

/**
 * 截图覆盖层。
 *
 * 坐标系有三套，必须时刻清楚自己在哪一套里：
 *   1. 物理像素 —— 后端返回的显示器 bounds、窗口 bounds
 *   2. CSS 像素  —— DOM 里鼠标事件拿到的
 *   3. 换算关系  —— css = (phys - displayOrigin) / scaleFactor
 *
 * 提交给后端的一律是物理像素。
 */
const PARAMS =
  typeof location !== 'undefined' ? new URLSearchParams(location.search) : new URLSearchParams()

/** 从 URL 读模式：selector.html?mode=record&fmt=gif 表示框完直接录 GIF */
const IS_RECORD = PARAMS.get('mode') === 'record'
/** 框完直接干什么：pin = 贴到屏幕，ocr = 取字。空 = 进编辑器 */
const AFTER = PARAMS.get('after')
/** 进来时带的格式（由点击的按钮/热键决定）；在选区里还能按 G / V 改 */
const INITIAL_FMT: RecordFormat = PARAMS.get('fmt') === 'gif' ? 'gif' : 'mp4'

/**
 * 智能窗口选择（截图和录屏都有）：
 *   光标停在哪，哪就亮 —— 亮的是**光标底下的那个控件**（按钮、编辑框、页面区），
 *   单击选中它，拖拽则框任意区域。
 *
 * 为什么不设一个"窗口模式"再让人去切：窗口截图本来就该是区域截图的一个子集，
 * 多一个模式只是多一步操作。要整个窗口时按住 Ctrl 就行。
 */

export default function Selector() {
  const [frame, setFrame] = useState<FramePayload | null>(null)
  const [display, setDisplay] = useState<DisplayPreview | null>(null)
  /** 光标底下的窗口/组件。悬停高亮、单击选中都用它 */
  const [hover, setHover] = useState<NativeElement | null>(null)
  const [anchor, setAnchor] = useState<{ x: number; y: number } | null>(null)
  const [cursor, setCursor] = useState<{ x: number; y: number }>({ x: 0, y: 0 })
  const [dragging, setDragging] = useState(false)
  const [selection, setSelection] = useState<Rect | null>(null)
  const [mode, setMode] = useState<'region' | 'scroll'>('region')
  /** 录成什么：视频还是 GIF。跟着进来的动作走，选区里可当场改 */
  const [fmt, setFmt] = useState<RecordFormat>(INITIAL_FMT)
  const [busy, setBusy] = useState(false)
  const rootRef = useRef<HTMLDivElement>(null)

  const scale = display?.scaleFactor ?? 1

  useEffect(() => {
    let disposed = false
    const boot = async () => {
      try {
        const payload = await api.captureFrame()
        if (disposed) return
        setFrame(payload)
        setDisplay(payload.displays[0] ?? null)
      } catch {
        // 帧已失效，直接退出
        void api.captureCancel()
      }
    }
    void boot()
    const unsub = events.onSelectorDisplay((id) => {
      setFrame((prev) => {
        const d = prev?.displays.find((x) => x.id === id)
        if (d) setDisplay(d)
        return prev
      })
    })
    return () => {
      disposed = true
      void unsub.then((u) => u())
    }
  }, [])

  /** 物理坐标 → CSS 坐标 */
  const toCss = useCallback(
    (physX: number, physY: number) => {
      if (!display) return { x: 0, y: 0 }
      return {
        x: (physX - display.bounds.x) / scale,
        y: (physY - display.bounds.y) / scale
      }
    },
    [display, scale]
  )

  /**
   * 查光标底下是什么。**只问后端**，不在前端缓存窗口列表：
   *   1. 只有后端能拿到 z 序和子窗口，前端按矩形过滤会选中背景里那个大窗口；
   *   2. 组件级识别（按钮、编辑框、页面区）本来就是 Win32 的活儿。
   * 后发先至的情况用序号丢弃旧结果，否则高亮会在两个窗口之间来回跳。
   */
  const hoverSeq = useRef(0)
  const lastQuery = useRef({ x: -999, y: -999, whole: false, t: 0 })

  /** 查一次光标下是什么，返回结果（不写 state）。松手落点判定要用"这一刻"的答案，
   * 用状态里缓存的值会拿到上一个位置的，点错窗口就是这么来的。 */
  const fetchElement = useCallback(
    async (cssX: number, cssY: number, whole: boolean) => {
      if (!display) return null
      const px = Math.round(display.bounds.x + cssX * scale)
      const py = Math.round(display.bounds.y + cssY * scale)
      lastQuery.current = { x: cssX, y: cssY, whole, t: performance.now() }
      try {
        return await api.captureElement(px, py, whole)
      } catch {
        return null
      }
    },
    [display, scale]
  )

  const queryElement = useCallback(
    (cssX: number, cssY: number, whole: boolean) => {
      const seq = ++hoverSeq.current
      void fetchElement(cssX, cssY, whole).then((el) => {
        if (seq === hoverSeq.current) setHover(el)
      })
    },
    [fetchElement]
  )

  // 覆盖层刚出现时就把光标下的窗口高亮出来。
  // 只靠 mousemove 的话，用户不动鼠标就永远看不到高亮，会觉得"识别不到窗口"。
  useEffect(() => {
    if (!display) return
    let alive = true
    void api
      .cursorPos()
      .then((pos) => {
        if (!alive || !display) return
        const [px, py] = pos
        const r = rootRef.current?.getBoundingClientRect()
        const x = (px - display.bounds.x) / scale - (r?.left ?? 0)
        const y = (py - display.bounds.y) / scale - (r?.top ?? 0)
        queryElement(x, y, false)
      })
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [display, scale, queryElement])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        void api.captureCancel()
      }
      if (e.key === 'Enter' && selection) {
        e.preventDefault()
        void commit()
      }
      if (e.key === 's' && !e.ctrlKey && !e.metaKey) setMode('scroll')
      // 录屏模式：G = 录 GIF，V = 录视频。不用退出去重来
      if (IS_RECORD && !e.ctrlKey && !e.metaKey && !e.altKey) {
        if (e.key === 'g') setFmt('gif')
        if (e.key === 'v') setFmt('mp4')
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selection])

  const normalize = (a: { x: number; y: number }, b: { x: number; y: number }): Rect => ({
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(a.x - b.x),
    height: Math.abs(a.y - b.y)
  })

  const onMouseDown = (e: React.MouseEvent) => {
    if (e.button !== 0 || busy) return
    const rect = rootRef.current?.getBoundingClientRect()
    if (!rect) return
    const x = e.clientX - rect.left
    const y = e.clientY - rect.top

    setAnchor({ x, y })
    setCursor({ x, y })
    setDragging(true)
    setSelection({ x, y, width: 0, height: 0 })
  }

  /** 选中一个窗口/组件。它的 rect 是物理像素，selection 这一层全是 CSS 像素
   * （commit 里还会乘回物理像素）。不换算的话，缩放屏上选出来的框
   * 和窗口对不上，尺寸标签也是错的。 */
  const pickElement = (target: NativeElement) => {
    setSelection({
      x: (target.rect.x - (display?.bounds.x ?? 0)) / scale,
      y: (target.rect.y - (display?.bounds.y ?? 0)) / scale,
      width: target.rect.width / scale,
      height: target.rect.height / scale
    })
    setHover(null)
  }

  /** 这个点是否已经在当前高亮的那个矩形里 */
  const inHover = (cssX: number, cssY: number) => {
    if (!hover || !display) return false
    const left = (hover.rect.x - display.bounds.x) / scale
    const top = (hover.rect.y - display.bounds.y) / scale
    return (
      cssX >= left &&
      cssX < left + hover.rect.width / scale &&
      cssY >= top &&
      cssY < top + hover.rect.height / scale
    )
  }

  const onMouseMove = (e: React.MouseEvent) => {
    const rect = rootRef.current?.getBoundingClientRect()
    if (!rect) return
    const x = e.clientX - rect.left
    const y = e.clientY - rect.top

    if (dragging && anchor) {
      setCursor({ x, y })
      setSelection(normalize(anchor, { x, y }))
      return
    }
    // 已经选好了一块：不再跟着鼠标改高亮，免得看着像选区在跳
    if (selection || mode === 'scroll') return

    // 还在当前高亮的那个窗口/控件里挪动：结果不会变，没必要再问后端
    // （枚举一次要过几百个窗口，鼠标一划就是每秒上百次，省下来才有余量）
    if (hover && e.ctrlKey === lastQuery.current.whole && inHover(x, y)) return

    // 节流：同一个小控件里挪几像素没必要每帧问一次后端
    const q = lastQuery.current
    const moved = Math.abs(x - q.x) + Math.abs(y - q.y)
    const now = performance.now()
    if (moved > 3 || e.ctrlKey !== q.whole || now - q.t > 300) {
      queryElement(x, y, e.ctrlKey)
    }
  }

  const onMouseUp = (e: React.MouseEvent) => {
    const wasDragging = dragging && anchor
    setDragging(false)
    if (!wasDragging) return

    // 原地单击（几乎没拖动）→ 当作"选光标底下这个窗口/控件"。
    // 拖出去了就是自由框选，走正常流程。
    const rect = rootRef.current?.getBoundingClientRect()
    if (!rect) return
    const x = e.clientX - rect.left
    const y = e.clientY - rect.top
    const moved = Math.abs(x - anchor.x) + Math.abs(y - anchor.y) > 6
    if (moved || mode === 'scroll') return

    void fetchElement(x, y, e.ctrlKey).then((el) => {
      if (el) pickElement(el)
      // 底下什么都没有（桌面）：把这次"空选区"清掉，别留一个点
      else setSelection(null)
    })
  }

  const commit = async () => {
    if (!selection || !display || busy) return
    if (selection.width < 4 || selection.height < 4) return
    setBusy(true)
    // CSS → 物理
    const phys: Rect = {
      x: Math.round(display.bounds.x + selection.x * scale),
      y: Math.round(display.bounds.y + selection.y * scale),
      width: Math.round(selection.width * scale),
      height: Math.round(selection.height * scale)
    }
    try {
      if (AFTER === 'pin') {
        // 手势 ↓→↑←：框完直接贴到屏幕上
        await api.capturePin(display.id, phys)
      } else if (AFTER === 'ocr') {
        // 手势 →↓←↑：框完直接取字
        await api.captureOcr(display.id, phys)
      } else if (mode === 'scroll' && !IS_RECORD) {
        // 录屏没有"滚动长录"这种东西，这里只可能来自截图模式
        await api.captureScroll(display.id, phys)
      } else {
        if (IS_RECORD) {
          // 录屏：后端会先关掉这个覆盖层再开采集，避免把深色遮罩录进去
          await api.recordStartRegion(phys, fmt)
        } else {
          await api.captureCommit(display.id, phys)
        }
      }
    } catch {
      setBusy(false)
      void api.captureCancel()
    }
  }

  const hoverRect = hover
    ? {
        left: (hover.rect.x - (display?.bounds.x ?? 0)) / scale,
        top: (hover.rect.y - (display?.bounds.y ?? 0)) / scale,
        width: hover.rect.width / scale,
        height: hover.rect.height / scale
      }
    : null

  const sizeLabel = selection
    ? `${Math.round(selection.width * scale)} × ${Math.round(selection.height * scale)}`
    : ''

  const toolbarStyle = (() => {
    if (selection) {
      const below = selection.y + selection.height + 10
      const top = below + 40 < window.innerHeight ? below : Math.max(8, selection.y - 46)
      const left = Math.min(Math.max(8, selection.x + selection.width - 244), window.innerWidth - 252)
      return { top, left } as React.CSSProperties
    }
    if (IS_RECORD) {
      // 录屏模式没有选区时工具栏也常驻：窗口选择（W）这个能力藏在一个
      // 看不见的快捷键后面，第一次用的人根本发现不了。
      // 放底部居中。注意要抬到任务栏上面 —— 任务栏是置顶窗口，
      // bottom:14 这种"贴边"位置会被它整个盖住，看起来像没渲染。
      return {
        bottom: 60,
        left: Math.max(8, (window.innerWidth - 236) / 2)
      } as React.CSSProperties
    }
    return { display: 'none' } as React.CSSProperties
  })()

  return (
    <div className="selector-root" ref={rootRef}>
      {/*
        交互层单独一个 div，铺满整个窗口但只包住背景和遮罩。
        工具栏是它的兄弟节点 —— 这样点按钮的 mousedown 根本不会冒泡到选区逻辑上。
        之前把事件绑在 root 上，点"确认"会先触发一次 mousedown 开始新选区，
        于是按钮永远点不动。
      */}
      <div
        className="selector-surface"
        onMouseDown={onMouseDown}
        onMouseMove={onMouseMove}
        onMouseUp={onMouseUp}
        onDoubleClick={() => void commit()}
        onContextMenu={(e) => {
          e.preventDefault()
          void api.captureCancel()
        }}
      >
      {display && (
        <img className="selector-bg" src={display.preview} alt="" draggable={false} />
      )}

      {hoverRect && hover && !selection && mode !== 'scroll' && (
        <>
          <div
            className={`selector-win-hint ${hover.kind === 'component' ? 'is-component' : ''}`}
            style={{
              left: hoverRect.left,
              top: hoverRect.top,
              width: hoverRect.width,
              height: hoverRect.height
            }}
          />
          {/* 标签：让"识别到了哪个窗口/控件"这件事看得见，而不只是一圈描边 */}
          <div
            className="selector-win-tag"
            style={{
              left: hoverRect.left,
              top: hoverRect.top > 26 ? hoverRect.top - 24 : hoverRect.top + 4
            }}
          >
            <b>{hover.kind === 'component' ? hover.title || hover.className : hover.title}</b>
            <span>
              {hover.kind === 'component' ? '控件' : '窗口'} {Math.round(hover.rect.width)} ×{' '}
              {Math.round(hover.rect.height)}
            </span>
            {hover.kind === 'component' && <em>按住 Ctrl 选整个窗口</em>}
          </div>
        </>
      )}

      {selection && (
        <>
          <div
            className="selector-mask"
            style={{
              left: selection.x,
              top: selection.y,
              width: Math.max(1, selection.width),
              height: Math.max(1, selection.height)
            }}
          />
          <div
            className="selector-edge"
            style={{
              left: selection.x,
              top: selection.y,
              width: Math.max(1, selection.width),
              height: Math.max(1, selection.height)
            }}
          />
          {/* 四角把手 */}
          {[
            ['nw', selection.x, selection.y],
            ['ne', selection.x + selection.width, selection.y],
            ['sw', selection.x, selection.y + selection.height],
            ['se', selection.x + selection.width, selection.y + selection.height]
          ].map(([k, l, t]) => (
            <div
              key={String(k)}
              className="selector-handle"
              style={{ left: Number(l) - 3, top: Number(t) - 3 }}
            />
          ))}
          <div
            className="selector-size-tag"
            style={{
              left: selection.x,
              top: selection.y + selection.height + 6 > window.innerHeight - 30
                ? selection.y - 26
                : selection.y + selection.height + 6
            }}
          >
            {sizeLabel}
          </div>
        </>
      )}

      </div>

      <div
        className="selector-toolbar"
        style={toolbarStyle}
        onMouseDown={(e) => e.stopPropagation()}
        onDoubleClick={(e) => e.stopPropagation()}
        onContextMenu={(e) => {
          e.preventDefault()
          e.stopPropagation()
        }}
      >
        <button
          className={mode === 'region' ? 'active' : ''}
          title="区域截图"
          onClick={() => setMode('region')}
        >
          <IconMonitor size={15} />
        </button>
        {!IS_RECORD && (
          <button
            className={mode === 'scroll' ? 'active' : ''}
            title="滚动长截图（S）"
            onClick={() => setMode('scroll')}
          >
            <IconScroll size={15} />
          </button>
        )}
        {IS_RECORD && (
          <>
            <div className="divider" />
            <div className="selector-fmt">
              <button
                className={fmt === 'mp4' ? 'active' : ''}
                title="录成视频 MP4（V）"
                onClick={() => setFmt('mp4')}
              >
                视频
              </button>
              <button
                className={fmt === 'gif' ? 'active' : ''}
                title="录成 GIF 动图（G）"
                onClick={() => setFmt('gif')}
              >
                GIF
              </button>
            </div>
          </>
        )}
        <div className="divider" />
        <button title="取消（Esc）" onClick={() => void api.captureCancel()}>
          <IconX size={15} />
        </button>
        <button
          title={
            IS_RECORD
              ? `开始录制${fmt === 'gif' ? ' GIF' : '视频'}（Enter）`
              : '确认（Enter）'
          }
          onClick={() => void commit()}
          style={{ background: 'var(--accent)' }}
        >
          <IconCheck size={15} />
        </button>
      </div>

      {!selection && (
        <div
          className="selector-hint"
          style={{ pointerEvents: 'none', bottom: IS_RECORD ? 112 : undefined }}
        >
          {IS_RECORD
            ? `${fmt === 'gif' ? 'GIF' : '视频'}录制 · 移到窗口上单击选中 · 拖拽框选任意区域 · G/V 切格式 · 回车开始`
            : '移到窗口/控件上单击选中 · 拖拽框选 · Ctrl 选整个窗口 · 回车确认 · Esc 取消 · S 长截图'}
        </div>
      )}
    </div>
  )
}

// 独立窗口的挂载点
const container = document.getElementById('root')
if (container) {
  createRoot(container).render(<Selector />)
}
