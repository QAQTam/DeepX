// ── Spinner ──

interface SpinnerProps {
  size?: 'sm' | 'md' | 'lg'
  className?: string
}

const sizeMap = { sm: 'w-3 h-3', md: 'w-4 h-4', lg: 'w-5 h-5' }

export function Spinner({ size = 'md', className = '' }: SpinnerProps) {
  return (
    <span
      aria-label="Loading"
      role="status"
      className={`${sizeMap[size]} border-2 border-current border-r-transparent rounded-full inline-block anim-spin-slow ${className}`}
    />
  )
}
