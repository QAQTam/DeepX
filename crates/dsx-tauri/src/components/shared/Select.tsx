// ── Select ──

import type { JSX } from 'solid-js'
import { For } from 'solid-js'

let _selectId = 0
function nextSelectId(prefix = 'select') {
  return `${prefix}-${++_selectId}`
}

interface SelectProps extends JSX.SelectHTMLAttributes<HTMLSelectElement> {
  label?: string
  options: { value: string; label: string }[]
}

export function Select(props: SelectProps) {
  const id = () => props.id || nextSelectId()
  return (
    <div class="flex flex-col gap-1">
      {props.label && (
        <label for={id()} class="text-sm font-medium text-[var(--text-h)]">{props.label}</label>
      )}
      <select
        {...props}
        id={id()}
        class={`bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg px-3 py-2 text-sm
          text-[var(--text-h)] outline-none transition-colors
          focus:border-[var(--accent)] focus:ring-1 focus:ring-[var(--accent)]/20 ${props.class || ''}`}
      >
        <For each={props.options}>
          {o => <option value={o.value}>{o.label}</option>}
        </For>
      </select>
    </div>
  )
}
