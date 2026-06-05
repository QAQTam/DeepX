// ── Card ──

import type { JSX } from 'solid-js'

interface CardProps extends JSX.HTMLAttributes<HTMLDivElement> {
  padding?: 'none' | 'sm' | 'md'
  children: JSX.Element
}

const paddingClass = { none: '', sm: 'p-3', md: 'p-4' } as const

export function Card({ padding = 'md', children, class: className = '', ...rest }: CardProps) {
  return (
    <div
      class={`bg-[var(--bg-secondary)] border border-[var(--border-light)] rounded-xl ${paddingClass[padding]} ${className}`}
      {...rest}
    >
      {children}
    </div>
  )
}
