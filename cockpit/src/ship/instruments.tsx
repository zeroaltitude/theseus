// The Ship's instruments (theseus-logs): a Victorian bridge's brass, lit by neon, each one reading live health.
// SVG, so they stay crisp at any size; every gradient and texture is drawn here, with no image files.
import { useId, type ReactNode } from 'react'
import { cn } from '@/lib/format'

const GOLD = '#d6a548'
const IVORY = '#efe3c8'
const CYAN = '#22d3ee'
const AMBER = '#fbbf24'
const ROSE = '#fb7185'

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

// ---------------------------------------------------------------- the compass: the live profile

export function Compass({ live, profiles, model }: { live?: string; profiles: string[]; model?: string }) {
  const names = profiles.length ? profiles : live ? [live] : []
  const at = (i: number) => (names.length <= 1 ? 0 : (360 / names.length) * i)
  const li = Math.max(0, names.indexOf(live ?? ''))
  return (
    <Dial title={`The live profile: ${live ?? '—'}${model ? ` (${model})` : ''}. Every profile is a bearing on the card; the needle points to the live one.`}
      label="Compass" sub={live ? `${live} · ${model ?? ''}` : '—'}>
      {() => (
        <g>
          <Ticks a0={0} a1={360} n={32} major={8} r={47} />
          {/* The rose: four long points and four short, in old gold and ivory. */}
          <g opacity="0.55">
            {[0, 90, 180, 270].map((a) => {
              const [x, y] = polar(a, 34)
              const [lx, ly] = polar(a - 90, 5)
              const [rx, ry] = polar(a + 90, 5)
              return <path key={a} d={`M ${lx} ${ly} L ${x} ${y} L ${rx} ${ry} Z`} fill={a === 0 ? GOLD : '#6e5230'} />
            })}
            {[45, 135, 225, 315].map((a) => {
              const [x, y] = polar(a, 22)
              const [lx, ly] = polar(a - 90, 3.4)
              const [rx, ry] = polar(a + 90, 3.4)
              return <path key={a} d={`M ${lx} ${ly} L ${x} ${y} L ${rx} ${ry} Z`} fill="#4a3a22" />
            })}
          </g>
          {names.map((n, i) => {
            const [x, y] = polar(at(i), 36)
            const on = i === li
            return <Engraved key={n} x={x} y={y + 2.2} size={on ? 7.2 : 6} color={on ? CYAN : '#b8a77f'}>{n.slice(0, 8).toUpperCase()}</Engraved>
          })}
          <Needle deg={at(li)} color={CYAN} len={30} />
        </g>
      )}
    </Dial>
  )
}

// ---------------------------------------------------------------- the pressure gauge: the gate

export function PressureGauge({ approvals, holds, held }: { approvals: number; holds: number; held: number }) {
  const v = approvals + holds
  const max = Math.max(8, v)
  const deg = -135 + (Math.min(v, max) / max) * 270
  const tone = v === 0 ? CYAN : v >= 5 ? ROSE : AMBER
  return (
    <Dial title={`The gate's pressure: ${approvals} approval${approvals === 1 ? '' : 's'} waiting for you, and ${holds} session${holds === 1 ? '' : 's'} holding external text${held ? `, ${held} turn${held === 1 ? '' : 's'} held by the kernel` : ''}.`}
      label="Gate" sub={`${approvals} waiting · ${holds} held`} glow={v ? tone : undefined}>
      {() => (
        <g>
          <path d={arc(-135, 135, 44)} fill="none" stroke="#2b3d52" strokeWidth="4" />
          <path d={arc(-135 + (5 / max) * 270, 135, 44)} fill="none" stroke={ROSE} strokeOpacity="0.55" strokeWidth="4" />
          <Ticks a0={-135} a1={135} n={max} major={1} r={41} />
          {Array.from({ length: max + 1 }, (_, i) => i).filter((i) => i % Math.ceil(max / 8) === 0).map((i) => {
            const [x, y] = polar(-135 + (i / max) * 270, 29)
            return <Engraved key={i} x={x} y={y + 2.3} size={7}>{String(i)}</Engraved>
          })}
          <Engraved x={60} y={86} size={6} color="#b8a77f">PRESSURE</Engraved>
          <Needle deg={deg} color={tone} len={38} />
        </g>
      )}
    </Dial>
  )
}

// ---------------------------------------------------------------- the fuel gauge: money

export function FuelGauge({ spent, reserved, limit, of, total }: { spent: number; reserved: number; limit?: number; of: string; total: number }) {
  const lim = limit && limit > 0 ? limit : undefined
  const left = lim ? Math.max(0, 1 - spent / lim) : 1
  const leftAfter = lim ? Math.max(0, 1 - (spent + reserved) / lim) : 1
  const deg = -90 + left * 180
  const shadow = reserved > 0 ? -90 + leftAfter * 180 : undefined
  const tone = left < 0.1 ? ROSE : left < 0.3 ? AMBER : GOLD
  return (
    <Dial title={`Fuel for ${of}: $${spent.toFixed(4)} spent of ${lim ? `$${lim.toFixed(2)}` : 'no limit'}${reserved ? `, $${reserved.toFixed(4)} reserved by calls in flight (the needle's shadow)` : ''}. Every session together has spent $${total.toFixed(4)}.`}
      label="Fuel" sub={<>{of} · ${spent.toFixed(spent < 1 ? 3 : 2)}{lim ? ` / $${lim.toFixed(0)}` : ''}</>}>
      {() => (
        <g>
          <path d={arc(-90, 90, 44)} fill="none" stroke="#2b3d52" strokeWidth="4" />
          <path d={arc(-90, -72, 44)} fill="none" stroke={ROSE} strokeOpacity="0.7" strokeWidth="4" />
          <Ticks a0={-90} a1={90} n={8} major={2} r={41} />
          <Engraved x={polar(-90, 31)[0] + 3} y={polar(-90, 31)[1] + 2.5} size={8} weight={700}>E</Engraved>
          <Engraved x={polar(90, 31)[0] - 3} y={polar(90, 31)[1] + 2.5} size={8} weight={700}>F</Engraved>
          <Engraved x={60} y={38} size={6.2} color="#b8a77f">½</Engraved>
          <Engraved x={60} y={84} size={5.6} color="#b8a77f">{`ALL $${total.toFixed(total < 10 ? 2 : 0)}`}</Engraved>
          <Needle deg={deg} color={tone} len={38} shadow={shadow} />
        </g>
      )}
    </Dial>
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
      label="Engine" sub={`${accepting === false ? 'holding' : 'accepting'} · ${running}/${ceiling}`} glow={order >= 2 ? CYAN : order === 0 ? AMBER : undefined}>
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

// ---------------------------------------------------------------- the ship's clock: uptime

const ROMAN = ['XII', 'I', 'II', 'III', 'IV', 'V', 'VI', 'VII', 'VIII', 'IX', 'X', 'XI']

export function ShipsClock({ uptime, version, down }: { uptime?: number; version?: string; down?: boolean }) {
  const s = uptime ?? 0
  const days = Math.floor(s / 86400)
  const h = (s / 3600) % 12
  const m = (s / 60) % 60
  const words = uptime === undefined ? '—' : days ? `${days}d ${Math.floor((s % 86400) / 3600)}h` : s >= 3600 ? `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m` : `${Math.floor(s / 60)}m ${Math.floor(s % 60)}s`
  return (
    <Dial title={down ? 'The daemon was down at this moment: its last start had a stop after it.' : `The daemon's uptime: ${words}${version ? `, version ${version}` : ''}. The hands keep the time it has been up.`}
      label="Chronometer" sub={down ? 'down then' : `up ${words}`} glow={down ? ROSE : undefined}>
      {() => (
        <g>
          <Ticks a0={0} a1={360} n={60} major={5} r={47} />
          {ROMAN.map((r, i) => {
            const [x, y] = polar(i * 30, 35)
            return <Engraved key={r} x={x} y={y + 2.2} size={6.2} color="#c9b98f">{r}</Engraved>
          })}
          {days > 0 && (
            <g>
              <rect x="70" y="56" width="14" height="8" rx="1" fill="#06101c" stroke={GOLD} strokeWidth="0.5" />
              <text x="77" y="62.3" textAnchor="middle" fontSize="6" fill={IVORY} fontFamily="'JetBrains Mono Variable', monospace">{days}d</text>
            </g>
          )}
          <g transform={`rotate(${h * 30} 60 60)`}><path d="M 58 62 L 60 32 L 62 62 Z" fill={IVORY} /></g>
          <g transform={`rotate(${m * 6} 60 60)`}><path d="M 58.8 63 L 60 18 L 61.2 63 Z" fill={GOLD} style={{ filter: `drop-shadow(0 0 1.5px ${GOLD})` }} /></g>
          <circle cx="60" cy="60" r="3.6" fill="#8c6a3c" />
          <circle cx="59.4" cy="59.4" r="2" fill="#f3d9a4" />
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
