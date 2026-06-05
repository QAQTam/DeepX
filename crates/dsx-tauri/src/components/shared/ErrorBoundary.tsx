// ── ErrorBoundary ──
// SolidJS built-in ErrorBoundary wrapper, matching the same API shape.

import { ErrorBoundary as SolidErrorBoundary, type JSX } from 'solid-js'
import { tt } from '../../i18n'

interface Props {
  children: JSX.Element
  fallback?: JSX.Element
  onError?: (error: Error) => void
}

export function ErrorBoundary(props: Props) {
  return (
    <SolidErrorBoundary
      fallback={(err: Error, reset: () => void) => {
        props.onError?.(err)
        return props.fallback || (
          <div class="p-4 bg-[var(--bg-secondary)] border border-[var(--error)]/30 rounded-lg" role="alert">
            <div class="flex items-center gap-2 mb-2">
              <span class="text-[var(--error)] text-lg">⚠</span>
              <span class="text-sm font-medium text-[var(--text-h)]">{tt('errors.renderError')}</span>
            </div>
            <p class="text-xs text-[var(--text)] mb-2 font-mono">{err.message}</p>
            <button
              onClick={reset}
              class="text-xs text-[var(--accent)] hover:underline focus-visible:outline-2 focus-visible:outline-[var(--accent)]"
            >
                {tt('common.retry')}
            </button>
          </div>
        )
      }}
    >
      {props.children}
    </SolidErrorBoundary>
  )
}
