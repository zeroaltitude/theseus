// Brass and planks (theseus-logs): the README's motifs, for every view. The plank strip is the README's divider: worn
// ivory planks, and a gold new one for each fact that arrives (it fills as a loading bar first). The coin is the
// README's logo, drawn inline.
import { useId } from 'react'

const PLANKS = 56

/** The strip's planks (seeded widths, the same on every page) and the order new ones go in: scattered along the
 *  hull, as the logo's are. */
const layout = (() => {
  let s = 0x9e3779b1
  const rnd = () => { s = (Math.imul(s ^ (s >>> 15), 0x2c1b3c6d) + 0x6d2b79f5) >>> 0; return s / 4294967296 }
  const widths = Array.from({ length: PLANKS }, () => 0.6 + rnd() * 0.9)
  const total = widths.reduce((a, b) => a + b, 0)
  let x = 0
  const planks = widths.map((w) => { const p = { x: (x / total) * 1000, w: (w / total) * 1000 }; x += w; return p })
  const keys = widths.map(() => rnd())
  const order = Array.from({ length: PLANKS }, (_, i) => i).sort((a, b) => keys[a] - keys[b])
  return { planks, order }
})()

/** A row of planks. `progress` (0 to 1) lays them left to right; `lit` gold planks are facts of the last minute. */
export function PlankStrip({ progress = 1, lit = 0, height = 5 }: { progress?: number; lit?: number; height?: number }) {
  const laid = Math.round(progress * PLANKS)
  const gold = new Set(layout.order.slice(0, Math.min(lit, 9)))
  const newest = lit > 0 ? layout.order[Math.min(lit, 9) - 1] : -1
  return (
    <svg className="plank-strip block w-full" height={height} viewBox={`0 0 1000 ${height}`} preserveAspectRatio="none" aria-hidden>
      {layout.planks.map((p, i) => {
        const on = i < laid
        const g = on && gold.has(i)
        return (
          <rect key={i} x={p.x + 0.6} y={0.5} width={Math.max(0.5, p.w - 1.2)} height={height - 1} rx={1}
            className={g ? (i === newest ? 'plank plank-gold plank-new' : 'plank plank-gold') : on ? 'plank' : 'plank plank-unlaid'} />
        )
      })}
    </svg>
  )
}

/** The README's logo: a Greek ship on an old coin, at night; a few planks new and gold. */
export function Coin({ size = 36, className }: { size?: number; className?: string }) {
  const id = useId().replace(/:/g, '')
  const field = `cf${id}`
  const hull = `ch${id}`
  return (
    <svg width={size} height={size} viewBox="0 0 512 512" className={className} role="img" aria-label="Theseus">
      <defs>
        <clipPath id={field}><circle cx="256" cy="256" r="225" /></clipPath>
        <clipPath id={hull}><path d="M150 286 Q277 300 404 284 C410 300 414 316 420 326 L436 332 L416 338 C380 350 220 352 168 342 C148 336 140 312 150 286 Z" /></clipPath>
      </defs>
      <circle cx="256" cy="256" r="252" fill="#13314d" />
      <circle cx="256" cy="256" r="241" fill="none" stroke="#d6a548" strokeWidth="10" />
      <circle cx="256" cy="256" r="228" fill="none" stroke="#d6a548" strokeWidth="6" strokeDasharray="0.1 14" strokeLinecap="round" />
      <g clipPath={`url(#${field})`}>
        <g fill="#e9c97f">
          <circle cx="150" cy="122" r="5" /><circle cx="362" cy="104" r="4" /><circle cx="404" cy="172" r="3.4" /><circle cx="116" cy="190" r="3.6" />
        </g>
        <rect x="0" y="346" width="512" height="166" fill="#173c5f" />
        <path d="M150 286 Q277 300 404 284 C410 300 414 316 420 326 L436 332 L416 338 C380 350 220 352 168 342 C148 336 140 312 150 286 Z" fill="#efe3c8" />
        <g clipPath={`url(#${hull})`}>
          <rect x="245" y="250" width="55" height="56" fill="#e2ae4f" />
          <rect x="330" y="306" width="55" height="44" fill="#e2ae4f" />
          <rect x="190" y="320" width="55" height="40" fill="#e2ae4f" />
        </g>
        <path d="M152 290 C132 276 116 254 120 232 C123 215 139 208 147 218" fill="none" stroke="#efe3c8" strokeWidth="14" strokeLinecap="round" />
        <line x1="262" y1="294" x2="262" y2="136" stroke="#efe3c8" strokeWidth="11" strokeLinecap="round" />
        <line x1="192" y1="156" x2="332" y2="156" stroke="#efe3c8" strokeWidth="10" strokeLinecap="round" />
        <path d="M200 159 L324 159 L319 264 Q262 279 205 264 Z" fill="#efe3c8" />
        <g stroke="#13314d" strokeWidth="11" strokeLinecap="round" fill="none">
          <ellipse cx="262" cy="211" rx="26" ry="31" /><line x1="250" y1="211" x2="274" y2="211" />
        </g>
        <path d="M0 372 q16 -12 32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0" fill="none" stroke="#5aa0b4" strokeWidth="9" />
        <path d="M-16 414 q16 -12 32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0 t32 0" fill="none" stroke="#3d7d93" strokeWidth="8" strokeOpacity="0.7" />
      </g>
    </svg>
  )
}
