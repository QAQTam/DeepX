import { useState } from 'preact/compat'
import { useQuery } from '@tanstack/react-query'
import { invoke } from '@tauri-apps/api/core'
import { T } from '../i18n'

interface WorkspacePanelProps {
  currentPhase: string
  planVersion: number
}

export function WorkspacePanel({ currentPhase, planVersion }: WorkspacePanelProps) {
  const [selectedPlan, setSelectedPlan] = useState<string | null>(null)

  const { data: plans = [] } = useQuery({
    queryKey: ['plans', planVersion],
    queryFn: () => invoke<any[]>('list_plans'),
  })

  const { data: planContent = '' } = useQuery({
    queryKey: ['plan', selectedPlan],
    queryFn: () => invoke<string>('read_plan', { name: selectedPlan! }),
    enabled: !!selectedPlan,
  })

  const { data: workspacePath, refetch: refetchWorkspace } = useQuery({
    queryKey: ['workspace'],
    queryFn: () => invoke<string>('get_workspace'),
  })

  const { data: dirEntries = null, refetch: refreshDir } = useQuery({
    queryKey: ['directory', workspacePath],
    queryFn: () => invoke<any>('scan_directory', { path: workspacePath! }).then(r => r.entries as { name: string; is_dir: boolean; size: number }[]),
    enabled: !!workspacePath,
  })

  const pickFolder = async () => {
    try {
      const { open } = await import('@tauri-apps/plugin-dialog')
      const selected = await open({ directory: true, multiple: false, title: '选择工作区目录' })
      if (selected) {
        invoke('set_workspace', { path: selected }).catch(() => {})
        refetchWorkspace().then(() => refreshDir())
      }
    } catch { /* dialog cancelled */ }
  }

  const statusColor = (s: string) =>
    s === 'done' ? 'var(--success)' : s === 'active' ? 'var(--accent)' : s === 'cancelled' ? 'var(--error)' : 'var(--muted)'

  return (
    <div className="h-full flex flex-col text-xs overflow-y-auto">
      <div className="p-3 border-b border-[var(--border)]">
        <div className="font-bold text-[var(--text-h)] text-sm mb-2">{T.workspace}</div>
        <div className="text-xs flex items-center gap-1.5">
          <span className="text-[var(--muted)]">阶段:</span>
          <span className={`px-1.5 py-0.5 rounded text-xs font-medium border ${
            currentPhase === 'plan' ? 'text-[var(--warning)] border-[var(--warning)]' :
            currentPhase === 'coding' ? 'text-[var(--text-h)] border-[var(--border)]' :
            currentPhase === 'debug' ? 'text-[var(--error)] border-[var(--error)]' :
            'text-[var(--accent)] border-[var(--accent)]'
          }`}>
            {currentPhase === 'plan' ? 'Plan' :
             currentPhase === 'coding' ? 'Code' :
             currentPhase === 'debug' ? 'Debug' : currentPhase}
          </span>
        </div>
      </div>

      <div className="p-3 border-b border-[var(--border)]">
        <div className="font-medium text-[var(--text-h)] text-xs mb-2">计划</div>
        {plans.length === 0 ? (
          <div className="text-xs text-[var(--muted)]">暂无计划</div>
        ) : (
          <div className="space-y-1 max-h-40 overflow-y-auto">
            {plans.map((p, i) => (
              <div key={i} onClick={() => setSelectedPlan(selectedPlan === p.name ? null : p.name)}
                className={`px-2 py-1 rounded-md cursor-pointer text-xs transition-colors ${
                  selectedPlan === p.name ? 'bg-[var(--accent-light)]' : 'hover:bg-[var(--bg-hover)]'
                }`}>
                <div className="flex items-center justify-between">
                  <span className="truncate text-[var(--text-h)]">{p.name}</span>
                  <span className="text-[11px]" style={{ color: statusColor(p.status) }}>{p.status}</span>
                </div>
                {p.summary && <div className="text-xs text-[var(--muted)] truncate">{p.summary}</div>}
              </div>
            ))}
          </div>
        )}
        {selectedPlan && planContent && (
          <div className="mt-2 p-2 bg-[var(--bg-tertiary)] rounded-lg text-[11px] text-[var(--text)] max-h-40 overflow-y-auto font-mono whitespace-pre-wrap border border-[var(--border)]">
            {planContent}
          </div>
        )}
      </div>

      <div className="p-3 border-t border-[var(--border)]">
        <div className="flex items-center justify-between mb-2">
          <div className="font-medium text-[var(--text-h)] text-xs">工作区</div>
          <button onClick={pickFolder} className="text-[var(--accent)] text-xs px-3 py-1.5 rounded-lg hover:bg-[var(--accent-light)] transition-colors min-h-[32px]">
            {workspacePath ? '切换' : '选择'}
          </button>
        </div>
        {workspacePath ? (
          <>
            <div className="font-mono text-xs text-[var(--muted)] truncate mb-1" title={workspacePath}>{workspacePath}</div>
            <div className="space-y-0.5 max-h-32 overflow-y-auto">
              {dirEntries?.slice(0, 30).map((e, i) => (
                <div key={i} className="flex items-center gap-1 text-[11px] font-mono">
                  <span className={e.is_dir ? 'text-[var(--accent)]' : 'text-[var(--muted)]'}>{e.is_dir ? '📁' : '📄'}</span>
                  <span className="truncate text-[var(--text)]">{e.name}</span>
                </div>
              ))}
            </div>
          </>
        ) : (
          <div className="text-[var(--muted)] text-xs">{T.noProject}</div>
        )}
      </div>
    </div>
  )
}
