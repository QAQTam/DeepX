// ── InfoPanel ──
// Left sidebar: context usage, KV cache, balance, tool calls, audit log, sessions.

import { createSignal, type Accessor, For } from 'solid-js'
import { tt } from '../i18n'
import { Button, Badge, Card, EmptyState } from './shared'
import { getAllToolLabels } from '../domain/tool-registry'

interface InfoPanelProps {
  tokens: Accessor<{ used: number; limit: number }>
  cache: Accessor<{ hit: number; miss: number }>
  balance: string
  sessionId: Accessor<string>
  sessions: Accessor<SessionMeta[]>
  auditLog: unknown[]
  toolBatch: { names: string[]; startMs: number } | null
  toolNames: string[]
  onSettings: () => void
  onNewSession: () => void
  onResumeSession: (seed: string) => void
  onDeleteAllSessions: () => void
  onDeleteSession: (seed: string) => void
  onRefreshBalance: () => void
}

interface SessionMeta {
  seed: string
  date?: string
  model?: string
  message_count?: number
}

const toolLabels = getAllToolLabels()

export function InfoPanel(props: InfoPanelProps) {
  const [showHistory, setShowHistory] = createSignal(true)
  const usagePct = () => {
    const t = props.tokens()
    return t.limit > 0 ? Math.min(100, (t.used / t.limit) * 100) : 0
  }
  const cacheTotal = () => {
    const c = props.cache()
    return c.hit + c.miss
  }
  const cacheHitPct = () => {
    const c = props.cache()
    const total = cacheTotal()
    return total > 0 ? (c.hit / total) * 100 : 0
  }
  const usageColor = () => usagePct() > 80 ? 'var(--error)' : usagePct() > 60 ? 'var(--warning)' : 'var(--success)'

  return (
    <div class="space-y-3">
      {/* Header */}
      <h2 class="text-sm font-semibold text-[var(--text-h)]">DeepX</h2>

      {/* Context Usage + KV Cache */}
      <Card padding="sm">
        <div class="text-xs text-[var(--muted)] mb-1.5">{tt('info.contextUsage')}</div>
        <div class="flex items-end gap-2">
          <div class="text-lg font-mono font-bold text-[var(--text-h)] leading-none">{props.tokens().used.toLocaleString()}</div>
          <div class="text-xs text-[var(--muted)] mb-0.5">/ {props.tokens().limit.toLocaleString()}</div>
          <div class="flex-1" />
          {cacheTotal() > 0 && (
            <div class="text-xs font-mono text-[var(--success)]">{Math.round(cacheHitPct())}% hit</div>
          )}
        </div>
        <div class="h-1.5 bg-[var(--bg-tertiary)] rounded-full overflow-hidden mt-1.5 flex">
          <div class="h-full rounded-full transition-all duration-500" style={{ width: `${usagePct()}%`, 'background-color': usageColor() }} />
          {cacheTotal() > 0 && (
            <div class="h-full transition-all duration-500" style={{ width: `${Math.min(usagePct() + (cacheHitPct() * (1 - usagePct() / 100)) / 2, 100) - usagePct()}%`, 'background-color': 'var(--success)', opacity: 0.3 }} />
          )}
        </div>
      </Card>

      {/* Balance */}
      <Card padding="sm">
        <div class="flex items-center justify-between">
          <span class="text-xs text-[var(--muted)]">{tt('info.balance')}</span>
          <Button variant="ghost" size="sm" onClick={props.onRefreshBalance}>{tt('common.refresh')}</Button>
        </div>
        <div class="text-sm font-mono font-bold text-[var(--text-h)] mt-0.5">{props.balance || '—'}</div>
      </Card>

      {/* Active Tool */}
      {props.toolNames.length > 0 && (
        <Card padding="sm">
          <div class="text-xs text-[var(--muted)] mb-1">{tt('info.activeTools')}</div>
          <div class="flex flex-wrap gap-1">
            <For each={props.toolNames}>{n => (
              <Badge variant="accent">{toolLabels[n] || n}</Badge>
            )}</For>
          </div>
        </Card>
      )}

      {/* Audit Log */}
      {props.auditLog.length > 0 && (
        <Card padding="sm">
          <div class="text-xs text-[var(--muted)] mb-1">{tt('info.auditLog')} ({props.auditLog.length})</div>
          <div class="max-h-32 overflow-y-auto space-y-1 text-xs font-mono">
            <For each={props.auditLog}>{(entry: any) => (
              <div class={`truncate ${entry.success ? 'text-[var(--text)]' : 'text-[var(--error)]'}`}>
                {entry.name} {entry.args?.slice(0, 40)}
              </div>
            )}</For>
          </div>
        </Card>
      )}

      {/* Session History */}
      <Card padding="sm">
        <button
          onClick={() => setShowHistory(s => !s)}
          class="w-full flex items-center justify-between text-xs text-[var(--muted)] hover:text-[var(--text)] transition-colors"
        >
          <span>{tt('info.sessions')} ({props.sessions().length})</span>
          <span class="text-[11px]">{showHistory() ? '▾' : '▸'}</span>
        </button>
        {showHistory() && (
          <div class="mt-2 space-y-1 max-h-48 overflow-y-auto">
            {props.sessions().length === 0 ? (
              <EmptyState title={tt('info.noSessions')} />
            ) : (
              <For each={props.sessions()}>{s => (
                <div class={`flex items-center justify-between text-xs py-1 px-1.5 rounded ${s.seed === props.sessionId() ? 'bg-[var(--accent)]/10' : ''}`}>
                  <button class="text-left flex-1 truncate text-[var(--text)] hover:text-[var(--accent)]" onClick={() => props.onResumeSession(s.seed)}>
                    <span class="font-mono">{s.seed?.slice(0, 8)}</span>
                    {s.date && <span class="text-[var(--muted)] ml-1">{s.date}</span>}
                    {s.message_count !== undefined && <span class="text-[var(--muted)] ml-1">({s.message_count})</span>}
                  </button>
                  <Button variant="ghost" size="sm" onClick={() => props.onDeleteSession(s.seed)} class="text-[var(--error)] text-xs">
                    ✕
                  </Button>
                </div>
              )}</For>
            )}
          </div>
        )}
        <div class="flex gap-2 mt-2">
          <Button variant="secondary" size="sm" onClick={props.onNewSession} class="flex-1">
            {tt('chat.newSession')}
          </Button>
          <Button variant="ghost" size="sm" onClick={props.onDeleteAllSessions} class="text-[var(--error)]">
            {tt('common.delete')}
          </Button>
        </div>
      </Card>
    </div>
  )
}
