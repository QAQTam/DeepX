// ── useSession Hook (SolidJS) ──
// Session list, delete, resume.

import { createSignal } from 'solid-js'
import { api, type SessionInfo } from '../bridge/tauri'

interface UseSessionReturn {
  readonly sessions: SessionInfo[]
  readonly loading: boolean
  refresh: () => Promise<void>
  deleteSession: (seed: string) => Promise<void>
  deleteAll: () => Promise<void>
  loadMessages: (seed: string) => Promise<unknown[]>
}

export function useSession(): UseSessionReturn {
  const [_sessions, setSessions] = createSignal<SessionInfo[]>([])
  const [_loading, setLoading] = createSignal(false)

  const refresh = async () => {
    setLoading(true)
    try {
      const list = await api.cmdSessions()
      setSessions(list)
    } catch { /* ignore */ }
    finally { setLoading(false) }
  }

  const deleteSession = async (seed: string) => {
    await api.deleteSession(seed)
    await refresh()
  }

  const deleteAll = async () => {
    await api.deleteAllSessions()
    setSessions([])
  }

  const loadMessages = async (seed: string) => {
    const res = await api.loadSessionMessages(seed)
    return Array.isArray(res) ? res : (res as any).messages ?? []
  }

  return {
    get sessions() { return _sessions() },
    get loading() { return _loading() },
    refresh, deleteSession, deleteAll, loadMessages,
  }
}
