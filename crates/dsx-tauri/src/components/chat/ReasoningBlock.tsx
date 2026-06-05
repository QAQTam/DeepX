// ── ReasoningBlock ──
// Collapsible reasoning/thinking chain display.

import { createSignal, onMount } from 'solid-js'
import { tt } from '../../i18n'

interface ReasoningBlockProps {
  content: string
}

export function ReasoningBlock({ content }: ReasoningBlockProps) {
  const [open, setOpen] = createSignal(true)
  let bottomRef!: HTMLDivElement

  onMount(() => {
    if (open()) {
      bottomRef?.scrollIntoView({ behavior: 'smooth' })
    }
  })

  if (!content.trim()) return null

  return (
    <div class="my-1">
      <button
        onClick={() => setOpen(o => !o)}
        class="flex items-center gap-1.5 text-xs text-[var(--muted)] hover:text-[var(--text)] transition-colors"
        aria-expanded={open()}
      >
        <span class="text-[10px]">{open() ? '\u25BE' : '\u25B8'}</span>
        <span>{open() ? tt('chat.reasoningHide') : tt('chat.reasoningShow')}</span>
      </button>
      {open() && (
        <div class="mt-2 p-3 bg-[var(--bg-tertiary)] rounded-lg text-xs text-[var(--text)] whitespace-pre-wrap border border-[var(--border-light)] max-h-64 overflow-y-auto font-mono leading-relaxed">
          {content}
          <div ref={bottomRef} />
        </div>
      )}
    </div>
  )
}
