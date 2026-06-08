// ── useBalance Hook (SolidJS) ──

import { createSignal } from 'solid-js'
import { api } from '../bridge/tauri'

interface UseBalanceReturn {
  readonly balance: string
  readonly loading: boolean
  refresh: (apiKey: string) => Promise<void>
  setBalance: (s: string) => void
}

export function useBalance(): UseBalanceReturn {
  const [_balance, setBalance] = createSignal('')
  const [_loading, setLoading] = createSignal(false)

  const refresh = async (apiKey: string) => {
    if (!apiKey) return
    setLoading(true)
    try {
      const result = await api.getBalance(apiKey)
      const info = result?.balance_infos?.[0]
      if (info) setBalance(`${info.total_balance} ${info.currency}`)
    } catch (e) {
      console.error('getBalance failed:', e)
      throw e
    }
    finally { setLoading(false) }
  }

  return {
    get balance() { return _balance() },
    get loading() { return _loading() },
    refresh, setBalance,
  }
}
