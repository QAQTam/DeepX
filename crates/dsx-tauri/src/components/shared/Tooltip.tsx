// ── Tooltip ──
// CSS-only tooltip. For more complex cases use aria-label instead.

import type { JSX } from 'solid-js'

interface TooltipProps {
  content: string
  children: JSX.Element
  position?: 'top' | 'bottom'
}

const posClass = { top: 'bottom-full mb-1.5', bottom: 'top-full mt-1.5' }

export function Tooltip({ content, children, position = 'top' }: TooltipProps) {
  return (
    <span class="relative group inline-flex" aria-label={content}>
      {children}
      <span class={`absolute left-1/2 -translate-x-1/2 ${posClass[position]} px-2 py-0.5
        text-xs bg-[var(--text-h)] text-[var(--bg-primary)] rounded-md whitespace-nowrap
        opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none z-50`}>
        {content}
      </span>
    </span>
  )
}
