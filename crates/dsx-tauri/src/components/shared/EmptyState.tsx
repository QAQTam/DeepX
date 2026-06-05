// ── EmptyState ──

interface EmptyStateProps {
  icon?: string
  title: string
  description?: string
  action?: { label: string; onClick: () => void }
}

export function EmptyState({ icon = '—', title, description, action }: EmptyStateProps) {
  return (
    <div class="flex flex-col items-center justify-center py-6 text-center gap-2">
      <span class="text-2xl text-[var(--muted)]" aria-hidden="true">{icon}</span>
      <p class="text-sm font-medium text-[var(--text)]">{title}</p>
      {description && <p class="text-xs text-[var(--muted)]">{description}</p>}
      {action && (
        <button onClick={action.onClick}
          class="mt-1 text-xs text-[var(--accent)] hover:underline focus-visible:outline-2 focus-visible:outline-[var(--accent)]">
          {action.label}
        </button>
      )}
    </div>
  )
}
