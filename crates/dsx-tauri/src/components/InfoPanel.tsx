import { useState } from 'preact/compat'
import { T } from '../i18n'

interface InfoPanelProps {
  tokens: { used: number; limit: number }
  cache: { hit: number; miss: number }
  predictedCacheHitPct: number | null
  balance: string
  sessionId: string
  sessions: any[]
  onSettings: () => void
  onNewSession: () => void
  onResumeSession: (seed: string) => void
  onDeleteAllSessions: () => void
  onDeleteSession: (seed: string) => void
}

export function InfoPanel({
  tokens, cache, predictedCacheHitPct, balance,
  onSettings, onNewSession, onResumeSession,
  sessionId, sessions,
  onDeleteAllSessions, onDeleteSession,
}: InfoPanelProps) {
  const pct = tokens.limit > 0 ? tokens.used / tokens.limit : 0
  const bar = pct > 0.65 ? 'var(--error)' : pct > 0.4 ? 'var(--warning)' : 'var(--accent)'
  const [showHistory, setShowHistory] = useState(false)

  const fmtDate = (ts: number) => {
    const d = new Date(ts * 1000)
    return `${d.getMonth() + 1}/${d.getDate()} ${d.getHours()}:${String(d.getMinutes()).padStart(2, '0')}`
  }

  return (
    <div className="h-full flex flex-col text-xs">
      <div className="p-3 border-b border-[var(--border)]">
        <div className="flex items-center justify-between mb-1">
          <div className="font-bold text-[var(--text-h)] text-sm">DSX</div>
          <div className="flex gap-1">
            <button onClick={onNewSession} className="text-[var(--muted)] hover:text-[var(--accent)] hover:bg-[var(--bg-hover)] rounded-lg min-w-[36px] min-h-[36px] flex items-center justify-center transition-colors text-lg" title="新对话" aria-label="新对话">＋</button>
            <button onClick={onSettings} className="text-[var(--muted)] hover:text-[var(--text-h)] hover:bg-[var(--bg-hover)] rounded-lg min-w-[36px] min-h-[36px] flex items-center justify-center transition-colors text-lg" aria-label="设置">⚙</button>
          </div>
        </div>
        <div className="text-xs font-mono text-[var(--muted)] truncate" title={sessionId}>{sessionId ? `#${sessionId.slice(0, 8)}` : '---'}</div>
      </div>

      <div className="p-3 space-y-3">
        <div>
          <div className="text-[var(--muted)] mb-1">{T.context}</div>
          <div className="h-2 bg-[var(--bg-tertiary)] rounded-full overflow-hidden">
            <div className="h-full rounded-full transition-all" style={{ width: `${Math.min(pct * 100, 100)}%`, background: bar }} />
          </div>
          <div className="text-xs text-[var(--muted)] mt-1">{tokens.used.toLocaleString()} / {tokens.limit.toLocaleString()}</div>
        </div>
        {cache.hit + cache.miss > 0 && (
          <div>
            <div className="text-[var(--muted)] mb-1">Cache</div>
            <div className="h-1.5 bg-[var(--bg-tertiary)] rounded-full overflow-hidden">
              <div className="h-full rounded-full bg-[var(--success)] transition-all"
                style={{ width: `${(cache.hit / (cache.hit + cache.miss)) * 100}%` }} />
            </div>
            <div className="text-xs text-[var(--muted)] mt-0.5">
              {Math.round((cache.hit / (cache.hit + cache.miss)) * 100)}% hit · {(cache.hit / 1000).toFixed(0)}K hit / {(cache.miss / 1000).toFixed(0)}K miss
            </div>
          </div>
        )}
        {predictedCacheHitPct !== null && (
          <div>
            <div className="text-[var(--muted)] mb-1">预测缓存命中</div>
            <div className="h-1.5 bg-[var(--bg-tertiary)] rounded-full overflow-hidden">
              <div className="h-full rounded-full bg-[var(--accent)] transition-all"
                style={{ width: `${predictedCacheHitPct * 100}%` }} />
            </div>
            <div className="text-xs text-[var(--muted)] mt-0.5">
              {Math.round(predictedCacheHitPct * 100)}%
            </div>
          </div>
        )}
        {balance && (
          <div className="text-xs">
            <span className="text-[var(--muted)]">余额: </span>
            <span className={`font-medium ${balance.startsWith('-') ? 'text-[var(--error)]' : balance === '0.00 CNY' || balance.startsWith('0') ? 'text-[var(--muted)]' : 'text-[var(--success)]'}`}>{balance}</span>
          </div>
        )}
      </div>

      {sessions.length > 0 && (
        <div className="border-t border-[var(--border)] flex-1 overflow-hidden flex flex-col">
          <button onClick={() => setShowHistory(!showHistory)}
            className="flex items-center justify-between px-3 py-2 text-xs text-[var(--muted)] hover:text-[var(--text-h)] hover:bg-[var(--bg-hover)]">
            <span>历史对话 ({sessions.length})</span>
            <span className="flex items-center gap-2">
              <span onClick={(e) => { e.stopPropagation(); if (confirm(`删除全部 ${sessions.length} 个会话？`)) onDeleteAllSessions() }}
                className="text-xs px-2 py-1 rounded text-[var(--error)] hover:bg-[var(--error)]/10 cursor-pointer transition-colors" title="删除全部" role="button" tabIndex={0}>
                全部清除
              </span>
              <span>{showHistory ? '▾' : '▸'}</span>
            </span>
          </button>
          {showHistory && (
            <div className="flex-1 overflow-y-auto px-2 space-y-0.5 pb-2">
              {sessions.map((s, i) => (
                <div key={i} onClick={() => onResumeSession(s.seed)} className="px-2 py-1.5 rounded-md hover:bg-[var(--bg-hover)] cursor-pointer text-xs flex items-start gap-1">
                  <div className="flex-1 min-w-0">
                    <div className="text-[var(--text)] truncate">{s.last_summary || `对话 ${s.seed?.slice(0, 8) || ''}`}</div>
                    <div className="text-xs text-[var(--muted)]">
                      {s.model || '?'} · {fmtDate(s.updated_at || 0)} · {s.message_count || s.messages?.length || 0}条
                    </div>
                  </div>
                  <button onClick={(e) => { e.stopPropagation(); if (confirm('删除此会话？')) onDeleteSession(s.seed) }}
                    className="ml-auto text-[var(--muted)] hover:text-[var(--error)] hover:bg-[var(--bg-hover)] rounded-lg min-w-[32px] min-h-[32px] flex items-center justify-center shrink-0 transition-colors text-base" aria-label="删除会话">
                    ✕
                  </button>
                </div>
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
