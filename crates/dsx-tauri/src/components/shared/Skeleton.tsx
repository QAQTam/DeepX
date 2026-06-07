// ── Skeleton ──
// Inline block shimmer placeholder for loading states.

import { mergeProps } from 'solid-js'

interface SkeletonProps {
  width?: string
  height?: string
  rounded?: 'sm' | 'md' | 'full'
  class?: string
}

const roundedClass = { sm: 'rounded-sm', md: 'rounded-md', full: 'rounded-full' }

export function Skeleton(props: SkeletonProps) {
  const merged = mergeProps({ width: '100%', height: '1rem', rounded: 'sm' as const, class: '' }, props)
  return (
    <span
      aria-hidden="true"
      class={`inline-block anim-shimmer bg-[var(--bg-tertiary)] ${roundedClass[merged.rounded]} ${merged.class}`}
      style={{ width: merged.width, height: merged.height }}
    />
  )
}

/** Pre-built skeleton variants */
export function SkeletonLine({ w = '100%' }: { w?: string }) { return <Skeleton width={w} height="0.875rem" class="my-0.5" /> }
export function SkeletonBlock({ lines = 3 }: { lines?: number }) {
  return <div class="space-y-1.5">{[...Array(lines)].map((_, i) => <SkeletonLine w={i === lines - 1 ? '65%' : '100%'} />)}</div>
}
