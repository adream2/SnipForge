import { useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { api, events } from './lib/api'
import './styles/ocr.css'

/**
 * 取字窗口（手势 →↓←↑ 框选之后弹这个）。
 *
 * 为什么独立小窗、且置顶：
 * 用户是在**别的程序**上框选的，下一步通常是"把这些字填进去"。
 * 弹主窗口会打断他手上的事；置顶小窗可以放在旁边，边看边抄。
 */
function Ocr() {
  const [img, setImg] = useState('')
  const [text, setText] = useState('')
  const [translated, setTranslated] = useState('')
  const [busy, setBusy] = useState(false)
  const [transBusy, setTransBusy] = useState(false)
  const [err, setErr] = useState('')
  const [copied, setCopied] = useState('')
  const [preview, setPreview] = useState(false)
  const areaRef = useRef<HTMLTextAreaElement>(null)

  const run = useRef<(dataUrl: string) => void>(() => {})

  useEffect(() => {
    run.current = (dataUrl: string) => {
      setImg(dataUrl)
      setText('')
      setTranslated('')
      setErr('')
      setPreview(false)
      setBusy(true)
      void api
        .imageOcr(dataUrl)
        .then((r) => {
          const t = (r.text ?? '').trim()
          setText(t)
          if (!t) setErr('这张图里没认出文字')
        })
        .catch((e) => setErr(String(e)))
        .finally(() => setBusy(false))
    }

    const unsub = events.onOcrImage((dataUrl) => run.current(dataUrl))
    // 窗口是刚建的时候，上面那个 emit 已经丢了 —— 主动来取一次
    void api.ocrTakeImage().then((img) => {
      if (img) run.current(img)
    })
    return () => {
      void unsub.then((u) => u())
    }
  }, [])

  const copy = async (what: 'text' | 'translated') => {
    const v = what === 'text' ? text : translated
    if (!v.trim()) return
    try {
      await api.clipboardCopyText(v)
      setCopied(what)
      window.setTimeout(() => setCopied((c) => (c === what ? '' : c)), 1200)
    } catch (e) {
      setErr(String(e))
    }
  }

  const translate = async () => {
    if (!text.trim() || transBusy) return
    setTransBusy(true)
    setErr('')
    try {
      setTranslated(await api.translateText(text))
    } catch (e) {
      setErr(String(e))
    } finally {
      setTransBusy(false)
    }
  }

  // Esc 关闭，Ctrl+C（没选中文本时）复制全文
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        void api.closeOcr()
      }
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'c') {
        const el = areaRef.current
        const selected = el && el.selectionEnd !== el.selectionStart
        if (!selected && text.trim()) {
          e.preventDefault()
          void copy('text')
        }
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [text])

  return (
    <div className="ocr-root">
      <header>
        <span className="ocr-title">取字</span>
        {busy && <span className="ocr-hint">识别中…</span>}
        {!busy && text && (
          <span className="ocr-hint">{text.replace(/\s/g, '').length} 字</span>
        )}
        <div style={{ flex: 1 }} />
        {img && (
          <button className="ocr-btn ghost" onClick={() => setPreview((v) => !v)}>
            {preview ? '收起原图' : '看原图'}
          </button>
        )}
      </header>

      {preview && img && (
        <div className="ocr-preview">
          <img src={img} alt="" />
        </div>
      )}

      <textarea
        ref={areaRef}
        className="ocr-text"
        value={text}
        spellCheck={false}
        placeholder={busy ? '' : '框选之后识别出来的文字会出现在这里，可以直接改'}
        onChange={(e) => setText(e.target.value)}
      />

      {err && <div className="ocr-err">{err}</div>}

      {translated && (
        <div className="ocr-trans">
          <div className="ocr-trans-head">
            <span>译文</span>
            <button className="ocr-btn tiny" onClick={() => void copy('translated')}>
              {copied === 'translated' ? '已复制' : '复制译文'}
            </button>
          </div>
          <div className="ocr-trans-body">{translated}</div>
        </div>
      )}

      <footer>
        <button className="ocr-btn primary" onClick={() => void copy('text')} disabled={!text.trim()}>
          {copied === 'text' ? '已复制' : '复制'}
        </button>
        <button className="ocr-btn" onClick={() => void translate()} disabled={!text.trim() || transBusy}>
          {transBusy ? '翻译中…' : '翻译'}
        </button>
        <div style={{ flex: 1 }} />
        <button className="ocr-btn ghost" onClick={() => void api.closeOcr()}>
          关闭
        </button>
      </footer>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(<Ocr />)
