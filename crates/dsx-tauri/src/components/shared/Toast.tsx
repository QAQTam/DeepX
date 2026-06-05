// ── Toast Notification System ──
// Lightweight toast queue with auto-dismiss.

import { createContext, useContext, createSignal, type JSX } from 'solid-js'

export interface Toast {
  id: number
  message: string
  type: 'info' | 'success' | 'warning' | 'error'
}

interface ToastCtxValue {
  toasts: Toast[]
  addToast: (message: string, type?: Toast['type']) => void
}

const ToastCtx = createContext<ToastCtxValue>({ toasts: [], addToast: () => {} })
export const useToast = () => useContext(ToastCtx)!

let nextId = 0

export function ToastProvider(props: { children: JSX.Element }) {
  const [toasts, setToasts] = createSignal<Toast[]>([])

  const addToast = (message: string, type: Toast['type'] = 'info') => {
    const id = nextId++
    setToasts(prev => [...prev, { id, message, type }])
    setTimeout(() => {
      setToasts(prev => prev.filter(t => t.id !== id))
    }, 4000)
  }

  return (
    <ToastCtx.Provider value={{ toasts: toasts(), addToast }}>
      {props.children}
      {/* Toast container */}
      <div class="fixed bottom-4 right-4 z-[100] flex flex-col gap-2 pointer-events-none" aria-live="polite">
        {toasts().map(t => (
          <div
            class={`pointer-events-auto px-4 py-2.5 rounded-lg shadow-lg text-sm anim-msg-in max-w-sm
              ${t.type === 'error' ? 'bg-[var(--error)] text-white'
              : t.type === 'success' ? 'bg-[var(--success)] text-white'
              : t.type === 'warning' ? 'bg-[var(--warning)] text-white'
              : 'bg-[var(--text-h)] text-[var(--bg-primary)]'}`}
            role="status"
          >
            {t.message}
          </div>
        ))}
      </div>
    </ToastCtx.Provider>
  )
}
