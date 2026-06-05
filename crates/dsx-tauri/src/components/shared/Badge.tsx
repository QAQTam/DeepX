// ── Badge ──

import type { JSX } from 'solid-js'

type BadgeVariant = 'default' | 'accent' | 'success' | 'warning' | 'error'

interface BadgeProps {
  variant?: BadgeVariant
  children: JSX.Element
  class?: string
}

const color: Record<BadgeVariant, string> = {
  default: 'bg-[var(--bg-tertiary)] text-[var(--text)]',
  accent:  'bg-[var(--accent)]/10 text-[var(--accent)]',
  success: 'bg-[var(--success)]/10 text-[var(--success)]',
  warning: 'bg-[var(--warning)]/10 text-[var(--warning)]',
  error:   'bg-[var(--error)]/10 text-[var(--error)]',
}

export function Badge({ variant = 'default', children, class: className = '' }: BadgeProps) {
  return (
    <span class={`inline-flex items-center px-1.5 py-0.5 rounded-md text-xs font-medium ${color[variant]} ${className}`}>
      {children}
    </span>
  )
}
