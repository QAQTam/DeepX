// ── Card ──

import type { ReactNode, HTMLAttributes } from 'react'

interface CardProps extends HTMLAttributes<HTMLDivElement> {
  padding?: 'none' | 'sm' | 'md'
  children: ReactNode
}

const paddingClass = { none: '', sm: 'p-3', md: 'p-4' } as const

export function Card({ padding = 'md', children, className = '', ...rest }: CardProps) {
  return (
    <div
      className={`bg-[var(--bg-secondary)] border border-[var(--border-light)] rounded-xl ${paddingClass[padding]} ${className}`}
      {...rest}
    >
      {children}
    </div>
  )
}
