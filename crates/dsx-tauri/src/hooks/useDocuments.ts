// ── useDocuments Hook (SolidJS) ──
// Document tracking + recent edits from debug_snapshot events.

import { createSignal } from 'solid-js'
import type { DocInfo } from '../types'

interface Task {
  id: string
  subject: string
  description: string
  status: string
}

interface UseDocumentsReturn {
  readonly documents: DocInfo[]
  readonly recentEdits: string[]
  readonly tasks: Task[]
  updateFromSnapshot: (payload: { documents?: DocInfo[]; recent_edits?: string[]; tasks?: Task[] }) => void
  clear: () => void
}

export function useDocuments(): UseDocumentsReturn {
  const [_documents, setDocuments] = createSignal<DocInfo[]>([])
  const [_recentEdits, setRecentEdits] = createSignal<string[]>([])
  const [_tasks, setTasks] = createSignal<Task[]>([])

  const updateFromSnapshot = (payload: {
    documents?: DocInfo[]
    recent_edits?: string[]
    tasks?: Task[]
  }) => {
    if (payload.documents) setDocuments(payload.documents)
    if (payload.recent_edits) setRecentEdits(payload.recent_edits)
    if (payload.tasks) setTasks(payload.tasks)
  }

  const clear = () => {
    setDocuments([])
    setRecentEdits([])
    setTasks([])
  }

  return {
    get documents() { return _documents() },
    get recentEdits() { return _recentEdits() },
    get tasks() { return _tasks() },
    updateFromSnapshot, clear,
  }
}
