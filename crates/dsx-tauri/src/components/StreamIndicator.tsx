// ── StreamIndicator ──
// Shows agent status with mode-specific animations: thinking dots,
// tool-calling activity bar, or typing cursor.

import { For } from 'solid-js'
import { tt } from '../i18n'

interface StreamIndicatorProps {
  mode: 'idle' | 'thinking' | 'tool_calling' | 'answering'
  toolNames: string[]
  secs: number
}

const ANIM_DELAYS = ['0ms', '80ms', '160ms', '240ms', '320ms', '400ms']

export function StreamIndicator(props: StreamIndicatorProps) {
  if (props.mode === 'idle') return null

  return (
    <div class={`mb-4 pl-2 anim-fade-in ${props.mode === 'tool_calling' ? 'tool-call-banner' : ''}`}>
      {props.mode === 'thinking' && <ThinkingBubble secs={props.secs} />}
      {props.mode === 'tool_calling' && <ToolCallingBubble names={props.toolNames} />}
      {props.mode === 'answering' && <AnsweringBubble />}
    </div>
  )
}

function ThinkingBubble(props: { secs: number }) {
  return (
    <div class="flex items-start gap-2.5">
      <div class="w-7 h-7 rounded-full bg-[var(--warning)]/15 flex items-center justify-center shrink-0 mt-0.5">
        <ThinkDots />
      </div>
      <div class="inline-block max-w-[80%] rounded-2xl px-4 py-2.5 text-sm bg-[var(--bg-secondary)] border border-[var(--border)] rounded-bl-md transition-theme">
        <div class="flex items-center gap-2">
          <span class="text-[var(--warning)] font-medium">{tt('chat.thinking')}</span>
          <span class="text-[var(--muted)] text-xs font-mono">{props.secs}s</span>
        </div>
      </div>
    </div>
  )
}

function ToolCallingBubble(props: { names: string[] }) {
  return (
    <div class="flex items-start gap-2.5">
      <div class="w-7 h-7 rounded-full bg-[var(--accent)]/15 flex items-center justify-center shrink-0 mt-0.5 anim-pulse-glow">
        <ActiveSpinner />
      </div>
      <div class="inline-block max-w-[85%] rounded-2xl px-4 py-2.5 text-sm bg-[var(--bg-secondary)] border border-[var(--accent)]/30 rounded-bl-md shadow-[0_0_12px_rgba(124,58,237,0.15)] transition-theme">
        <div class="text-[var(--accent)] font-medium mb-1.5">{tt('chat.toolCalling')}</div>
        <div class="flex flex-wrap gap-1.5">
          <For each={props.names}>{(n, i) => (
            <span
              class="text-xs font-mono bg-[var(--accent)]/10 text-[var(--accent)] px-2 py-1 rounded-md border border-[var(--accent)]/20 animate-task-enter"
              style={{ 'animation-delay': ANIM_DELAYS[i() % ANIM_DELAYS.length] ?? '0ms' } as any}
            >
              {n}
            </span>
          )}</For>
        </div>
      </div>
    </div>
  )
}

function AnsweringBubble() {
  return (
    <div class="flex items-start gap-2.5">
      <div class="w-7 h-7 rounded-full bg-[var(--success)]/15 flex items-center justify-center shrink-0 mt-0.5">
        <span class="text-sm anim-blink">●</span>
      </div>
      <div class="inline-block max-w-[80%] rounded-2xl px-4 py-2.5 text-sm bg-[var(--bg-secondary)] border border-[var(--border)] rounded-bl-md transition-theme">
        <div class="flex items-center gap-2">
          <span class="text-[var(--success)] font-medium">{tt('chat.answering')}</span>
          <BlinkCursor />
        </div>
      </div>
    </div>
  )
}

function ThinkDots() {
  return (
    <svg width="18" height="6" viewBox="0 0 18 6" fill="currentColor" aria-hidden="true">
      <circle cx="3" cy="3" r="2" class="text-[var(--warning)]">
        <animate attributeName="cy" values="3;1;3" dur="0.8s" repeatCount="indefinite" begin="0s"/>
      </circle>
      <circle cx="9" cy="3" r="2" class="text-[var(--warning)]">
        <animate attributeName="cy" values="3;1;3" dur="0.8s" repeatCount="indefinite" begin="0.2s"/>
      </circle>
      <circle cx="15" cy="3" r="2" class="text-[var(--warning)]">
        <animate attributeName="cy" values="3;1;3" dur="0.8s" repeatCount="indefinite" begin="0.4s"/>
      </circle>
    </svg>
  )
}

function ActiveSpinner() {
  return (
    <svg width="16" height="16" viewBox="0 0 16 16" class="text-[var(--accent)]">
      <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray="8 3">
        <animateTransform attributeName="transform" type="rotate" from="0 8 8" to="360 8 8" dur="1.2s" repeatCount="indefinite"/>
      </circle>
    </svg>
  )
}

function BlinkCursor() {
  return <span class="inline-block w-0.5 h-4 bg-[var(--success)] ml-1 anim-blink" aria-hidden="true" />
}
