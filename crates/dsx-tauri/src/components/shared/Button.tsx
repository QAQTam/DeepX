// ── Button ──
// Variants: primary | secondary | ghost | danger
// Sizes: sm | md | lg

import type { JSX } from 'solid-js'
import { Spinner } from './Spinner'

type Variant = 'primary' | 'secondary' | 'ghost' | 'danger'
type Size = 'sm' | 'md' | 'lg'

interface ButtonProps extends JSX.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant
  size?: Size
  loading?: boolean
  icon?: JSX.Element
  children?: JSX.Element
}

const variantClass: Record<Variant, string> = {
  primary:  'bg-[var(--accent)] text-white hover:brightness-110',
  secondary: 'bg-[var(--bg-tertiary)] text-[var(--text-h)] hover:brightness-95 border border-[var(--border)]',
  ghost:    'text-[var(--text-h)] hover:bg-[var(--bg-tertiary)]',
  danger:   'bg-[var(--error)] text-white hover:brightness-110',
}

const sizeClass: Record<Size, string> = {
  sm: 'px-2.5 py-1 text-xs rounded-md gap-1',
  md: 'px-3.5 py-2 text-sm rounded-lg gap-1.5',
  lg: 'px-5 py-2.5 text-sm rounded-xl gap-2',
}

export function Button({
  variant = 'secondary', size = 'md', loading, icon, children,
  disabled, class: className = '', ...rest
}: ButtonProps) {
  return (
    <button
      class={`inline-flex items-center justify-center font-medium transition-colors
        disabled:opacity-40 disabled:cursor-not-allowed focus-visible:outline-2 focus-visible:outline-[var(--accent)]
        ${variantClass[variant]} ${sizeClass[size]} ${className}`}
      disabled={disabled || loading}
      aria-busy={loading}
      {...rest}
    >
      {loading ? <Spinner size={size} /> : icon}
      {children && <span>{children}</span>}
    </button>
  )
}
