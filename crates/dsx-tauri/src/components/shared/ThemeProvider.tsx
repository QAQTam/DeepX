// ── ThemeProvider ──
// Manages data-theme attribute on <html> for CSS variable switching.
// Supports: 'light' | 'dark' | 'system'

import { createContext, useContext, useEffect, useState, type ReactNode, useCallback } from 'react'

export type Theme = 'light' | 'dark' | 'system'

interface ThemeCtx {
  theme: Theme
  setTheme: (t: Theme) => void
  resolved: 'light' | 'dark'
}

const ThemeContext = createContext<ThemeCtx>({ theme: 'system', setTheme: () => {}, resolved: 'dark' })

export function useTheme() { return useContext(ThemeContext) }

function getStored(): Theme {
  try {
    const v = localStorage.getItem('dsx-theme')
    if (v === 'light' || v === 'dark') return v
    return 'system'
  } catch { return 'system' }
}

function resolveTheme(t: Theme): 'light' | 'dark' {
  if (t === 'light' || t === 'dark') return t
  // system
  if (typeof window !== 'undefined' && window.matchMedia?.('(prefers-color-scheme: dark)').matches) return 'dark'
  return 'light'
}

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setThemeState] = useState<Theme>(getStored)
  const [resolved, setResolved] = useState<'light' | 'dark'>(() => resolveTheme(getStored()))

  const setTheme = useCallback((t: Theme) => {
    setThemeState(t)
    try { localStorage.setItem('dsx-theme', t) } catch { /* noop */ }
  }, [])

  // Apply to DOM
  useEffect(() => {
    const r = resolveTheme(theme)
    setResolved(r)
    const root = document.documentElement
    if (theme === 'dark') root.setAttribute('data-theme', 'dark')
    else if (theme === 'light') root.setAttribute('data-theme', 'light')
    else root.removeAttribute('data-theme')
  }, [theme])

  // Listen for system changes when in 'system' mode
  useEffect(() => {
    if (theme !== 'system') return
    const mq = window.matchMedia('(prefers-color-scheme: dark)')
    const handler = () => { const r = resolveTheme('system'); setResolved(r) }
    mq.addEventListener('change', handler)
    return () => mq.removeEventListener('change', handler)
  }, [theme])

  return (
    <ThemeContext.Provider value={{ theme, setTheme, resolved }}>
      {children}
    </ThemeContext.Provider>
  )
}
