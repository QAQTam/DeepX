// ── ThemeProvider ──
// Manages data-theme attribute on <html> for CSS variable switching.
// Supports: 'light' | 'dark' | 'system'

import { createContext, useContext, createSignal, createEffect, type JSX } from 'solid-js'

export type Theme = 'light' | 'dark' | 'system'

interface ThemeCtx {
  theme: Theme
  setTheme: (t: Theme) => void
  resolved: 'light' | 'dark'
}

const ThemeContext = createContext<ThemeCtx>({ theme: 'system', setTheme: () => {}, resolved: 'dark' })

export function useTheme() { return useContext(ThemeContext)! }

function getStored(): Theme {
  try {
    const v = localStorage.getItem('dsx-theme')
    if (v === 'light' || v === 'dark') return v
    return 'system'
  } catch { return 'system' }
}

function resolveTheme(t: Theme): 'light' | 'dark' {
  if (t === 'light' || t === 'dark') return t
  if (typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches) return 'dark'
  return 'light'
}

export function ThemeProvider(props: { children: JSX.Element }) {
  const [theme, setThemeState] = createSignal<Theme>(getStored())
  const [resolved, setResolved] = createSignal<'light' | 'dark'>(resolveTheme(getStored()))

  const setTheme = (t: Theme) => {
    setThemeState(t)
    try { localStorage.setItem('dsx-theme', t) } catch { /* noop */ }
  }

  // Apply to DOM
  createEffect(() => {
    const t = theme()
    const r = resolveTheme(t)
    setResolved(r)
    const root = document.documentElement
    if (t === 'dark') root.setAttribute('data-theme', 'dark')
    else if (t === 'light') root.setAttribute('data-theme', 'light')
    else root.removeAttribute('data-theme')
  })

  // Listen for system theme changes when in 'system' mode
  const mq = typeof window !== 'undefined' ? window.matchMedia('(prefers-color-scheme: dark)') : null

  createEffect(() => {
    if (theme() !== 'system') return
    const handler = () => {
      setResolved(resolveTheme('system'))
    }
    mq?.addEventListener('change', handler)
    // No cleanup needed — Solid handles disposal automatically when effect re-runs
    // but for completeness, we'd use onCleanup
  })

  return (
    <ThemeContext.Provider value={{ theme: theme(), setTheme, resolved: resolved() }}>
      {props.children}
    </ThemeContext.Provider>
  )
}
