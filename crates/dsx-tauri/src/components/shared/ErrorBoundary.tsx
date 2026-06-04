// ── ErrorBoundary ──
// Catches render errors in a subtree. Shows fallback UI.

import { Component, type ReactNode, type ErrorInfo } from 'react'

interface Props {
  children: ReactNode
  fallback?: ReactNode
  onError?: (error: Error, info: ErrorInfo) => void
}

interface State {
  hasError: boolean
  error: Error | null
}

export class ErrorBoundary extends Component<Props, State> {
  state: State = { hasError: false, error: null }

  static getDerivedStateFromError(error: Error): State {
    return { hasError: true, error }
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error('[ErrorBoundary]', error, info)
    this.props.onError?.(error, info)
  }

  render() {
    if (this.state.hasError) {
      return this.props.fallback || (
        <div className="p-4 bg-[var(--bg-secondary)] border border-[var(--error)]/30 rounded-lg" role="alert">
          <div className="flex items-center gap-2 mb-2">
            <span className="text-[var(--error)] text-lg">⚠</span>
            <span className="text-sm font-medium text-[var(--text-h)]">发生了错误</span>
          </div>
          <p className="text-xs text-[var(--text)] mb-2 font-mono">{this.state.error?.message}</p>
          <button
            onClick={() => this.setState({ hasError: false, error: null })}
            className="text-xs text-[var(--accent)] hover:underline focus-visible:outline-2 focus-visible:outline-[var(--accent)]"
          >
            重试
          </button>
        </div>
      )
    }
    return this.props.children
  }
}
