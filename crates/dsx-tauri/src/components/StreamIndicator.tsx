// ── StreamIndicator ──
// Shows agent status: thinking (with timer), calling tools, or answering.

import { For } from 'solid-js'
import { tt } from '../i18n'

interface StreamIndicatorProps {
  mode: 'idle' | 'thinking' | 'tool_calling' | 'answering'
  toolNames: string[]
  secs: number
}

export function StreamIndicator(props: StreamIndicatorProps) {
  if (props.mode === 'idle') return null

  return (
    <div class="flex items-start gap-3 mb-4 pl-2 anim-fade-in">
      {/* Avatar dot */}
      <div class={`w-7 h-7 rounded-full flex items-center justify-center shrink-0 mt-0.5
        ${props.mode === 'thinking' ? 'bg-[var(--warning)]/15'
          : props.mode === 'tool_calling' ? 'bg-[var(--accent)]/15'
          : 'bg-[var(--success)]/15'}`}>
        {props.mode === 'thinking' ? <ThinkDots />
         : props.mode === 'tool_calling' ? <span class="text-sm anim-spin-slow">◇</span>
         : <span class="text-sm anim-blink">●</span>}
      </div>

      {/* Bubble */}
      <div class={`inline-block max-w-[80%] rounded-2xl px-4 py-2.5 text-sm leading-relaxed
        bg-[var(--bg-secondary)] border border-[var(--border)] rounded-bl-md transition-theme`}>
        {props.mode === 'thinking' && (
          <div class="flex items-center gap-2">
            <span class="text-[var(--warning)] font-medium">{tt('chat.thinking')}</span>
            <span class="text-[var(--muted)] text-xs font-mono">{props.secs}s</span>
          </div>
        )}
        {props.mode === 'tool_calling' && (
          <div>
            <span class="text-[var(--accent)] font-medium">{tt('chat.toolCalling')}</span>
            <div class="flex flex-wrap gap-1 mt-1">
              <For each={props.toolNames}>{(n) => (
                <span class="text-xs font-mono bg-[var(--bg-tertiary)] px-1.5 py-0.5 rounded text-[var(--text)]">{n}</span>
              )}</For>
            </div>
          </div>
        )}
        {props.mode === 'answering' && (
          <div class="flex items-center gap-2">
            <span class="text-[var(--success)] font-medium">{tt('chat.answering')}</span>
            <BlinkCursor />
          </div>
        )}
      </div>
    </div>
  )
}

function ThinkDots() {
  return (
    <svg width="18" height="6" viewBox="0 0 18 6" fill="currentColor" aria-hidden="true">
      <circle cx="3" cy="3" r="2" fill="currentColor" class="text-[var(--warning)]">
        <animate attributeName="cy" values="3;1;3" dur="0.8s" repeatCount="indefinite" begin="0s"/>
      </circle>
      <circle cx="9" cy="3" r="2" fill="currentColor" class="text-[var(--warning)]">
        <animate attributeName="cy" values="3;1;3" dur="0.8s" repeatCount="indefinite" begin="0.2s"/>
      </circle>
      <circle cx="15" cy="3" r="2" fill="currentColor" class="text-[var(--warning)]">
        <animate attributeName="cy" values="3;1;3" dur="0.8s" repeatCount="indefinite" begin="0.4s"/>
      </circle>
    </svg>
  )
}

function BlinkCursor() {
  return <span class="inline-block w-0.5 h-4 bg-[var(--success)] ml-1 anim-blink" aria-hidden="true" />
}
