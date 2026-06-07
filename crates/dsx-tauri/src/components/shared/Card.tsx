// ── Card ──

import type { JSX } from 'solid-js'
import { mergeProps } from 'solid-js'

interface CardProps extends JSX.HTMLAttributes<HTMLDivElement> {
  padding?: 'none' | 'sm' | 'md'
  children: JSX.Element
}

const paddingClass = { none: '', sm: 'p-3', md: 'p-4' } as const

export function Card(props: CardProps) {
  const merged = mergeProps({ padding: 'md' as const, class: '' }, props)
  const { padding:_, class:__, ...rest } = merged
  return (
    <div
      class={`bg-[var(--bg-secondary)] border border-[var(--border-light)] rounded-xl ${paddingClass[merged.padding]} ${merged.class}`}
      {...rest}
    >
      {merged.children}
    </div>
  )
}
