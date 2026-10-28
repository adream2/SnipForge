import { useEffect, useRef, useState } from 'react'

/**
 * 快捷键输入。
 *
 * 以前这里是个普通文本框，用户得手打 `Ctrl+Alt+R` 这种字符串 ——
 * 大小写打错、写成 "Ctrl-Alt-R"、只打一个字母都会让注册失败，
 * 而且每敲一个字符就保存一次（"Ctrl+" 这种中间态也会被写进配置）。
 *
 * 现在改成"点一下，然后按下你想用的组合键"，值和系统的认知天然一致。
 */

/** 存的是 tauri 认的 Super，显示的得是用户认的 Win */
const DISPLAY: Record<string, string> = {
  Super: 'Win',
  Control: 'Ctrl',
  Meta: 'Win',
  CommandOrControl: 'Ctrl'
}

/** 把 accelerator 拆成片段：Ctrl+Alt+R → ['Ctrl','Alt','R'] */
export function splitAccel(accel: string): string[] {
  return accel
    .split('+')
    .map((s) => s.trim())
    .filter(Boolean)
    .map((p) => DISPLAY[p] ?? p)
}

/** 主键的友好名字：KeyR → R、Digit1 → 1、其它保持原样（Space / F9 / ArrowUp） */
function mainKeyName(e: KeyboardEvent): string {
  const code = e.code || ''
  if (/^Key[A-Z]$/.test(code)) return code.slice(3)
  if (/^Digit\d$/.test(code)) return code.slice(5)
  if (/^Numpad\d$/.test(code)) return code
  if (/^F\d{1,2}$/.test(code)) return code
  // 没有 code（合成/注入事件）时退回 e.key，可能是小写，统一成大写好看
  const fallback = code || e.key
  if (/^[a-z]$/.test(fallback)) return fallback.toUpperCase()
  return fallback
}

const MODS = ['Control', 'Alt', 'Shift', 'Meta']

export function HotkeyInput({
  value,
  onChange,
  ok,
  hint
}: {
  value: string
  onChange: (v: string) => void
  ok?: boolean
  hint?: string
}) {
  const [capturing, setCapturing] = useState(false)
  /** 捕获过程中只按了修饰键时的实时预览 */
  const [pending, setPending] = useState<string[]>([])
  const btnRef = useRef<HTMLButtonElement>(null)

  useEffect(() => {
    if (!capturing) return

    const onKey = (e: KeyboardEvent) => {
      // 捕获期间吞掉一切按键，否则 Ctrl+W 会关窗口、空格会滚页面
      e.preventDefault()
      e.stopPropagation()

      const mods: string[] = []
      if (e.ctrlKey) mods.push('Ctrl')
      if (e.altKey) mods.push('Alt')
      if (e.shiftKey) mods.push('Shift')
      if (e.metaKey) mods.push('Super')

      // 只按了修饰键：先显示预览，等真正的键
      if (MODS.includes(e.key)) {
        setPending(mods)
        return
      }
      if (e.key === 'Escape') {
        setCapturing(false)
        setPending([])
        return
      }
      // Delete / Backspace = 清空（这个动作不要快捷键）
      if (e.key === 'Backspace' || e.key === 'Delete') {
        onChange('')
        setCapturing(false)
        setPending([])
        return
      }

      const main = mainKeyName(e)
      if (!main) return
      onChange([...mods, main].join('+'))
      setCapturing(false)
      setPending([])
    }

    // 用捕获阶段，抢在 webview 的默认行为之前
    window.addEventListener('keydown', onKey, true)
    return () => window.removeEventListener('keydown', onKey, true)
  }, [capturing, onChange])

  const parts = capturing ? pending : splitAccel(value)

  return (
    <button
      ref={btnRef}
      className={`hotkey-box ${capturing ? 'capturing' : ''} ${ok === false ? 'fail' : ''}`}
      title={hint ?? '点击后按下组合键；Esc 取消，Delete 清空'}
      onClick={() => {
        setPending([])
        setCapturing((c) => !c)
      }}
      onBlur={() => {
        // 点到别处就别一直挂在捕获态
        setCapturing(false)
        setPending([])
      }}
    >
      {capturing ? (
        parts.length > 0 ? (
          <>
            {parts.map((p) => (
              <kbd key={p}>{p}</kbd>
            ))}
            <span className="hotkey-tip">再按一个键…</span>
          </>
        ) : (
          <span className="hotkey-tip">按下组合键…（Esc 取消）</span>
        )
      ) : parts.length > 0 ? (
        parts.map((p) => <kbd key={p}>{p}</kbd>)
      ) : (
        <span className="hotkey-empty">未设置</span>
      )}
    </button>
  )
}
