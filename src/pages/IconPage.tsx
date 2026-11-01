import { useCallback, useEffect, useRef, useState } from 'react'
import { open, save } from '@tauri-apps/plugin-dialog'
import { getCurrentWebview } from '@tauri-apps/api/webview'
import { api } from '../lib/api'
import type { IconBundle } from '../lib/types'
import { IconApp, IconCopy, IconLoader, IconSave } from '../components/Icons'
import { useToast } from '../components/Toast'

/** 提取后能识别的全部落盘类型 */
const PICK_FILTERS = [
  { name: '可提取图标的文件', extensions: ['exe', 'dll', 'lnk', 'apk', 'ipa', 'ico'] },
  { name: '应用程序', extensions: ['exe', 'dll', 'ocx', 'cpl', 'scr'] },
  { name: '快捷方式', extensions: ['lnk'] },
  { name: 'Android / iOS 安装包', extensions: ['apk', 'aab', 'ipa'] },
  { name: '图标与图片', extensions: ['ico', 'png', 'jpg', 'jpeg', 'webp', 'bmp'] },
  { name: '全部文件', extensions: ['*'] }
]

const KIND_LABEL: Record<string, string> = {
  exe: 'Windows 程序',
  dll: '动态库',
  lnk: '快捷方式',
  apk: 'Android 应用',
  ipa: 'iOS 应用',
  ico: 'ICO 文件',
  image: '图片'
}

function kb(n: number): string {
  if (n >= 1024 * 1024) return `${(n / 1048576).toFixed(1)} MB`
  return `${(n / 1024).toFixed(0)} KB`
}

function fileName(p: string): string {
  return p.split(/[\\/]/).pop() ?? p
}

/**
 * 图标提取：从 exe / dll / 快捷方式 / APK / IPA 里取出原始图标。
 *
 * 这里的核心是"拿到全部尺寸"而不是"拿到一个图标" ——
 * 用户可能是要给自己的软件做图标（需要 16/32/48/256 全套），
 * 也可能是想复刻某个 App 的图标（需要最清晰的那张）。两种诉求都满足。
 */
export default function IconPage() {
  const [bundle, setBundle] = useState<IconBundle | null>(null)
  const [busy, setBusy] = useState(false)
  const [over, setOver] = useState(false)
  /** 选中的图标下标；空集合 = 没选（此时操作"最大的那个"） */
  const [sel, setSel] = useState<Set<number>>(new Set())
  const [recent, setRecent] = useState<string[]>([])
  const toast = useToast()
  /** 拖放事件回调里要读最新状态，但又不能重新订阅，用 ref 兜住 */
  const busyRef = useRef(false)
  busyRef.current = busy

  const run = useCallback(
    async (path: string) => {
      if (busyRef.current) return
      setBusy(true)
      setSel(new Set())
      try {
        const result = await api.iconExtract(path)
        setBundle(result)
        // 默认选中最大的那个：多数人只想要它
        setSel(new Set([0]))
        setRecent((prev) => [path, ...prev.filter((p) => p !== path)].slice(0, 8))
        const note = result.notes[result.notes.length - 1]
        if (note) toast.push(`${result.name}：${note}`, 'success')
      } catch (e) {
        setBundle(null)
        toast.push(String(e), 'error')
      } finally {
        setBusy(false)
      }
    },
    [toast]
  )

  // 拖放：Tauri 给的是**磁盘路径**（不是 File 内容），正好是后端要的
  useEffect(() => {
    let alive = true
    let unlisten: (() => void) | undefined
    void getCurrentWebview()
      .onDragDropEvent((event) => {
        if (!alive) return
        const p = event.payload
        if (p.type === 'over' || p.type === 'enter') {
          setOver(true)
        } else if (p.type === 'drop') {
          setOver(false)
          const path = p.paths?.[0]
          if (path) void run(path)
        } else {
          setOver(false)
        }
      })
      .then((fn) => {
        if (alive) unlisten = fn
        else fn()
      })
      .catch(() => {
        // 某些环境不支持拖放事件：不影响"点击选择"这条主路径
      })
    return () => {
      alive = false
      unlisten?.()
    }
  }, [run])

  const pick = async () => {
    const picked = await open({ multiple: false, filters: PICK_FILTERS })
    if (typeof picked === 'string') await run(picked)
  }

  const toggle = (i: number) => {
    setSel((prev) => {
      const next = new Set(prev)
      if (next.has(i)) next.delete(i)
      else next.add(i)
      return next
    })
  }

  /** 要操作的下标：没勾选就取最大的那个 */
  const targets = (): number[] => {
    if (!bundle) return []
    const list = sel.size > 0 ? [...sel] : [0]
    return list.filter((i) => i >= 0 && i < bundle.icons.length).sort((a, b) => a - b)
  }

  const saveOne = async (i: number) => {
    if (!bundle) return
    const icon = bundle.icons[i]
    try {
      await api.imageSaveAs(icon.dataUrl, `${bundle.name}_${icon.width}x${icon.height}.png`)
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const saveAll = async () => {
    if (!bundle) return
    const list = targets()
    setBusy(true)
    let saved = 0
    let last = ''
    try {
      for (const i of list) {
        const icon = bundle.icons[i]
        const r = await api.imageSave(
          icon.dataUrl,
          `${bundle.name}_${icon.width}x${icon.height}.png`,
          false
        )
        last = r.path
        saved += 1
      }
      toast.push(`已保存 ${saved} 个 PNG 到图片目录`, 'success')
      if (last) await api.openImageFolder(last)
    } catch (e) {
      toast.push(String(e), 'error')
    } finally {
      setBusy(false)
    }
  }

  const exportIco = async () => {
    if (!bundle) return
    // ICO 单帧上限 256×256，超出的挑出来提前告知，别等后端报错
    const list = targets().filter((i) => {
      const ic = bundle.icons[i]
      return ic.width <= 256 && ic.height <= 256
    })
    if (list.length === 0) {
      toast.push('选中的图标都大于 256×256，ICO 格式不支持。请选小尺寸的，或先保存成 PNG', 'error')
      return
    }
    const out = await save({
      defaultPath: `${bundle.name}.ico`,
      filters: [{ name: 'Windows 图标', extensions: ['ico'] }]
    })
    if (typeof out !== 'string') return
    try {
      await api.iconBuildIco(
        list.map((i) => bundle.icons[i]),
        out
      )
      toast.push(`已导出 ${list.length} 帧 ICO：${fileName(out)}`, 'success')
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const copyLargest = async () => {
    if (!bundle) return
    const list = targets()
    const icon = bundle.icons[list[list.length - 1] ?? 0]
    if (!icon) return
    try {
      await api.imageCopy(icon.dataUrl)
      toast.push(`已复制 ${icon.width}×${icon.height} 到剪贴板`, 'success')
    } catch (e) {
      toast.push(String(e), 'error')
    }
  }

  const allSelected = !!bundle && sel.size === bundle.icons.length

  return (
    <>
      <div className="page-head">
        <h1>图标提取</h1>
        <div className="spacer" />
        {bundle && (
          <>
            <button className="btn sm" onClick={() => void copyLargest()} title="复制最大的那个到剪贴板">
              <IconCopy size={14} /> 复制
            </button>
            <button className="btn sm" onClick={() => void exportIco()} title="把选中的打包成 .ico">
              .ico
            </button>
            <button className="btn sm primary" onClick={() => void saveAll()} disabled={busy}>
              {busy ? <IconLoader /> : <IconSave size={14} />} 保存 PNG
            </button>
          </>
        )}
        <button className="btn sm" onClick={() => void pick()} disabled={busy}>
          {busy ? <IconLoader /> : <IconApp size={14} />} 选择文件
        </button>
      </div>

      <div className="page-body">
        {!bundle ? (
          <div
            className={`icon-drop ${over ? 'over' : ''}`}
            onClick={() => void pick()}
            onDragOver={(e) => {
              e.preventDefault()
              setOver(true)
            }}
            onDragLeave={() => setOver(false)}
            onDrop={(e) => {
              e.preventDefault()
              setOver(false)
            }}
          >
            <IconApp size={38} />
            <div className="icon-drop-title">
              {busy ? '正在提取…' : '拖入文件，或点击选择'}
            </div>
            <div className="icon-drop-sub">
              exe · dll · 快捷方式 · APK · IPA · ICO
            </div>
            <div className="icon-drop-hint">
              会取出文件里<strong>全部尺寸</strong>的原始图标，不是系统显示的那一张缩略图
            </div>
          </div>
        ) : (
          <>
            <div className="icon-meta">
              <div className="icon-meta-main">
                <strong>{bundle.name}</strong>
                <span className="icon-kind">{KIND_LABEL[bundle.kind] ?? bundle.kind}</span>
                <span className="icon-count">{bundle.icons.length} 个尺寸</span>
              </div>
              <div className="icon-path" title={bundle.path}>
                {bundle.path}
              </div>
              {bundle.notes.length > 0 && (
                <ul className="icon-notes">
                  {bundle.notes.map((n, i) => (
                    <li key={i}>{n}</li>
                  ))}
                </ul>
              )}
            </div>

            <div className="icon-toolbar">
              <button
                className="btn sm"
                onClick={() =>
                  setSel(allSelected ? new Set() : new Set(bundle.icons.map((_, i) => i)))
                }
              >
                {allSelected ? '取消全选' : '全选'}
              </button>
              <span className="icon-tip">
                点图标可多选；不选时「保存」只存最大的那个
              </span>
            </div>

            <div className="icon-grid">
              {bundle.icons.map((icon, i) => (
                <div
                  key={`${icon.width}x${icon.height}-${i}`}
                  className={`icon-card ${sel.has(i) ? 'sel' : ''}`}
                  onClick={() => toggle(i)}
                  title={`${icon.source}\n${kb(icon.bytes)}`}
                >
                  <div className="icon-stage">
                    <img src={icon.dataUrl} alt={`${icon.width}×${icon.height}`} />
                  </div>
                  <div className="icon-caption">
                    <strong>
                      {icon.width}×{icon.height}
                    </strong>
                    <span>{kb(icon.bytes)}</span>
                  </div>
                  <button
                    className="icon-card-save"
                    title="另存为 PNG"
                    onClick={(e) => {
                      e.stopPropagation()
                      void saveOne(i)
                    }}
                  >
                    <IconSave size={13} />
                  </button>
                </div>
              ))}
            </div>

            {recent.length > 1 && (
              <div className="icon-recent">
                <div className="icon-recent-title">本次会话提取过</div>
                {recent.map((p) => (
                  <button key={p} className="icon-recent-item" onClick={() => void run(p)}>
                    {fileName(p)}
                  </button>
                ))}
              </div>
            )}
          </>
        )}
      </div>
    </>
  )
}
