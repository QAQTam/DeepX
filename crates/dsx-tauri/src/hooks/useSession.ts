// ── useSession Hook ──
// Session list, delete, resume.

import { useState, useCallback } from 'react'
import { api, type SessionInfo } from '../bridge/tauri'

interface UseSessionReturn {
  sessions: SessionInfo[]
  loading: boolean
  refresh: () => Promise<void>
  deleteSession: (seed: string) => Promise<void>
  deleteAll: () => Promise<void>
  loadMessages: (seed: string) => Promise<unknown[]>
}

export function useSession(): UseSessionReturn {
  const [sessions, setSessions] = useState<SessionInfo[]>([])
  const [loading, setLoading] = useState(false)

  const refresh = useCallback(async () => {
    setLoading(true)
    try {
      const list = await api.cmdSessions()
      setSessions(list)
    } catch { /* ignore */ }
    finally { setLoading(false) }
  }, [])

  const deleteSession = useCallback(async (seed: string) => {
    await api.deleteSession(seed)
    await refresh()
  }, [refresh])

  const deleteAll = useCallback(async () => {
    await api.deleteAllSessions()
    setSessions([])
  }, [])

  const loadMessages = useCallback(async (seed: string) => {
    const res = await api.loadSessionMessages(seed)
    return res.messages
  }, [])

  return { sessions, loading, refresh, deleteSession, deleteAll, loadMessages }
}