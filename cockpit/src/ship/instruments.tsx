// The Ship's instruments (theseus-logs): a Victorian bridge's brass, lit by neon, each one reading live health.
// SVG, so they stay crisp at any size; every gradient and texture is drawn here, with no image files. The Ship's console
// keeps two (the owner's C3, theseus-hnof.2): the engine telegraph and the nixie tubes. The compass (the live profile)
// and the chronometer (the uptime) retired to the top bar's profile chip and UP; the gate and the fuel gauge, to the
// watch. The dial kit (Dial, Ticks, Needle, Engraved) also draws the speed wall's and the money views' dials.
import { useId, type ReactNode } from 'react'
import { cn } from '@/lib/format'

const GOLD = '#d6a548'
const IVORY = '#efe3c8'
const CYAN = '#22d3ee'
const AMBER = '#fbbf24'

/** A round brass instrument: bezel, rivets, a night-glass face, engraved ticks, and the glass's glare. */
export function Dial({ children, title, label, sub, glow }: { children: (ids: { face: string }) => ReactNode; title: string; label: string; sub?: ReactNode; glow?: string }) {
  const id = useId().replace(/:/g, '')
  const brass = `b${id}`
  const face = `f${id}`
  const glare = `g${id}`
  const rim = `r${id}`
  return (
    <figure className="ship-instrument flex flex-col items-center" title={title}>
      <svg className="ship-dial" viewBox="0 0 120 120" role="img" aria-label={title}>
        <defs>
          <linearGradient id={brass} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#f3d9a4" />
            <stop offset="0.28" stopColor="#c9a467" />
            <stop offset="0.55" stopColor="#8c6a3c" />
            <stop offset="0.78" stopColor="#b08d57" />
            <stop offset="1" stopColor="#5b4325" />
          </linearGradient>
          <linearGradient id={rim} x1="1" y1="1" x2="0" y2="0">
            <stop offset="0" stopColor="#f3d9a4" />
            <stop offset="0.5" stopColor="#6e5230" />
            <stop offset="1" stopColor="#2c2010" />
          </linearGradient>
          <radialGradient id={face} cx="0.5" cy="0.42" r="0.62">
            <stop offset="0" stopColor="#10243a" />
            <stop offset="0.7" stopColor="#081423" />
            <stop offset="1" stopColor="#040b15" />
          </radialGradient>
          <linearGradient id={glare} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#ffffff" stopOpacity="0.16" />
            <stop offset="1" stopColor="#ffffff" stopOpacity="0" />
          </linearGradient>
        </defs>
        <circle cx="60" cy="60" r="58.5" fill={`url(#${brass})`} />
        <circle cx="60" cy="60" r="53.5" fill={`url(#${rim})`} />
        <circle cx="60" cy="60" r="51" fill={`url(#${face})`} />
        {glow && <circle cx="60" cy="60" r="50" fill="none" stroke={glow} strokeOpacity="0.35" strokeWidth="1.2" style={{ filter: `drop-shadow(0 0 3px ${glow})` }} />}
        {Array.from({ length: 8 }, (_, i) => {
          const a = (i / 8) * Math.PI * 2 + Math.PI / 8
          const x = 60 + Math.cos(a) * 56
          const y = 60 + Math.sin(a) * 56
          return (
            <g key={i}>
              <circle cx={x} cy={y} r="1.9" fill="#4a3519" />
              <circle cx={x - 0.4} cy={y - 0.4} r="1.3" fill="#f3d9a4" opacity="0.85" />
            </g>
          )
        })}
        {children({ face })}
        <path d="M 22 44 A 42 42 0 0 1 98 44 Q 60 30 22 44 Z" fill={`url(#${glare})`} />
      </svg>
      <figcaption className="mt-0.5 flex flex-col items-center leading-tight">
        <span className="ship-engraved text-[10px]">{label}</span>
        {sub && <span className="num max-w-[132px] truncate text-[10.5px] text-ink-dim">{sub}</span>}
      </figcaption>
    </figure>
  )
}

export const polar = (deg: number, r: number) => {
  const a = ((deg - 90) * Math.PI) / 180
  return [60 + Math.cos(a) * r, 60 + Math.sin(a) * r] as const
}

/** Ticks along an arc from `a0` to `a1` degrees (0 is up, clockwise). */
export function Ticks({ a0, a1, n, major = 1, r = 46, color = GOLD }: { a0: number; a1: number; n: number; major?: number; r?: number; color?: string }) {
  return (
    <g stroke={color} strokeLinecap="round">
      {Array.from({ length: n + 1 }, (_, i) => {
        const a = a0 + ((a1 - a0) * i) / n
        const big = i % major === 0
        const [x1, y1] = polar(a, r)
        const [x2, y2] = polar(a, r - (big ? 6 : 3))
        return <line key={i} x1={x1} y1={y1} x2={x2} y2={y2} strokeWidth={big ? 1.3 : 0.7} opacity={big ? 0.95 : 0.6} />
      })}
    </g>
  )
}

export function Needle({ deg, color = IVORY, len = 40, shadow }: { deg: number; color?: string; len?: number; shadow?: number }) {
  return (
    <g>
      {shadow !== undefined && (
        <g transform={`rotate(${shadow} 60 60)`} opacity="0.38">
          <path d={`M 58.6 64 L 60 ${60 - len} L 61.4 64 Z`} fill={AMBER} />
        </g>
      )}
      <g transform={`rotate(${deg} 60 60)`} style={{ transition: 'transform 900ms cubic-bezier(.3,1.4,.5,1)' }}>
        <path d={`M 58.3 66 L 60 ${60 - len} L 61.7 66 Z`} fill={color} style={{ filter: `drop-shadow(0 0 2px ${color})` }} />
        <path d="M 58.8 66 L 60 74 L 61.2 66 Z" fill="#8c6a3c" />
      </g>
      <circle cx="60" cy="60" r="4.6" fill="#8c6a3c" />
      <circle cx="59.2" cy="59.2" r="2.6" fill="#f3d9a4" />
    </g>
  )
}

export function Engraved({ x, y, children, size = 6.4, color = GOLD, anchor = 'middle', weight = 600 }: { x: number; y: number; children: ReactNode; size?: number; color?: string; anchor?: 'middle' | 'start' | 'end'; weight?: number }) {
  return (
    <text x={x} y={y} textAnchor={anchor} fontSize={size} fill={color} fontFamily="'Cinzel Variable', serif" fontWeight={weight} letterSpacing="0.6">
      {children}
    </text>
  )
}

// ---------------------------------------------------------------- the engine telegraph: the kernel

const ORDERS = ['STOP', 'SLOW', 'HALF', 'FULL'] as const

export function EngineTelegraph({ accepting, running, ceiling, held }: { accepting?: boolean; running: number; ceiling: number; held: number }) {
  const order = accepting === false ? 0 : running === 0 ? 1 : running * 2 <= ceiling ? 2 : 3
  const deg = -72 + order * 48
  const tone = order === 0 ? AMBER : order === 1 ? GOLD : CYAN
  return (
    <Dial title={`The kernel: ${accepting === false ? 'holding new turns (STOP)' : 'accepting'}; ${running} of ${ceiling} executions running${held ? `, ${held} turns held` : ''}. SLOW is idle, HALF up to half the ceiling, FULL past it.`}
      label="Engine" sub={`${accepting === false ? 'holding' : 'accepting'} · ${running} of ${ceiling}${held ? ` · ${held} held` : ''}`} glow={order >= 2 ? CYAN : order === 0 ? AMBER : undefined}>
      {() => (
        <g>
          {ORDERS.map((o, i) => {
            const a0 = -96 + i * 48
            const on = i === order
            const [x, y] = polar(a0 + 24, 33)
            return (
              <g key={o}>
                <path d={sector(a0 + 1, a0 + 47, 25, 46)} fill={on ? tone : '#0d1d30'} fillOpacity={on ? 0.28 : 1} stroke={GOLD} strokeOpacity="0.5" strokeWidth="0.6" />
                <Engraved x={x} y={y + 2.4} size={7} color={on ? tone : '#9c8c66'} weight={700}>{o}</Engraved>
              </g>
            )
          })}
          <Engraved x={60} y={84} size={5.6} color="#b8a77f">AHEAD</Engraved>
          <g transform={`rotate(${deg + 24} 60 60)`} style={{ transition: 'transform 700ms cubic-bezier(.3,1.3,.5,1)' }}>
            <rect x="58.4" y="18" width="3.2" height="44" rx="1.4" fill="#c9a467" />
            <rect x="56" y="15" width="8" height="6" rx="2" fill="#f3d9a4" />
          </g>
          <circle cx="60" cy="60" r="6" fill="#8c6a3c" />
          <circle cx="59" cy="59" r="3.4" fill="#f3d9a4" />
        </g>
      )}
    </Dial>
  )
}

// ---------------------------------------------------------------- nixie tubes: tokens a minute

export function Nixie({ value, digits = 6, label, title, className }: { value: number | null; digits?: number; label: string; title: string; className?: string }) {
  const text = value === null ? '-'.repeat(digits) : String(Math.max(0, Math.round(value))).padStart(digits, '0').slice(-digits)
  const lead = value === null ? digits : text.length - String(Math.max(0, Math.round(value))).length
  return (
    <figure className={cn('ship-instrument flex flex-col items-center', className)} title={title}>
      <div className="nixie-bank">
        {text.split('').map((d, i) => (
          <span key={i} className={cn('nixie-tube', i < lead && 'nixie-off')}>
            <span className="nixie-ghost" aria-hidden>8</span>
            <span className="nixie-digit">{d}</span>
          </span>
        ))}
      </div>
      <figcaption className="mt-1 flex flex-col items-center leading-tight">
        <span className="ship-engraved text-[10px]">{label}</span>
      </figcaption>
    </figure>
  )
}

// ---------------------------------------------------------------- the sea gauge: the work now (the living sea)

/** The sea's state (theseus-hnof.2, the owner's C5): three rows of the logo's waves behind night glass, swinging as high
 *  as the sea runs, and the sea's state in words. It moves only when the sea's height changes: the chart's swell is the
 *  one that rolls. */
export function SeaGauge({ height, word, title }: { height: number; word: string; title: string }) {
  const id = useId().replace(/:/g, '')
  const amp = 0.4 + height * 4.6
  const row = (y: number, k: number) => {
    let d = ''
    for (let x = 0; x <= 96; x += 3) {
      const yy = y + Math.sin(x / 7.5 + k * 1.9) * amp * (0.75 + 0.25 * Math.sin(x / 23 + k))
      d += `${x === 0 ? 'M' : 'L'} ${x + 2} ${yy.toFixed(2)} `
    }
    return d
  }
  const tone = height > 0 ? CYAN : '#5b6b78'
  return (
    <figure className="ship-instrument ship-sea flex flex-col items-center" title={title}>
      <svg className="ship-sea-gauge" viewBox="0 0 100 52" role="img" aria-label={title}>
        <defs>
          <linearGradient id={`sb${id}`} x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#f3d9a4" />
            <stop offset="0.35" stopColor="#b08d57" />
            <stop offset="0.7" stopColor="#5b4325" />
            <stop offset="1" stopColor="#c9a467" />
          </linearGradient>
          <linearGradient id={`sg${id}`} x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" stopColor="#0d2238" />
            <stop offset="1" stopColor="#040b15" />
          </linearGradient>
          <clipPath id={`sc${id}`}><rect x="4" y="4" width="92" height="44" rx="6" /></clipPath>
        </defs>
        <rect x="1" y="1" width="98" height="50" rx="8" fill={`url(#sb${id})`} />
        <rect x="4" y="4" width="92" height="44" rx="6" fill={`url(#sg${id})`} />
        <g clipPath={`url(#sc${id})`} fill="none" strokeLinecap="round">
          {[15, 26, 37].map((y, k) => (
            <path key={k} d={row(y, k)} stroke={tone} strokeOpacity={0.35 + 0.5 * height * (k === 1 ? 1 : 0.8)} strokeWidth={k === 1 ? 1.5 : 1.1}
              style={height > 0 ? { filter: `drop-shadow(0 0 ${1 + height * 2}px ${CYAN})` } : undefined} />
          ))}
        </g>
        <path d="M 8 9 Q 50 2 92 9" fill="none" stroke="#ffffff" strokeOpacity="0.12" strokeWidth="3" />
      </svg>
      <figcaption className="mt-1 flex flex-col items-center leading-tight">
        <span className="ship-engraved text-[10px]">The sea</span>
        <span className="num text-[10.5px] text-ink-dim">{word}</span>
      </figcaption>
    </figure>
  )
}

// ---------------------------------------------------------------- geometry

export function arc(a0: number, a1: number, r: number): string {
  const [x0, y0] = polar(a0, r)
  const [x1, y1] = polar(a1, r)
  return `M ${x0} ${y0} A ${r} ${r} 0 ${a1 - a0 > 180 ? 1 : 0} 1 ${x1} ${y1}`
}

function sector(a0: number, a1: number, r0: number, r1: number): string {
  const [x0, y0] = polar(a0, r1)
  const [x1, y1] = polar(a1, r1)
  const [x2, y2] = polar(a1, r0)
  const [x3, y3] = polar(a0, r0)
  return `M ${x0} ${y0} A ${r1} ${r1} 0 0 1 ${x1} ${y1} L ${x2} ${y2} A ${r0} ${r0} 0 0 0 ${x3} ${y3} Z`
}
