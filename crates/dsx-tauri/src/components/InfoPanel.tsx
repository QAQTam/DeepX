// ── InfoPanel ──
// Left sidebar: context usage, KV cache, balance, tool calls, audit log, sessions.

import { useState } from 'react'
import { tt } from '../i18n'
import { Button, Badge, Card, EmptyState } from './shared'
import { getAllToolLabels } from '../domain/tool-registry'

interface InfoPanelProps {
  tokens: { used: number; limit: number }
  cache: { hit: number; miss: number }
  balance: string
  sessionId: string
  sessions: SessionMeta[]
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

export function InfoPanel({
  tokens, cache, balance, sessionId, sessions,
  toolBatch, toolNames, auditLog,
  onSettings, onNewSession,
  onResumeSession, onDeleteAllSessions, onDeleteSession, onRefreshBalance,
}: InfoPanelProps) {
  const [showHistory, setShowHistory] = useState(true)
  const usagePct = tokens.limit > 0 ? Math.min(100, (tokens.used / tokens.limit) * 100) : 0
  const cacheTotal = cache.hit + cache.miss
  const cacheHitPct = cacheTotal > 0 ? (cache.hit / cacheTotal) * 100 : 0
  const usageColor = usagePct > 80 ? 'var(--error)' : usagePct > 60 ? 'var(--warning)' : 'var(--success)'

  return (
    <div className="space-y-3">
      {/* Header */}
      <div className="flex items-center justify-between">
        <span className="text-xs font-semibold text-[var(--text-h)] uppercase tracking-wide">{tt('info.title')}</span>
        <Button variant="ghost" size="sm" onClick={onSettings} aria-label={tt('common.settings')}>
          ⚙
        </Button>
      </div>

      {/* Context usage */}
      <Card padding="sm">
        <div className="text-xs text-[var(--muted)] mb-1.5">{tt('info.usage')}</div>
        <div className="flex items-center justify-between text-xs text-[var(--text-h)] font-mono mb-1">
          <span>{tokens.used.toLocaleString()}</span>
          <span>{tokens.limit.toLocaleString()}</span>
        </div>
        <div className="h-2 bg-[var(--bg-tertiary)] rounded-full overflow-hidden">
          <div className="h-full rounded-full transition-all duration-300" style={{ width: `${usagePct}%`, backgroundColor: usageColor }} />
        </div>
      </Card>

      {/* KV Cache */}
      <Card padding="sm">
        <div className="text-xs text-[var(--muted)] mb-1.5">{tt('info.kvCache')}</div>
        <div className="flex gap-2 text-xs font-mono text-[var(--text-h)]">
          <Badge variant="success">{tt('info.hit')} {cache.hit.toLocaleString()}</Badge>
          <Badge variant="default">{tt('info.miss')} {cache.miss.toLocaleString()}</Badge>
        </div>
        {cacheTotal > 0 && (
          <div className="mt-1.5 h-1.5 bg-[var(--bg-tertiary)] rounded-full overflow-hidden flex">
            <div className="h-full bg-[var(--success)]" style={{ width: `${cacheHitPct}%` }} />
            <div className="h-full bg-[var(--text)]/20" style={{ width: `${100 - cacheHitPct}%` }} />
          </div>
        )}
      </Card>

      {/* Balance */}
      <Card padding="sm">
        <div className="flex items-center justify-between mb-1">
          <span className="text-xs text-[var(--muted)]">{tt('info.balance')}</span>
          <Button variant="ghost" size="sm" onClick={onRefreshBalance} aria-label={tt('common.refresh')}>
            ↻
          </Button>
        </div>
        <div className="text-xs font-mono text-[var(--text-h)]">{balance || tt('info.noBalance')}</div>
      </Card>

      {/* Active tool calls */}
      {(toolBatch || toolNames.length > 0) && (
        <Card padding="sm">
          <div className="text-xs text-[var(--muted)] mb-1.5">{tt('info.toolCallTitle')}</div>
          <div className="flex flex-wrap gap-1">
            {toolNames.map((n, i) => (
              <Badge key={i} variant="accent">{toolLabels[n] || n}</Badge>
            ))}
          </div>
        </Card>
      )}

      {/* Session history */}
      <Card padding="sm">
        <button onClick={() => setShowHistory(h => !h)} className="w-full flex items-center justify-between text-xs text-[var(--muted)]">
          <span>{tt('info.history')} ({sessions.length})</span>
          <span className="text-[10px]">{showHistory ? '▾' : '▸'}</span>
        </button>
        {showHistory && (
          <div className="mt-2 space-y-1 max-h-48 overflow-y-auto">
            {sessions.length === 0 ? (
              <EmptyState icon="💬" title={tt('info.noHistory')} />
            ) : (
              sessions.map((s, i) => (
                <div key={s.seed || i} className={`flex items-center justify-between px-2 py-1 rounded-md text-xs ${s.seed === sessionId ? 'bg-[var(--accent-light)]' : 'hover:bg-[var(--bg-tertiary)]'}`}>
                  <button onClick={() => onResumeSession(s.seed)} className="flex-1 text-left truncate">
                    <span className="text-[var(--text-h)]">{s.seed.slice(0, 10)}</span>
                    {s.date && <span className="text-[var(--muted)] ml-1">{s.date}</span>}
                    {s.message_count && <span className="text-[var(--muted)] ml-1">{s.message_count} {tt('info.messages')}</span>}
                  </button>
                  <Button variant="ghost" size="sm" onClick={() => onDeleteSession(s.seed)} aria-label={tt('common.delete')}>
                    ×
                  </Button>
                </div>
              ))
            )}
          </div>
        )}
      </Card>

      {/* Audit Log */}
      {auditLog.length > 0 && (
        <Card padding="sm">
          <div className="text-xs text-[var(--muted)] mb-1">{tt('info.auditLog')}</div>
          <div className="space-y-0.5 max-h-32 overflow-y-auto">
            {auditLog.slice().reverse().map((r: any, i: number) => (
              <div key={i} className="text-xs font-mono truncate">
                <span className={r.success ? 'text-[var(--success)]' : 'text-[var(--error)]'}>
                  {r.success ? '✓' : '✗'}
                </span>
                <span className="text-[var(--text-h)] ml-1">{r.name}</span>
                {r.args && <span className="text-[var(--muted)] ml-1">{r.args.slice(0, 60)}</span>}
              </div>
            ))}
          </div>
        </Card>
      )}

      {/* Actions */}
      <div className="flex gap-2">
        <Button variant="secondary" size="sm" onClick={onNewSession} className="flex-1">
          {tt('chat.newSession')}
        </Button>
        <Button variant="ghost" size="sm" onClick={onDeleteAllSessions} className="text-[var(--error)]">
          {tt('common.delete')}
        </Button>
      </div>
    </div>
  )
}
