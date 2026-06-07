// ── Input ──
// Standard text/password input with label + error support.

import type { JSX } from 'solid-js'

let _inputId = 0
function nextId(prefix = 'input') {
  return `${prefix}-${++_inputId}`
}

interface InputProps extends JSX.InputHTMLAttributes<HTMLInputElement> {
  label?: string
  hint?: string
  error?: string
}

export function Input(props: InputProps) {
  const id = () => props.id || nextId()
  const { label, hint, error, class: userClass, ...rest } = props
  return (
    <div class="flex flex-col gap-1">
      {label && (
        <label for={id()} class="text-sm font-medium text-[var(--text-h)]">
          {label}
        </label>
      )}
      <input
        {...rest}
        id={id()}
        class={`bg-[var(--bg-primary)] border rounded-lg px-3 py-2 text-sm text-[var(--text-h)]
          outline-none transition-colors font-mono
          placeholder:text-[var(--muted)]
          focus:border-[var(--accent)] focus:ring-1 focus:ring-[var(--accent)]/20
          ${error ? 'border-[var(--error)]' : 'border-[var(--border)]'}
          ${userClass || ''}`}
        aria-invalid={!!error}
        aria-describedby={error ? `${id()}-error` : hint ? `${id()}-hint` : undefined}
      />
      {hint && !error && (
        <p id={`${id()}-hint`} class="text-xs text-[var(--muted)]">{hint}</p>
      )}
      {error && (
        <p id={`${id()}-error`} class="text-xs text-[var(--error)]" role="alert">{error}</p>
      )}
    </div>
  )
}
