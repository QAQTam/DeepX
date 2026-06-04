// ── Input ──
// Standard text/password input with label + error support.

import { type InputHTMLAttributes, useId } from 'react'

interface InputProps extends InputHTMLAttributes<HTMLInputElement> {
  label?: string
  hint?: string
  error?: string
}

export function Input({ label, hint, error, className = '', id, ...rest }: InputProps) {
  const generatedId = useId()
  const inputId = id || generatedId
  return (
    <div className="flex flex-col gap-1">
      {label && (
        <label htmlFor={inputId} className="text-sm font-medium text-[var(--text-h)]">
          {label}
        </label>
      )}
      <input
        id={inputId}
        className={`bg-[var(--bg-primary)] border rounded-lg px-3 py-2 text-sm text-[var(--text-h)]
          outline-none transition-colors font-mono
          placeholder:text-[var(--muted)]
          focus:border-[var(--accent)] focus:ring-1 focus:ring-[var(--accent)]/20
          ${error ? 'border-[var(--error)]' : 'border-[var(--border)]'}
          ${className}`}
        aria-invalid={!!error}
        aria-describedby={error ? `${inputId}-error` : hint ? `${inputId}-hint` : undefined}
        {...rest}
      />
      {hint && !error && (
        <p id={`${inputId}-hint`} className="text-xs text-[var(--muted)]">{hint}</p>
      )}
      {error && (
        <p id={`${inputId}-error`} className="text-xs text-[var(--error)]" role="alert">{error}</p>
      )}
    </div>
  )
}
