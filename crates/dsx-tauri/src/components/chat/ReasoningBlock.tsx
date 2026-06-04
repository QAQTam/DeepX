// ── ReasoningBlock ──
// Collapsible reasoning/thinking chain display.

import { useState } from 'react'
import { tt } from '../../i18n'

interface ReasoningBlockProps {
  content: string
}

export function ReasoningBlock({ content }: ReasoningBlockProps) {
  const [open, setOpen] = useState(false)

  if (!content.trim()) return null

  return (
    <div className="my-1">
      <button
        onClick={() => setOpen(o => !o)}
        className="flex items-center gap-1.5 text-xs text-[var(--muted)] hover:text-[var(--text)] transition-colors"
        aria-expanded={open}
      >
        <span className="text-[10px]">{open ? '▾' : '▸'}</span>
        <span>{open ? tt('chat.reasoningHide') : tt('chat.reasoningShow')}</span>
      </button>
      {open && (
        <div className="mt-2 p-3 bg-[var(--bg-tertiary)] rounded-lg text-xs text-[var(--text)] whitespace-pre-wrap border border-[var(--border-light)] max-h-64 overflow-y-auto font-mono leading-relaxed">
          {content}
        </div>
      )}
    </div>
  )
}
