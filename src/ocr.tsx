import { useEffect, useRef, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { api, events } from './lib/api'
import { LANGUAGES, providerLabel } from './lib/translate'
import './styles/ocr.css'

/**
 * 取字 / 翻译窗口（手势框选之后弹这个）。
 *
 * 为什么独立小窗、且置顶：用户是在**别的程序**上框选的，下一步通常是
 * "把这些字填进去"或"看懂它"。弹主窗口会打断他手上的事；置顶小窗可以
 * 放在旁边，边看边抄。
 *
 * 左右分栏：左边是识别出来的原文（可以改），右边是译文。
 * 改完原文点「重译」；换语言会自动重译。
 */
function Ocr() {
  const [img, setImg] = useState('')
  const [text, setText] = useState('')
  const [translated, setTranslated] = useState('')
  const [busy, setBusy] = useState(false)
  const [transBusy, setTransBusy] = useState(false)
  const [err, setErr] = useState('')
  const [transErr, setTransErr] = useState('')
  const [copied, setCopied] = useState('')
  const [preview, setPreview] = useState(false)
  const [provider, setProvider] = useState('tencent')
  const [target, setTarget] = useState('zh-CN')
  const areaRef = useRef<HTMLTextAreaElement>(null)
  /** 自动翻译发生在"识别完成"那一刻，而那个回调是挂载时注册的；
      直接读 state 会拿到挂载时的旧值（比如配置里是英文也按中文翻） */
  const targetRef = useRef(target)
  targetRef.current = target
  /** 当前这笔译文对应的原文，避免"改了原文但右边还是旧译文"时看不出 */
  const [translatedFrom, setTranslatedFrom] = useState('')

  const run = useRef<(dataUrl: string) => void>(() => {})
  const doTranslate = useRef<(value: string, lang: string) => void>(() => {})

  doTranslate.current = (value: string, lang: string) => {
    const t = value.trim()
    if (!t) return
    setTransBusy(true)
    setTransErr('')
    api
      .translateText(t, lang)
      .then((r) => {
        setTranslated(r)
        setTranslatedFrom(t)
      })
      .catch((e) => {
        setTranslated('')
        setTranslatedFrom('')
        setTransErr(String(e))
      })
      .finally(() => setTransBusy(false))
  }

  useEffect(() => {
    // 配置里存的目标语言/供应商：窗口打开时读一次
    void api.settingsGet().then((r) => {
      const t = r.settings.translate
      if (t?.provider) setProvider(t.provider)
      if (t?.target) setTarget(t.target)
    })

    run.current = (dataUrl: string) => {
      setImg(dataUrl)
      setText('')
      setTranslated('')
      setTranslatedFrom('')
      setErr('')
      setTransErr('')
      setPreview(false)
      setBusy(true)
      void api
        .imageOcr(dataUrl)
        .then((r) => {
          const t = (r.text ?? '').trim()
          setText(t)
          if (!t) {
            setErr('这张图里没认出文字')
            return
          }
          // 识别完直接翻：用户点开这个窗口多半就是想看懂它
          doTranslate.current(t, targetRef.current)
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
    // target 在这里只是初始值，后续变化走独立的 effect
    // eslint-disable-next-line react-hooks/exhaustive-deps
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

  /** 换语言：存下来 + 立刻重译 */
  const changeTarget = (lang: string) => {
    setTarget(lang)
    void api.settingsPatch({ translate: { target: lang } }).catch(() => {})
    if (text.trim()) doTranslate.current(text, lang)
  }

  // Esc 关闭，Ctrl+C（没选中文本时）复制原文
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

  const stale = !!translated && translatedFrom !== text.trim()
  const chars = text.replace(/\s/g, '').length

  return (
    <div className="ocr-root">
      <header>
        <span className="ocr-title">取字</span>
        {busy && <span className="ocr-hint">识别中…</span>}
        {!busy && text && <span className="ocr-hint">{chars} 字</span>}
        <div style={{ flex: 1 }} />
        {img && (
          <button className="ocr-btn ghost" onClick={() => setPreview((v) => !v)}>
            {preview ? '收起原图' : '看原图'}
          </button>
        )}
        <select
          className="ocr-select"
          value={target}
          title="翻译成什么语言"
          onChange={(e) => changeTarget(e.target.value)}
        >
          {LANGUAGES.map((l) => (
            <option key={l.id} value={l.id}>
              {l.label}
            </option>
          ))}
        </select>
        <span className="ocr-provider" title="在「设置 → 翻译服务」里更换">
          {providerLabel(provider)}
        </span>
      </header>

      {preview && img && (
        <div className="ocr-preview">
          <img src={img} alt="" />
        </div>
      )}

      <div className="ocr-split">
        <section className="ocr-pane">
          <div className="ocr-pane-head">
            <span>原文</span>
            {stale && <em className="ocr-stale">原文改过</em>}
            <div style={{ flex: 1 }} />
            <button className="ocr-btn tiny" onClick={() => void copy('text')} disabled={!text.trim()}>
              {copied === 'text' ? '已复制' : '复制'}
            </button>
          </div>
          <textarea
            ref={areaRef}
            className="ocr-text"
            value={text}
            spellCheck={false}
            placeholder={busy ? '' : '框选之后识别出来的文字会出现在这里，可以直接改'}
            onChange={(e) => setText(e.target.value)}
          />
        </section>

        <section className="ocr-pane">
          <div className="ocr-pane-head">
            <span>译文</span>
            <div style={{ flex: 1 }} />
            <button
              className="ocr-btn tiny"
              onClick={() => doTranslate.current(text, target)}
              disabled={!text.trim() || transBusy}
            >
              {transBusy ? '翻译中…' : stale ? '重译' : '重新翻译'}
            </button>
            <button
              className="ocr-btn tiny"
              onClick={() => void copy('translated')}
              disabled={!translated.trim()}
            >
              {copied === 'translated' ? '已复制' : '复制'}
            </button>
          </div>
          <div className="ocr-trans-body">
            {transBusy && !translated ? (
              <span className="ocr-placeholder">翻译中…</span>
            ) : translated ? (
              translated
            ) : (
              <span className="ocr-placeholder">
                {text.trim() ? '点「重新翻译」' : '还没有内容'}
              </span>
            )}
          </div>
        </section>
      </div>

      {(err || transErr) && (
        <div className="ocr-err">
          {err || transErr}
          {transErr && (
            <button
              className="ocr-btn tiny"
              style={{ marginLeft: 8 }}
              onClick={() => void api.showMain('settings')}
            >
              去设置
            </button>
          )}
        </div>
      )}

      <footer>
        <button
          className="ocr-btn primary"
          onClick={() => void copy('text')}
          disabled={!text.trim()}
        >
          {copied === 'text' ? '已复制' : '复制原文'}
        </button>
        <button
          className="ocr-btn"
          onClick={() => void copy('translated')}
          disabled={!translated.trim()}
        >
          {copied === 'translated' ? '已复制' : '复制译文'}
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
