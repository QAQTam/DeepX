// ── useDocuments Hook ──
// Document tracking + recent edits from debug_snapshot events.

import { useState, useCallback } from 'react'
import type { DocInfo } from '../types'

interface Task {
  subject: string
  description: string
  status: string
}

interface UseDocumentsReturn {
  documents: DocInfo[]
  recentEdits: string[]
  tasks: Task[]
  updateFromSnapshot: (payload: { documents?: DocInfo[]; recent_edits?: string[]; tasks?: Task[] }) => void
  clear: () => void
}

export function useDocuments(): UseDocumentsReturn {
  const [documents, setDocuments] = useState<DocInfo[]>([])
  const [recentEdits, setRecentEdits] = useState<string[]>([])
  const [tasks, setTasks] = useState<Task[]>([])

  const updateFromSnapshot = useCallback((payload: {
    documents?: DocInfo[]
    recent_edits?: string[]
    tasks?: Task[]
  }) => {
    if (payload.documents) setDocuments(payload.documents)
    if (payload.recent_edits) setRecentEdits(payload.recent_edits)
    if (payload.tasks) setTasks(payload.tasks)
  }, [])

  const clear = useCallback(() => {
    setDocuments([])
    setRecentEdits([])
    setTasks([])
  }, [])

  return { documents, recentEdits, tasks, updateFromSnapshot, clear }
}
