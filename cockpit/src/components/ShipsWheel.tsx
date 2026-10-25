// The one busy indicator (theseus-6o8q8): a ship's wheel, turning while work is in flight. Its colour is the text's
// own, so each theme fits it. It turns by CSS (`ships-wheel` in index.css), and holds still, still drawn, under Calm and
// reduced motion.
import { cn } from '@/lib/format'

/** `sm` is 12 px, inline in a button or a line; `md` is 16 px, for a panel's header. */
export type WheelSize = 'sm' | 'md'

const PX: Record<WheelSize, number> = { sm: 12, md: 16 }

/** Eight spokes, each ending in a handle past the rim. */
const SPOKES = [0, 45, 90, 135, 180, 225, 270, 315]

/** A ship's wheel: a rim, a hub, eight spokes with handles past the rim. `label` is what a screen reader hears. */
export function ShipsWheel({ size = 'sm', label = 'working', className }: { size?: WheelSize; label?: string; className?: string }) {
  const px = PX[size]
  return (
    <svg role="status" aria-busy="true" aria-label={label} width={px} height={px} viewBox="0 0 24 24" fill="none"
      stroke="currentColor" strokeLinecap="round" className={cn('ships-wheel inline-block shrink-0 align-middle', className)}>
      <circle cx="12" cy="12" r="6.5" strokeWidth="1.8" />
      {SPOKES.map((deg) => (
        <g key={deg} transform={`rotate(${deg} 12 12)`}>
          <line x1="12" y1="12" x2="12" y2="2.6" strokeWidth="1.6" />
          <circle cx="12" cy="2.6" r="1.5" fill="currentColor" stroke="none" />
        </g>
      ))}
      <circle cx="12" cy="12" r="2.2" fill="currentColor" stroke="none" />
    </svg>
  )
}
