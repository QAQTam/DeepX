// ── StreamIndicator ──
// Shows agent status: thinking (with timer), calling tools, or answering.

import { tt } from '../i18n'

interface StreamIndicatorProps {
  mode: 'idle' | 'thinking' | 'tool_calling' | 'answering'
  toolNames: string[]
  secs: number
}

export function StreamIndicator({ mode, toolNames, secs }: StreamIndicatorProps) {
  if (mode === 'idle') return null

  return (
    <div className="flex items-start gap-3 mb-4 pl-2">
      {/* Avatar dot */}
      <div className={`w-7 h-7 rounded-full flex items-center justify-center shrink-0 mt-0.5
        ${mode === 'thinking' ? 'bg-[var(--warning)]/15'
          : mode === 'tool_calling' ? 'bg-[var(--accent)]/15'
          : 'bg-[var(--success)]/15'}`}>
        {mode === 'thinking' ? <ThinkDots />
         : mode === 'tool_calling' ? <span className="text-sm anim-spin-slow">◇</span>
         : <span className="text-sm anim-blink">●</span>}
      </div>

      {/* Bubble */}
      <div className={`inline-block max-w-[80%] rounded-2xl px-4 py-2.5 text-sm leading-relaxed
        bg-[var(--bg-secondary)] border border-[var(--border)] rounded-bl-md transition-theme`}>
        {mode === 'thinking' && (
          <div className="flex items-center gap-2">
            <span className="text-[var(--warning)] font-medium">{tt('chat.thinking')}</span>
            <span className="text-[var(--muted)] text-xs font-mono">{secs}s</span>
          </div>
        )}
        {mode === 'tool_calling' && (
          <div>
            <span className="text-[var(--accent)] font-medium">{tt('chat.toolCalling')}</span>
            <div className="flex flex-wrap gap-1 mt-1">
              {toolNames.map((n, i) => (
                <span key={i} className="text-xs font-mono bg-[var(--bg-tertiary)] px-1.5 py-0.5 rounded text-[var(--text)]">{n}</span>
              ))}
            </div>
          </div>
        )}
        {mode === 'answering' && (
          <div className="flex items-center gap-2">
            <span className="text-[var(--success)] font-medium">{tt('chat.answering')}</span>
            <BlinkCursor />
          </div>
        )}
      </div>
    </div>
  )
}

function ThinkDots() {
  return (
    <svg width="18" height="6" viewBox="0 0 18 6">
      <circle cx="3" cy="3" r="2" fill="currentColor" className="text-[var(--warning)]">
        <animate attributeName="cy" values="3;1;3" dur="0.8s" repeatCount="indefinite" begin="0s"/>
      </circle>
      <circle cx="9" cy="3" r="2" fill="currentColor" className="text-[var(--warning)]">
        <animate attributeName="cy" values="3;1;3" dur="0.8s" repeatCount="indefinite" begin="0.2s"/>
      </circle>
      <circle cx="15" cy="3" r="2" fill="currentColor" className="text-[var(--warning)]">
        <animate attributeName="cy" values="3;1;3" dur="0.8s" repeatCount="indefinite" begin="0.4s"/>
      </circle>
    </svg>
  )
}

function BlinkCursor() {
  return <span className="inline-block w-0.5 h-4 bg-[var(--success)] ml-1 anim-blink" aria-hidden="true" />
}
