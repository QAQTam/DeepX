// ── useBalance Hook ──

import { useState, useCallback } from 'react'
import { api } from '../bridge/tauri'

interface UseBalanceReturn {
  balance: string
  loading: boolean
  refresh: (apiKey: string) => Promise<void>
  setBalance: (s: string) => void
}

export function useBalance(): UseBalanceReturn {
  const [balance, setBalance] = useState('')
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async (apiKey: string) => {
    if (!apiKey) return
    setLoading(true)
    try {
      const result = await api.getBalance(apiKey)
      const info = result?.balance_infos?.[0]
      if (info) setBalance(`${info.total_balance} ${info.currency}`)
    } catch { /* noop */ }
    finally { setLoading(false) }
  }, [])

  return { balance, loading, refresh, setBalance }
}