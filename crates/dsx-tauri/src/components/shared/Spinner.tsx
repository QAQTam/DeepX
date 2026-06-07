// ── Spinner ──

import { mergeProps } from 'solid-js'

interface SpinnerProps {
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

const sizeMap = { sm: 'w-3 h-3', md: 'w-4 h-4', lg: 'w-5 h-5' }

export function Spinner(props: SpinnerProps) {
  const merged = mergeProps({ size: 'md' as const, className: '' }, props)
  return (
    <span
      aria-label="Loading"
      role="status"
      class={`${sizeMap[merged.size]} border-2 border-current border-r-transparent rounded-full inline-block anim-spin-slow ${merged.className}`}
    />
  )
}
