import { useCallback, useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { api } from './lib/api'
import type { RecordStatus } from './lib/types'
import './styles/global.css'
import './styles/record.css'

type Phase = 'countdown' | 'recording' | 'paused' | 'saving'

function fmt(ms: number): string {
  const total = Math.floor(ms / 1000)
  const m = Math.floor(total / 60)
  const s = total % 60
  return `${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
}

function RecordBar() {
  const [phase, setPhase] = useState<Phase>('countdown')
  const [left, setLeft] = useState(3)
  const [elapsed, setElapsed] = useState(0)
  const [lens, setLens] = useState(false)
  const [anno, setAnno] = useState(false)
  const [fps, setFps] = useState(0)
  const [err, setErr] = useState('')
  /** 只允许 begin 一次，倒计时的重入会写出重复帧 */
  const began = useRef(false)

  // 倒计时 → 开录
  useEffect(() => {
    let cancelled = false
    void api
      .settingsGet()
      .then((r) => {
        if (!cancelled) setLeft(Math.max(0, r.settings.record.countdown))
      })
      .catch(() => setLeft(3))
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    if (phase !== 'countdown') return
    if (left <= 0) {
      if (began.current) return
      began.current = true
      void api
        .recordBegin()
        .then(() => setPhase('recording'))
        .catch((e) => setErr(String(e)))
      return
    }
    const t = window.setTimeout(() => setLeft((v) => v - 1), 700)
    return () => window.clearTimeout(t)
  }, [phase, left])

  // 录制中轮询状态：时长、实际帧率、暂停标记
  useEffect(() => {
    if (phase !== 'recording' && phase !== 'paused') return
    let alive = true
    const tick = async () => {
      try {
        const st: RecordStatus | null = await api.recordStatus()
        if (!alive || !st) return
        setElapsed(st.elapsedMs)
        setFps(st.fps)
        setLens(st.lens)
        setPhase(st.paused ? 'paused' : 'recording')
      } catch {
        /* 录制刚结束的那一瞬可能查不到，忽略 */
      }
    }
    void tick()
    const id = window.setInterval(tick, 250)
    return () => {
      alive = false
      window.clearInterval(id)
    }
  }, [phase])

  const togglePause = useCallback(async () => {
    try {
      const paused = await api.recordTogglePause()
      setPhase(paused ? 'paused' : 'recording')
    } catch (e) {
      setErr(String(e))
    }
  }, [])

  const toggleLens = useCallback(async () => {
    const next = !lens
    setLens(next)
    try {
      await api.recordSetLens(next)
    } catch (e) {
      setErr(String(e))
    }
  }, [lens])

  const toggleAnno = useCallback(async () => {
    const next = !anno
    setAnno(next)
    try {
      await api.recordSetInteractive(next)
    } catch (e) {
      setErr(String(e))
    }
  }, [anno])

  const stop = useCallback(async () => {
    setPhase('saving')
    try {
      await api.recordStop()
    } catch (e) {
      setErr(String(e))
    }
  }, [])

  // 空格暂停、Esc 停止 —— 鼠标在别处时也能操作
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        void stop()
      } else if (e.key === ' ' && phase !== 'saving' && phase !== 'countdown') {
        e.preventDefault()
        void togglePause()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [phase, stop, togglePause])

  if (phase === 'countdown') {
    return (
      <div className="rb-root rb-count">
        <div className="rb-count-num">{left > 0 ? left : '开录'}</div>
        <div className="rb-count-tip">准备好，录制即将开始 · Esc 取消</div>
      </div>
    )
  }

  return (
    <div className="rb-root">
      <div className="rb-drag" data-tauri-drag-region />

      <div className={`rb-dot ${phase === 'paused' ? 'paused' : ''}`} />
      <div className="rb-time">{fmt(elapsed)}</div>
      <div className="rb-meta">
        {Math.round(fps) || '—'} fps
        {phase === 'paused' && <span className="rb-tag">已暂停</span>}
      </div>

      <div className="rb-sep" />

      <button
        className="rb-btn"
        onClick={() => void togglePause()}
        disabled={phase === 'saving'}
        title={phase === 'paused' ? '继续（空格）' : '暂停（空格）'}
      >
        {phase === 'paused' ? '继续' : '暂停'}
      </button>

      <button
        className={`rb-btn ${anno ? 'on' : ''}`}
        onClick={() => void toggleAnno()}
        title="在画面上标注"
      >
        标注
      </button>

      <button
        className={`rb-btn ${lens ? 'on' : ''}`}
        onClick={() => void toggleLens()}
        title="放大镜跟随鼠标"
      >
        放大
      </button>

      <button className="rb-stop" onClick={() => void stop()} disabled={phase === 'saving'}>
        {phase === 'saving' ? '保存中…' : '停止'}
      </button>

      {err && <div className="rb-err">{err}</div>}
    </div>
  )
}

const container = document.getElementById('root')
if (container) {
  createRoot(container).render(<RecordBar />)
}
