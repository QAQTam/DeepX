// ── Skeleton ──
// Inline block shimmer placeholder for loading states.

interface SkeletonProps {
  width?: string
  height?: string
  rounded?: 'sm' | 'md' | 'full'
  className?: string
}

const roundedClass = { sm: 'rounded-sm', md: 'rounded-md', full: 'rounded-full' }

export function Skeleton({ width = '100%', height = '1rem', rounded = 'sm', className = '' }: SkeletonProps) {
  return (
    <span
      aria-hidden="true"
      className={`inline-block anim-shimmer bg-[var(--bg-tertiary)] ${roundedClass[rounded]} ${className}`}
      style={{ width, height }}
    />
  )
}

/** Pre-built skeleton variants */
export function SkeletonLine({ w = '100%' }: { w?: string }) { return <Skeleton width={w} height="0.875rem" className="my-0.5" /> }
export function SkeletonBlock({ lines = 3 }: { lines?: number }) {
  return <div className="space-y-1.5">{[...Array(lines)].map((_, i) => <SkeletonLine key={i} w={i === lines - 1 ? '65%' : '100%'} />)}</div>
}