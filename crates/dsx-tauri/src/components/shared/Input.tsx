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
  return (
    <div class="flex flex-col gap-1">
      {props.label && (
        <label for={id()} class="text-sm font-medium text-[var(--text-h)]">
          {props.label}
        </label>
      )}
      <input
        {...props}
        id={id()}
        class={`bg-[var(--bg-primary)] border rounded-lg px-3 py-2 text-sm text-[var(--text-h)]
          outline-none transition-colors font-mono
          placeholder:text-[var(--muted)]
          focus:border-[var(--accent)] focus:ring-1 focus:ring-[var(--accent)]/20
          ${props.error ? 'border-[var(--error)]' : 'border-[var(--border)]'}
          ${props.class || ''}`}
        aria-invalid={!!props.error}
        aria-describedby={props.error ? `${id()}-error` : props.hint ? `${id()}-hint` : undefined}
      />
      {props.hint && !props.error && (
        <p id={`${id()}-hint`} class="text-xs text-[var(--muted)]">{props.hint}</p>
      )}
      {props.error && (
        <p id={`${id()}-error`} class="text-xs text-[var(--error)]" role="alert">{props.error}</p>
      )}
    </div>
  )
}
