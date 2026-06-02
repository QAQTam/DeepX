import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'
import { T } from '../i18n'
import type { DocInfo } from '../types'

interface WorkspacePanelProps {
  documents: DocInfo[]
  recentEdits: string[]
  tasks: Task[]
}

interface Task {
  subject: string
  description: string
  status: string
}

const statusLabel: Record<string, string> = {
  pending: '待办',
  in_progress: '进行中',
  completed: '已完成',
  cancelled: '已取消',
}

const taskStatusColor = (s: string) =>
  s === 'in_progress' ? 'var(--accent)' : s === 'completed' ? 'var(--success)' : s === 'cancelled' ? 'var(--error)' : 'var(--warning)'

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
      const selected = await open({ directory: true, multiple: false, title: '选择工作区目录' })
      if (selected) {
        setWorkspacePath(selected)
        invoke('set_workspace', { path: selected }).catch(() => {})
        refreshDir(selected)
      }
    } catch { /* dialog cancelled */ }
  }

  return (
    <div className="h-full flex flex-col text-base overflow-y-auto">
      <div className="p-3 border-b border-[var(--border)]">
        <div className="font-bold text-[var(--text-h)] text-base mb-2">{T.workspace}</div>
      </div>

      {/* ── Documents ── */}
      {documents.length > 0 && (
        <div className="p-3 border-b border-[var(--border)]">
          <button onClick={() => setShowDocs(v => !v)}
            className="w-full flex items-center justify-between font-medium text-[var(--text-h)] text-base mb-2">
            <span>文档追踪 ({documents.length})</span>
            <span className="text-[14px] text-[var(--muted)]">{showDocs ? '▾' : '▸'}</span>
          </button>
          {showDocs && (
            <div className="space-y-1 max-h-48 overflow-y-auto">
              {documents.map((doc, i) => (
                <div key={i} className="px-2 py-1 rounded-md bg-[var(--bg-tertiary)] border border-[var(--border)]">
                  <div className="flex items-center justify-between gap-1">
                    <span className="font-mono text-[13px]" style={{ color: doc.is_stale ? 'var(--error)' : 'var(--success)' }}>
                      {doc.tag}
                    </span>
                    <span className="text-[14px] text-[var(--muted)] ml-1">
                      {doc.turns_since_read}轮
                    </span>
                  </div>
                  <div className="text-[14px] text-[var(--text)] truncate font-mono mt-0.5">{doc.path}</div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* ── Recent Edits ── */}
      {recentEdits.length > 0 && (
        <div className="p-3 border-b border-[var(--border)]">
          <button onClick={() => setShowEdits(v => !v)}
            className="w-full flex items-center justify-between font-medium text-[var(--text-h)] text-base mb-2">
            <span>最近编辑 ({recentEdits.length})</span>
            <span className="text-[14px] text-[var(--muted)]">{showEdits ? '▾' : '▸'}</span>
          </button>
          {showEdits && (
            <div className="space-y-0.5 max-h-40 overflow-y-auto">
              {recentEdits.map((edit, i) => (
                <div key={i} className="text-[14px] text-[var(--muted)] font-mono truncate px-1 hover:text-[var(--text)]">
                  {edit}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* ── Tasks ── */}
      <div className="p-3 border-b border-[var(--border)]">
        <button onClick={() => setShowTasks(v => !v)}
          className="w-full flex items-center justify-between font-medium text-[var(--text-h)] text-base mb-2">
          <span>任务 ({tasks.length})</span>
          <span className="text-[14px] text-[var(--muted)]">{showTasks ? '▾' : '▸'}</span>
        </button>
        {showTasks && (
          tasks.length === 0 ? (
            <div className="text-[13px] text-[var(--muted)]">暂无任务</div>
          ) : (
            <div className="space-y-1.5 max-h-56 overflow-y-auto">
              {tasks.map((t, i) => (
                <div key={i} className={`px-3 py-2 rounded-lg border transition-all duration-300 ${
                  t.status === 'in_progress' ? 'bg-[var(--accent)]/5 border-[var(--accent)]/30' :
                  t.status === 'completed' ? 'bg-[var(--success)]/5 border-[var(--success)]/30 opacity-80' :
                  t.status === 'cancelled' ? 'bg-[var(--error)]/5 border-[var(--error)]/20 opacity-60 line-through' :
                  'bg-[var(--bg-tertiary)] border-[var(--border)]'
                }`}>
                  <div className="flex items-center gap-2">
                    <span className={`shrink-0 text-sm ${
                      t.status === 'in_progress' ? 'animate-spin' : ''}`}>
                      {t.status === 'completed' ? '✓' :
                       t.status === 'in_progress' ? '⟳' :
                       t.status === 'cancelled' ? '✗' : '○'}
                    </span>
                    <span className="text-[var(--text-h)] text-[15px] font-medium truncate">{t.subject}</span>
                    <span className="text-[13px] px-1.5 py-0.5 rounded-full font-medium shrink-0 ml-auto"
                      style={{ color: taskStatusColor(t.status), background: `${taskStatusColor(t.status)}15` }}>
                      {statusLabel[t.status] || t.status}
                    </span>
                  </div>
                  {t.description && (
                    <div className="text-[14px] text-[var(--muted)] truncate mt-0.5">{t.description}</div>
                  )}
                </div>
              ))}
            </div>
          )
        )}
      </div>

      {/* ── Workspace ── */}
      <div className="p-3 border-t border-[var(--border)]">
        <div className="flex items-center justify-between mb-2">
          <div className="font-medium text-[var(--text-h)] text-base">工作区</div>
          <button onClick={pickFolder} className="text-[var(--accent)] text-[13px] hover:brightness-110">
            {workspacePath ? '切换' : '选择'}
          </button>
        </div>
        {workspacePath ? (
          <>
            <div className="font-mono text-[14px] text-[var(--muted)] truncate mb-1" title={workspacePath}>{workspacePath}</div>
            <div className="space-y-0.5 max-h-32 overflow-y-auto">
              {dirEntries?.slice(0, 30).map((e, i) => (
                <div key={i} className="flex items-center gap-1 text-[14px] font-mono">
                  <span className={e.is_dir ? 'text-[var(--accent)]' : 'text-[var(--muted)]'}>{e.is_dir ? '📁' : '📄'}</span>
                  <span className="truncate text-[var(--text)]">{e.name}</span>
                </div>
              ))}
            </div>
          </>
        ) : (
          <div className="text-[var(--muted)] text-[13px]">{T.noProject}</div>
        )}
      </div>
    </div>
  )
}
