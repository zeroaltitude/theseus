// The tour (theseus-hnof): on a first visit, and on ?, the Ship walks the operator through what its shapes are, one at
// a time, pointing at a real one on the chart (a harbour, a ship, a bench, an oar, a lantern) and flying the camera to it.
// Its words are the one-page "what you're looking at". It reads the chart as it stands: a step whose shape the fleet has
// none of says so and points at nothing.
//
// What's new (theseus-hnof.2, the owner's C6): after an update that adds something this browser hasn't seen, the same
// card flies to just the new things (`news.ts`), once, and can be skipped. Either card stands beside its shape and off
// the Ship's instruments (`placement.ts`).
import { useEffect, useMemo, useRef, useState } from 'react'
import type { ShipEngine } from './engine'
import type { ShipModel } from './model'
import type { NewsItem } from './news'
import { instrumentRects } from './instrumentRects'
import { placeCard } from './placement'
import { TOUR_TEXT } from './tourText'

type Anchor =
  | { kind: 'formation'; i: number }
  | { kind: 'vessel'; i: number }
  | { kind: 'bench'; i: number }
  | { kind: 'oar'; light: number }
  | { kind: 'dom'; selector: string }
  | { kind: 'none' }

interface Step { title: string; body: string; anchor: Anchor; fly?: (e: ShipEngine) => void }

/** The ship the tour shows: the one towing tasks, else the one with the most turns. */
function hostOf(m: ShipModel): number {
  const order = m.vessels.map((v, i) => ({ v, i })).sort((a, b) => b.v.benches.length - a.v.benches.length)
  return (order.find(({ v }) => m.vessels.some((x) => x.parentId === v.id)) ?? order[0])?.i ?? -1
}

function buildSteps(m: ShipModel): Step[] {
  const vi = hostOf(m)
  const v = vi >= 0 ? m.vessels[vi] : undefined
  const fi = v ? m.formations.findIndex((f) => f.members.includes(vi)) : m.formations.length ? 0 : -1
  const bench = v && v.benches.length ? v.benches[v.benches.length - 1] : -1
  // An oar worth showing: a failed one, else any, on that ship.
  const calls = m.lights.map((l, i) => ({ l, i })).filter(({ l }) => l.kind === 'call' && l.vessel === vi)
  const oar = (calls.find(({ l }) => l.failed) ?? calls[calls.length - 1])?.i ?? -1
  const lantern = m.vessels.findIndex((x) => x.rig === 'lantern')
  const t = TOUR_TEXT
  return [
    { ...t[0], anchor: fi >= 0 ? { kind: 'formation', i: fi } : { kind: 'none' }, fly: (e) => e.fit() },
    { ...t[1], anchor: vi >= 0 ? { kind: 'vessel', i: vi } : { kind: 'none' }, fly: (e) => { if (vi >= 0) e.flyToVessel(vi) } },
    { ...t[2], anchor: bench >= 0 ? { kind: 'bench', i: bench } : { kind: 'none' }, fly: (e) => { if (vi >= 0) e.flyToVessel(vi) } },
    { ...t[3], anchor: oar >= 0 ? { kind: 'oar', light: oar } : { kind: 'none' }, fly: (e) => { const b = oar >= 0 ? m.lights[oar].bench : -1; if (b >= 0) e.flyToBench(b) } },
    { ...t[4], anchor: lantern >= 0 ? { kind: 'vessel', i: lantern } : vi >= 0 ? { kind: 'vessel', i: vi } : { kind: 'none' }, fly: (e) => e.fit() },
    { ...t[5], anchor: { kind: 'dom', selector: '.ship-watch-slot' } },
    { ...t[6], anchor: { kind: 'dom', selector: '.ship-depth' } },
  ]
}

/** What's new: each new thing's stop, the fleet in view behind it. */
function newsSteps(m: ShipModel, items: NewsItem[]): Step[] {
  const vi = hostOf(m)
  return items.map((n) => ({
    title: n.title, body: n.body,
    anchor: n.anchor.kind === 'dom' ? { kind: 'dom', selector: n.anchor.selector } : vi >= 0 ? { kind: 'vessel', i: vi } : { kind: 'none' },
    fly: (e) => e.fit(),
  }))
}

/** Where an anchor is on screen now (CSS pixels in the Ship's box), or null. */
function where(e: ShipEngine, m: ShipModel, a: Anchor, host: HTMLElement): { x: number; y: number } | null {
  if (a.kind === 'none') return null
  if (a.kind === 'dom') {
    const el = host.querySelector(a.selector) as HTMLElement | null
    if (!el) return null
    const r = el.getBoundingClientRect()
    const o = host.getBoundingClientRect()
    // Its edge that faces the open chart: the left of a right-hand instrument, the top of one at the foot.
    const foot = r.top - o.top > o.height * 0.6
    return foot ? { x: r.left - o.left + r.width / 2, y: r.top - o.top - 4 } : { x: r.left - o.left - 4, y: r.top - o.top + Math.min(r.height / 2, 60) }
  }
  if (a.kind === 'formation') {
    const f = m.formations[a.i]
    if (!f) return null
    const p = e.project(f.x, 0, f.z - f.radius)
    return p.on ? { x: p.x, y: p.y } : null
  }
  if (a.kind === 'vessel') {
    const now = e.vesselNow(a.i)
    const p = e.project(now.x, 0, now.z)
    return p.on ? { x: p.x, y: p.y } : null
  }
  if (a.kind === 'bench') {
    const b = m.benches[a.i]
    if (!b) return null
    const now = e.vesselNow(b.vessel)
    const p = e.project(now.x + b.x * Math.cos(now.heading), 0.2, now.z + b.x * Math.sin(now.heading))
    return p.on ? { x: p.x, y: p.y } : null
  }
  // An oar: its blade, where its result is (or will be).
  const l = m.lights[a.light]
  if (!l) return null
  const r = m.lights.find((q) => q.kind === 'result' && q.toolUseId && q.toolUseId === l.toolUseId)
  const v = m.vessels[l.vessel]
  const now = e.vesselNow(l.vessel)
  const side = Math.sign(l.lz) || 1
  const x = r ? r.lx : l.lx - Math.max(1.1, v.beam * 0.55) * 0.42
  const z = r ? r.lz : side * (v.beam * 0.5 + Math.max(1.1, v.beam * 0.55))
  const c = Math.cos(now.heading)
  const s = Math.sin(now.heading)
  const p = e.project(now.x + x * c - z * s, 0.2, now.z + x * s + z * c)
  return p.on ? { x: p.x, y: p.y } : null
}

export interface TourProps {
  engine: ShipEngine
  model: ShipModel
  host: HTMLElement
  /** The full tour, or what's new since this browser last looked. */
  news?: NewsItem[]
  onClose: () => void
}

export function Tour({ engine, model, host, news, onClose }: TourProps) {
  const steps = useMemo(() => (news ? newsSteps(model, news) : buildSteps(model)), [model, news])
  const [i, setI] = useState(0)
  const [at, setAt] = useState<{ x: number; y: number } | null>(null)
  const [pos, setPos] = useState<{ x: number; y: number; h: number } | null>(null)
  const card = useRef<HTMLDivElement>(null)
  const step = steps[Math.min(i, steps.length - 1)]
  // Fly to the step's shape when the step changes (not when the model does).
  const flown = useRef(-1)
  useEffect(() => {
    if (flown.current === i) return
    flown.current = i
    step.fly?.(engine)
  }, [i, step, engine])
  // Follow the anchor while the camera moves, and keep the card beside it, off the instruments.
  useEffect(() => {
    let raf = 0
    const tick = () => {
      const p = where(engine, model, step.anchor, host)
      setAt((q) => (p && q && Math.abs(p.x - q.x) < 0.5 && Math.abs(p.y - q.y) < 0.5 ? q : p))
      const el = card.current
      if (el) {
        const size = { w: el.offsetWidth, h: el.offsetHeight }
        const box = { w: host.clientWidth, h: host.clientHeight }
        const c = p ? placeCard(p, size, box, instrumentRects(host, 8, el), 56) : { x: box.w / 2 - size.w / 2, y: box.h / 2 - size.h / 2 }
        setPos((q) => (q && Math.abs(q.x - c.x) < 0.5 && Math.abs(q.y - c.y) < 0.5 && q.h === size.h ? q : { x: c.x, y: c.y, h: size.h }))
      }
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [engine, model, step, host])
  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose()
      if (e.key === 'ArrowRight' || e.key === 'Enter') setI((x) => Math.min(steps.length - 1, x + 1))
      if (e.key === 'ArrowLeft') setI((x) => Math.max(0, x - 1))
    }
    window.addEventListener('keydown', k)
    return () => window.removeEventListener('keydown', k)
  }, [steps.length, onClose])
  const cw = 360
  const left = pos?.x ?? host.clientWidth / 2 - cw / 2
  const top = pos?.y ?? host.clientHeight / 2 - 120
  const last = i === steps.length - 1
  const h = pos?.h ?? 180
  // The leader runs from the shape to the card's nearest edge.
  const lx = at ? (at.x < left ? left : at.x > left + cw ? left + cw : at.x) : 0
  const ly = at ? (at.y < top ? top : at.y > top + h ? top + h : at.y) : 0
  return (
    <div className="pointer-events-none absolute inset-0 z-[35]">
      {at && (
        <svg className="absolute inset-0 h-full w-full" aria-hidden>
          <line x1={at.x} y1={at.y} x2={lx} y2={ly} stroke="#22d3ee" strokeOpacity="0.7" strokeWidth="1.2" strokeDasharray="4 3" />
        </svg>
      )}
      {at && <div className="ship-tour-ring" style={{ left: at.x, top: at.y }} />}
      <div ref={card} data-ship-ui className="brass-card pointer-events-auto absolute" style={{ left, top, width: cw, visibility: pos ? 'visible' : 'hidden' }}
        role="dialog" aria-label={news ? 'What’s new on the Ship' : 'The Ship’s tour'}>
        <div className="flex items-baseline justify-between">
          <span className="ship-engraved text-[10px]">{news ? 'What’s new' : 'The tour'} · {i + 1} of {steps.length}</span>
          <button onClick={onClose} className="text-[11px] text-ink-faint hover:text-ink" title={news ? 'Skip what’s new (Esc): it won’t show again' : 'Close the tour (Esc)'}>skip</button>
        </div>
        <h3 className="mt-1 font-display text-[16px] font-semibold text-ivory">{step.title}</h3>
        <p className="mt-1 text-[12.5px] leading-snug text-ink-dim">{step.body}</p>
        {step.anchor.kind === 'none' && <p className="mt-1 text-[11px] text-ink-faint">The fleet has none of these yet.</p>}
        <div className="mt-2.5 flex items-center gap-1.5">
          {steps.map((_, k) => <i key={k} className={`h-1.5 w-1.5 rounded-full ${k === i ? 'bg-neon' : 'bg-[#5b4325]'}`} />)}
          <span className="flex-1" />
          {news && last && <span className="mr-1 text-[10.5px] text-ink-faint">? for the whole tour</span>}
          {i > 0 && <button className="brass-button" onClick={() => setI(i - 1)}>Back</button>}
          <button className="brass-button brass-button-on" onClick={() => (last ? onClose() : setI(i + 1))}>{last ? 'Done' : 'Next'}</button>
        </div>
      </div>
    </div>
  )
}
