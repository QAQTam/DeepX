// ── EmptyState ──

import { mergeProps } from 'solid-js'

interface EmptyStateProps {
  icon?: string
  title: string
  description?: string
  action?: { label: string; onClick: () => void }
}

export function EmptyState(props: EmptyStateProps) {
  const merged = mergeProps({ icon: '—' }, props)
  return (
    <div class="flex flex-col items-center justify-center py-6 text-center gap-2">
      <span class="text-2xl text-[var(--muted)]" aria-hidden="true">{merged.icon}</span>
      <p class="text-sm font-medium text-[var(--text)]">{props.title}</p>
      {props.description && <p class="text-xs text-[var(--muted)]">{props.description}</p>}
      {props.action && (
        <button onClick={props.action.onClick}
          class="mt-1 text-xs text-[var(--accent)] hover:underline focus-visible:outline-2 focus-visible:outline-[var(--accent)]">
          {props.action.label}
        </button>
      )}
    </div>
  )
}
