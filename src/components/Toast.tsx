import { createContext, useCallback, useContext, useMemo, useRef, useState } from 'react'

type ToastKind = 'info' | 'success' | 'error'

interface ToastItem {
  id: number
  message: string
  kind: ToastKind
}

interface ToastApi {
  push: (message: string, kind?: ToastKind) => void
}

const ToastContext = createContext<ToastApi>({ push: () => {} })

export const useToast = (): ToastApi => useContext(ToastContext)

export function ToastProvider({ children }: { children: React.ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([])
  const seq = useRef(0)

  const push = useCallback((message: string, kind: ToastKind = 'info') => {
    const id = ++seq.current
    setItems((prev) => [...prev, { id, message, kind }])
    // 长消息多留一会儿，不然没看完就没了
    const ttl = Math.min(6000, 2200 + message.length * 40)
    setTimeout(() => {
      setItems((prev) => prev.filter((i) => i.id !== id))
    }, ttl)
  }, [])

  const api = useMemo(() => ({ push }), [push])

  return (
    <ToastContext.Provider value={api}>
      {children}
      <div className="toasts">
        {items.map((i) => (
          <div key={i.id} className={`toast ${i.kind}`}>
            {i.message}
          </div>
        ))}
      </div>
    </ToastContext.Provider>
  )
}
