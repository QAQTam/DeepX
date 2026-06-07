// ── ToolBatchStrip ──
// Collapses >5 tool calls into a compact summary with per-tool status.
// Each tool name has a running/done indicator dot.

import { createSignal, For } from 'solid-js'
import { ToolCard } from './ToolCard'
import type { ToolCardEntry } from '../../types'

interface Props {
  cards: ToolCardEntry[]
}

const MAX_UNCOLLAPSED = 5

export function ToolBatchStrip(props: Props) {
  if (props.cards.length <= MAX_UNCOLLAPSED) {
    return (
      <div class="mt-2 space-y-2 max-w-[85%]">
        <For each={props.cards}>{(tc, i) => (
          <ToolCard ctx={{
            id: tc.id || `tc-${i()}`,
            name: tc.name,
            args: tc.args || '',
            body: tc.body,
            output: tc.output,
            success: tc.success,
          }} />
        )}</For>
      </div>
    )
  }

  const [open, setOpen] = createSignal(false)
  const pendingCount = () => props.cards.filter(c => c.success === undefined).length
  const doneCount = () => props.cards.filter(c => c.success !== undefined).length

  return (
    <div class="mt-2 max-w-[85%]">
      {/* Summary strip */}
      <button
        onClick={() => setOpen(o => !o)}
        class="w-full flex items-center gap-2.5 px-3 py-2 rounded-lg border border-[var(--accent)]/20 bg-[var(--bg-secondary)] hover:bg-[var(--bg-tertiary)] transition-colors text-left"
      >
        <span class="shrink-0 text-sm anim-spin-slow">
          <svg width="16" height="16" viewBox="0 0 16 16" class="text-[var(--accent)]">
            <circle cx="8" cy="8" r="6" fill="none" stroke="currentColor" stroke-width="1.5" stroke-dasharray="6 2">
              <animateTransform attributeName="transform" type="rotate" from="0 8 8" to="360 8 8" dur="1.2s" repeatCount="indefinite"/>
            </circle>
          </svg>
        </span>
        <div class="flex-1 min-w-0">
          <div class="text-xs font-medium text-[var(--text-h)]">
            {pendingCount() > 0
              ? `${pendingCount()} running · ${doneCount()} done`
              : `${doneCount()} tools completed`}
          </div>
          <div class="text-[11px] text-[var(--muted)] font-mono truncate mt-0.5">
            {props.cards.map(c => c.name).join(' · ')}
          </div>
        </div>
        <span class="shrink-0 text-xs text-[var(--muted)]">
          {open() ? '▴' : `▾ ${props.cards.length}`}
        </span>
      </button>

      {/* Expanded list */}
      {open() && (
        <div class="mt-2 space-y-2">
          <For each={props.cards}>{(tc, i) => (
            <ToolCard ctx={{
              id: tc.id || `tc-${i()}`,
              name: tc.name,
              args: tc.args || '',
              body: tc.body,
              output: tc.output,
              liveOutput: tc.liveOutput,
              success: tc.success,
            }} />
          )}</For>
        </div>
      )}
    </div>
  )
}
