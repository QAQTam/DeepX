// ── Select ──

import { type SelectHTMLAttributes, useId } from 'react'

interface SelectProps extends SelectHTMLAttributes<HTMLSelectElement> {
  label?: string
  options: { value: string; label: string }[]
}

export function Select({ label, options, className = '', id, ...rest }: SelectProps) {
  const generatedId = useId()
  const selectId = id || generatedId
  return (
    <div className="flex flex-col gap-1">
      {label && (
        <label htmlFor={selectId} className="text-sm font-medium text-[var(--text-h)]">{label}</label>
      )}
      <select id={selectId}
        className={`bg-[var(--bg-primary)] border border-[var(--border)] rounded-lg px-3 py-2 text-sm
          text-[var(--text-h)] outline-none transition-colors
          focus:border-[var(--accent)] focus:ring-1 focus:ring-[var(--accent)]/20 ${className}`}
        {...rest}
      >
        {options.map(o => (
          <option key={o.value} value={o.value}>{o.label}</option>
        ))}
      </select>
    </div>
  )
}
