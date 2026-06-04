// ── WorkspacePanel ──
// Right sidebar: workspace directory browser, document tracking, recent edits, tasks.

import { useState, useEffect } from 'react'
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

export function WorkspacePanel({ documents, recentEdits, tasks }: WorkspacePanelProps) {
  const [workspacePath, setWorkspacePath] = useState<string | null>(null)
  const [dirEntries, setDirEntries] = useState<{ name: string; is_dir: boolean; size: number }[] | null>(null)
  const [showTasks, setShowTasks] = useState(true)
  const [showDocs, setShowDocs] = useState(true)
  const [showEdits, setShowEdits] = useState(true)

  useEffect(() => {
    invoke<string>('get_workspace').then(p => { setWorkspacePath(p); refreshDir(p) }).catch(() => {})
  }, [])

  const refreshDir = (path: string) => {
    invoke<any>('scan_directory', { path }).then(r => setDirEntries(r.entries)).catch(() => setDirEntries(null))
  }

  const pickFolder = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({ directory: true, multiple: false, title: tt('workspace.selectFolder') })
      if (selected && typeof selected === 'string') {
        setWorkspacePath(selected)
        invoke('set_workspace', { path: selected }).catch(() => {})
        refreshDir(selected)
      }
    } catch { /* user cancelled */ }
  }

  return (
    <div className="p-3 space-y-3">
      {/* Header */}
      <div className="flex items-center justify-between">
        <span className="text-xs font-semibold text-[var(--text-h)] uppercase tracking-wide">{tt('workspace.title')}</span>
        <Button variant="ghost" size="sm" onClick={pickFolder} aria-label={tt('workspace.selectFolder')}>
          📂
        </Button>
      </div>

      {workspacePath ? (
        <>
          {/* Directory browser */}
          <Card padding="sm">
            <div className="text-xs text-[var(--muted)] mb-1 truncate font-mono" title={workspacePath}>
              {workspacePath.split(/[/\\]/).pop() || workspacePath}
            </div>
            {dirEntries && dirEntries.length > 0 ? (
              <div className="max-h-32 overflow-y-auto space-y-0.5">
                {dirEntries.slice(0, 30).map((e, i) => (
                  <div key={i} className="flex items-center gap-1.5 text-xs text-[var(--text)] truncate">
                    <span className="text-[10px] shrink-0">{e.is_dir ? '📁' : '·'}</span>
                    <span className="truncate">{e.name}</span>
                    {!e.is_dir && e.size > 0 && (
                      <span className="text-[var(--muted)] text-[10px] shrink-0">{formatSize(e.size)}</span>
                    )}
                  </div>
                ))}
                {dirEntries.length > 30 && <div className="text-[10px] text-[var(--muted)]">+{dirEntries.length - 30} more</div>}
              </div>
            ) : dirEntries ? (
              <EmptyState icon="📂" title={tt('workspace.directoryEmpty')} />
            ) : null}
          </Card>

          {/* Tasks */}
          <Card padding="sm">
            <button onClick={() => setShowTasks(o => !o)} className="w-full flex items-center justify-between text-xs text-[var(--muted)]">
              <span>{tt('workspace.tasks')} ({tasks.length})</span>
              <span className="text-[10px]">{showTasks ? '▾' : '▸'}</span>
            </button>
            {showTasks && (
              <div className="mt-2 space-y-1 max-h-40 overflow-y-auto">
                {tasks.length === 0 ? (
                  <EmptyState icon="📋" title={tt('workspace.noTasks')} />
                ) : (
                  tasks.map((t, i) => (
                    <div key={i} className="flex items-start gap-1.5 text-xs">
                      <span className="text-[10px] mt-0.5 shrink-0">{t.status === 'in_progress' ? '◉' : t.status === 'completed' ? '✓' : '○'}</span>
                      <div className="min-w-0">
                        <div className="text-[var(--text-h)] truncate">{t.subject}</div>
                        <Badge variant={statusColor(t.status)}>{tt(statusLabel[t.status] || t.status)}</Badge>
                      </div>
                    </div>
                  ))
                )}
              </div>
            )}
          </Card>

          {/* Document tracking */}
          <Card padding="sm">
            <button onClick={() => setShowDocs(o => !o)} className="w-full flex items-center justify-between text-xs text-[var(--muted)]">
              <span>{tt('workspace.documents')} ({documents.length})</span>
              <span className="text-[10px]">{showDocs ? '▾' : '▸'}</span>
            </button>
            {showDocs && (
              <div className="mt-2 space-y-1 max-h-32 overflow-y-auto">
                {documents.length === 0 ? (
                  <EmptyState icon="📄" title={tt('workspace.noDocuments')} />
                ) : (
                  documents.map((d, i) => (
                    <div key={i} className="flex items-center justify-between text-xs">
                      <span className={`truncate flex-1 ${d.is_stale ? 'text-[var(--error)]' : 'text-[var(--text)]'}`}>
                        {d.tag ? `${d.tag} ` : ''}{d.path.split(/[/\\]/).pop() || d.path}
                      </span>
                      {d.turns_since_read > 0 && (
                        <span className="text-[var(--muted)] text-[10px] ml-1 shrink-0">
                          -{d.turns_since_read}
                        </span>
                      )}
                    </div>
                  ))
                )}
              </div>
            )}
          </Card>

          {/* Recent edits */}
          <Card padding="sm">
            <button onClick={() => setShowEdits(o => !o)} className="w-full flex items-center justify-between text-xs text-[var(--muted)]">
              <span>{tt('workspace.recentEdits')} ({recentEdits.length})</span>
              <span className="text-[10px]">{showEdits ? '▾' : '▸'}</span>
            </button>
            {showEdits && (
              <div className="mt-2 space-y-0.5 max-h-32 overflow-y-auto">
                {recentEdits.length === 0 ? (
                  <EmptyState icon="✏️" title={tt('workspace.noEdits')} />
                ) : (
                  recentEdits.map((p, i) => (
                    <div key={i} className="text-xs text-[var(--text)] font-mono truncate">
                      {p.split(/[/\\]/).pop() || p}
                    </div>
                  ))
                )}
              </div>
            )}
          </Card>
        </>
      ) : (
        <EmptyState
          icon="📂"
          title={tt('workspace.noProject')}
          description={tt('workspace.selectFolder')}
          action={{ label: tt('workspace.selectFolder'), onClick: pickFolder }}
        />
      )}
    </div>
  )
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}K`
  return `${(bytes / 1024 / 1024).toFixed(1)}M`
}