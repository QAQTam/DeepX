// ── ReasoningBlock ──
// Collapsible reasoning/thinking chain display.

import { createSignal, createEffect } from 'solid-js'
import { tt } from '../../i18n'

interface ReasoningBlockProps {
  content: string
}

export function ReasoningBlock(props: ReasoningBlockProps) {
  const [open, setOpen] = createSignal(true)
  let bottomRef!: HTMLDivElement

  createEffect(() => {
    props.content
    open()
    bottomRef?.scrollIntoView({ behavior: 'smooth' })
  })

  if (!props.content.trim()) return null

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
          {props.content}
          <div ref={bottomRef} />
        </div>
      )}
    </div>
  )
}
