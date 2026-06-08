// ── ReasoningBlock ──
// Collapsible reasoning/thinking chain display.
// Only auto-scrolls during active streaming (content growing), not on resume.

import { createSignal, createEffect, onCleanup } from 'solid-js'
import { tt } from '../../i18n'

interface ReasoningBlockProps {
  content: string
}

export function ReasoningBlock(props: ReasoningBlockProps) {
  const [open, setOpen] = createSignal(true)
  let bottomRef!: HTMLDivElement
  let rafId = 0
  let prevLen = 0

  createEffect(() => {
    const cur = props.content.length
    open()
    // Only auto-scroll if content is actively growing (streaming).
    // On resume, content arrives at full length — skip scroll.
    const isStreaming = cur > prevLen && prevLen > 0
    prevLen = cur
    if (!isStreaming) return
    if (rafId) return
    rafId = requestAnimationFrame(() => {
      rafId = 0
      bottomRef?.scrollIntoView({ behavior: 'auto' })
    })
  })

  onCleanup(() => {
    if (rafId) cancelAnimationFrame(rafId)
  })

  if (!props.content.trim()) return null

  return (
    <div class="my-1">
      <button
        onClick={() => setOpen(o => !o)}
        class="flex items-center gap-1.5 text-xs text-[var(--muted)] hover:text-[var(--text)] transition-colors"
        aria-expanded={open()}
      >
        <span class="text-[11px]">{open() ? '\u25BE' : '\u25B8'}</span>
        <span>{open() ? tt('chat.reasoningHide') : tt('chat.reasoningShow')}</span>
      </button>
      {open() && (
        <div class="mt-2 p-3 bg-[var(--bg-tertiary)] rounded-lg text-[13px] text-[var(--text)] whitespace-pre-wrap border border-[var(--border-light)] max-h-64 overflow-y-auto font-mono leading-relaxed">
          {props.content}
          <div ref={bottomRef} />
        </div>
      )}
    </div>
  )
}
