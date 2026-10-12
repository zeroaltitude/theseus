// The sonar ring (theseus-mbulo, `sonar.ts`): a thin circle growing from the middle of the Ship to beyond its edges as
// a ping sounds, fading as it grows, a fainter one trailing it. Drawn above the sea and its labels and below the
// panels (it sits after the labels in the Ship, before the panels, and takes no pointer). Nothing is mounted while idle.
import { useLayoutEffect, useRef, useState } from 'react'
import { RING, ringDiameter, useRing } from './sonar.ts'

export function SonarRing() {
  const id = useRing((s) => s.id)
  const box = useRef<HTMLDivElement>(null)
  const [d, setD] = useState(0)
  useLayoutEffect(() => {
    const r = box.current?.parentElement?.getBoundingClientRect()
    if (id && r) setD(ringDiameter(r.width, r.height))
  }, [id])
  if (!id) return null
  const ring = (delay: number, opacity: number) => (
    <svg key={delay} className="sonar-ring" width={d} height={d} viewBox="0 0 100 100" aria-hidden
      style={{ animationDuration: `${RING.secs}s`, animationDelay: `${delay}s`, ['--sonar-peak' as string]: opacity, visibility: d ? 'visible' : 'hidden' }}>
      <circle cx="50" cy="50" r="49" fill="none" stroke={RING.color} strokeWidth={RING.stroke} vectorEffect="non-scaling-stroke" />
    </svg>
  )
  return (
    <div ref={box} key={id} data-sonar-ring className="sonar-box pointer-events-none absolute inset-0 overflow-hidden" aria-hidden>
      {ring(0, 0.8)}
      {ring(RING.trailAt, RING.trailOpacity)}
    </div>
  )
}
