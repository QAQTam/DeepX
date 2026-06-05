// ── WorkspacePanel ──
// Right sidebar: workspace directory browser, document tracking, recent edits, tasks.

import { createSignal, onMount } from 'solid-js'
import { invoke } from '@tauri-apps/api/core'
import type { DocInfo } from '../types'
import { tt } from '../i18n'
import { Button, Badge, Card, EmptyState } from './shared'

interface Task {
  subject: string
  description: string
  status: string
}

interface WorkspacePanelProps {
  documents: DocInfo[]
  recentEdits: string[]
  tasks: Task[]
}

const statusLabel: Record<string, string> = {
  pending: 'workspace.pending',
  in_progress: 'workspace.inProgress',
  completed: 'workspace.completed',
  cancelled: 'workspace.cancelled',
}

const statusColor = (s: string) =>
  s === 'in_progress' ? 'accent' : s === 'completed' ? 'success' : s === 'cancelled' ? 'error' : 'warning'

export function WorkspacePanel(props: WorkspacePanelProps) {
  const [workspacePath, setWorkspacePath] = createSignal<string | null>(null)
  const [dirEntries, setDirEntries] = createSignal<{ name: string; is_dir: boolean; size: number }[] | null>(null)
  const [showTasks, setShowTasks] = createSignal(true)
  const [showDocs, setShowDocs] = createSignal(true)
  const [showEdits, setShowEdits] = createSignal(true)

  onMount(() => {
    invoke<string>('get_workspace').then(p => { setWorkspacePath(p); refreshDir(p) }).catch(() => {})
  })

  const refreshDir = (path: string) => {
    invoke<any>('scan_directory', { path }).then(r => setDirEntries(r.entries)).catch(() => setDirEntries(null))
  }

  const pickFolder = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({ directory: true, multiple: false, title: tt('workspace.selectFolder') })
      if (selected) {
        await invoke('set_workspace', { path: selected })
        setWorkspacePath(selected as string)
        refreshDir(selected as string)
      }
    } catch { /* ignore */ }
  }

  return (
    <div class="space-y-3">
      {/* Header */}
      <div class="flex items-center justify-between">
        <h2 class="text-sm font-semibold text-[var(--text-h)]">{tt('workspace.title')}</h2>
        <Button variant="ghost" size="sm" onClick={pickFolder}>{tt('workspace.select')}</Button>
      </div>

      {/* Current path */}
      <div class="text-xs text-[var(--muted)] font-mono truncate">
        {workspacePath() || tt('workspace.noFolder')}
      </div>

      {/* Directory tree */}
      {dirEntries() && dirEntries()!.length > 0 && (
        <Card padding="sm">
          <div class="max-h-48 overflow-y-auto space-y-0.5 text-xs font-mono">
            {dirEntries()!.map(e => (
              <div class="flex items-center gap-1.5 text-[var(--text)]">
                <span class="text-[var(--muted)]">{e.is_dir ? '📁' : '📄'}</span>
                <span class="truncate flex-1">{e.name}</span>
                {!e.is_dir && <span class="text-[var(--muted)] shrink-0">{formatSize(e.size)}</span>}
              </div>
            ))}
          </div>
        </Card>
      )}

      {/* Tasks */}
      <Card padding="sm">
        <button
          onClick={() => setShowTasks(s => !s)}
          class="w-full flex items-center justify-between text-xs text-[var(--muted)] hover:text-[var(--text)] transition-colors"
        >
          <span>{tt('workspace.tasks')} ({props.tasks.length})</span>
          <span class="text-[10px]">{showTasks() ? '▾' : '▸'}</span>
        </button>
        {showTasks() && (
          <div class="mt-2 space-y-1 max-h-48 overflow-y-auto">
            {props.tasks.length === 0 ? (
              <EmptyState title={tt('workspace.noTasks')} />
            ) : (
              props.tasks.map(t => (
                <div class="flex items-center gap-1.5 text-xs">
                  <Badge variant={statusColor(t.status)}>{tt(statusLabel[t.status] || t.status)}</Badge>
                  <span class="text-[var(--text)] truncate">{t.subject}</span>
                </div>
              ))
            )}
          </div>
        )}
      </Card>

      {/* Documents */}
      <Card padding="sm">
        <button
          onClick={() => setShowDocs(s => !s)}
          class="w-full flex items-center justify-between text-xs text-[var(--muted)] hover:text-[var(--text)] transition-colors"
        >
          <span>{tt('workspace.documents')} ({props.documents.length})</span>
          <span class="text-[10px]">{showDocs() ? '▾' : '▸'}</span>
        </button>
        {showDocs() && (
          <div class="mt-2 space-y-0.5 max-h-48 overflow-y-auto text-xs font-mono">
            {props.documents.length === 0 ? (
              <EmptyState title={tt('workspace.noDocuments')} />
            ) : (
              props.documents.map(d => (
                <div class="text-[var(--text)] truncate">
                  <span class="text-[var(--muted)]">{d.path}</span>
                </div>
              ))
            )}
          </div>
        )}
      </Card>

      {/* Recent Edits */}
      <Card padding="sm">
        <button
          onClick={() => setShowEdits(s => !s)}
          class="w-full flex items-center justify-between text-xs text-[var(--muted)] hover:text-[var(--text)] transition-colors"
        >
          <span>{tt('workspace.recentEdits')} ({props.recentEdits.length})</span>
          <span class="text-[10px]">{showEdits() ? '▾' : '▸'}</span>
        </button>
        {showEdits() && (
          <div class="mt-2 space-y-0.5 max-h-48 overflow-y-auto text-xs font-mono">
            {props.recentEdits.length === 0 ? (
              <EmptyState title={tt('workspace.noEdits')} />
            ) : (
              props.recentEdits.map(f => (
                <div class="text-[var(--text)] truncate">{f}</div>
              ))
            )}
          </div>
        )}
      </Card>
    </div>
  )
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}K`
  return `${(bytes / 1024 / 1024).toFixed(1)}M`
}
