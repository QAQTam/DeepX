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
        ${mode === 'thinking' ? 'bg-[var(--warning)]/15' :
          mode === 'tool_calling' ? 'bg-[var(--accent)]/15' :
          'bg-[var(--success)]/15'}`}>
        {mode === 'thinking' ? <ThinkDots /> :
         mode === 'tool_calling' ? <span className="text-sm animate-spin">◇</span> :
         <span className="text-sm">●</span>}
      </div>

      {/* Bubble */}
      <div className={`inline-block max-w-[80%] rounded-2xl px-4 py-2.5 text-sm leading-relaxed
        bg-[var(--bg-secondary)] border border-[var(--border)] rounded-bl-md`}>
        {mode === 'thinking' && (
          <div className="flex items-center gap-2">
            <span className="text-[var(--warning)] font-medium">思考中</span>
            <span className="text-[var(--muted)] text-xs font-mono">{secs}s</span>
          </div>
        )}
        {mode === 'tool_calling' && (
          <div>
            <span className="text-[var(--accent)] font-medium">调用工具</span>
            <div className="flex flex-wrap gap-1 mt-1">
              {toolNames.map((n, i) => (
                <span key={i} className="text-xs font-mono bg-[var(--bg-tertiary)] px-1.5 py-0.5 rounded text-[var(--text)]">{n}</span>
              ))}
            </div>
          </div>
        )}
        {mode === 'answering' && (
          <div className="flex items-center gap-2">
            <span className="text-[var(--success)] font-medium">回答中</span>
            <BlinkCursor />
          </div>
        )}
      </div>
    </div>
  )
}

function ThinkDots() {
  return (
    <span className="inline-flex gap-0.5">
      <span className="w-1 h-1 rounded-full bg-[var(--warning)] animate-pulse" />
      <span className="w-1 h-1 rounded-full bg-[var(--warning)] animate-pulse" style={{ animationDelay: '0.15s' }} />
      <span className="w-1 h-1 rounded-full bg-[var(--warning)] animate-pulse" style={{ animationDelay: '0.3s' }} />
    </span>
  )
}

function BlinkCursor() {
  return (
    <>
      <style>{`@keyframes blink{0%,50%{opacity:1}51%,100%{opacity:0}}`}</style>
      <span className="inline-block w-0.5 h-4 bg-[var(--success)] ml-1" style={{ animation: 'blink 1s step-end infinite' }} />
    </>
  )
}
