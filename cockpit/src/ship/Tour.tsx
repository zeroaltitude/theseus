// The tour (theseus-hnof): on a first visit, and on ?, the Ship walks the operator through what its shapes are, one at
// a time, pointing at a real one on the chart (a harbour, a ship, a bench, an oar, a lantern) and flying the camera to it.
// Its words are the one-page "what you're looking at". It reads the chart as it stands: a step whose shape the fleet has
// none of says so and points at nothing.
import { useEffect, useMemo, useRef, useState } from 'react'
import type { ShipEngine } from './engine'
import type { ShipModel } from './model'
import { TOUR_TEXT } from './tourText'

type Anchor =
  | { kind: 'formation'; i: number }
  | { kind: 'vessel'; i: number }
  | { kind: 'bench'; i: number }
  | { kind: 'oar'; light: number }
  | { kind: 'dom'; selector: string }
  | { kind: 'none' }

interface Step { title: string; body: string; anchor: Anchor; fly?: (e: ShipEngine) => void }

function buildSteps(m: ShipModel): Step[] {
  // The ship to show: the one towing tasks, else the one with the most turns.
  const order = m.vessels.map((v, i) => ({ v, i })).sort((a, b) => b.v.benches.length - a.v.benches.length)
  const host = order.find(({ v }) => m.vessels.some((x) => x.parentId === v.id)) ?? order[0]
  const vi = host?.i ?? -1
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

/** Where an anchor is on screen now (CSS pixels in the Ship's box), or null. */
function where(e: ShipEngine, m: ShipModel, a: Anchor, host: HTMLElement): { x: number; y: number } | null {
  if (a.kind === 'none') return null
  if (a.kind === 'dom') {
    const el = host.querySelector(a.selector) as HTMLElement | null
    if (!el) return null
    const r = el.getBoundingClientRect()
    const o = host.getBoundingClientRect()
    return { x: r.left - o.left - 4, y: r.top - o.top + Math.min(r.height / 2, 60) }
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

export function Tour({ engine, model, host, onClose }: { engine: ShipEngine; model: ShipModel; host: HTMLElement; onClose: () => void }) {
  const steps = useMemo(() => buildSteps(model), [model])
  const [i, setI] = useState(0)
  const [at, setAt] = useState<{ x: number; y: number } | null>(null)
  const step = steps[Math.min(i, steps.length - 1)]
  // Fly to the step's shape when the step changes (not when the model does).
  const flown = useRef(-1)
  useEffect(() => {
    if (flown.current === i) return
    flown.current = i
    step.fly?.(engine)
  }, [i, step, engine])
  // Follow the anchor while the camera moves.
  useEffect(() => {
    let raf = 0
    const tick = () => {
      const p = where(engine, model, step.anchor, host)
      setAt((q) => (p && q && Math.abs(p.x - q.x) < 0.5 && Math.abs(p.y - q.y) < 0.5 ? q : p))
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
  const W = host.clientWidth
  const H = host.clientHeight
  const cw = 360
  // The card beside its shape, on the side with room; centred when there is none.
  const left = at ? (at.x > W / 2 ? Math.max(12, at.x - cw - 56) : Math.min(W - cw - 12, at.x + 56)) : W / 2 - cw / 2
  const top = at ? Math.max(70, Math.min(H - 250, at.y - 60)) : H / 2 - 120
  const last = i === steps.length - 1
  return (
    <div className="pointer-events-none absolute inset-0 z-[35]">
      {at && (
        <svg className="absolute inset-0 h-full w-full" aria-hidden>
          <line x1={at.x} y1={at.y} x2={left + (at.x > left ? cw : 0)} y2={top + 40} stroke="#22d3ee" strokeOpacity="0.7" strokeWidth="1.2" strokeDasharray="4 3" />
        </svg>
      )}
      {at && <div className="ship-tour-ring" style={{ left: at.x, top: at.y }} />}
      <div data-ship-ui className="brass-card pointer-events-auto absolute" style={{ left, top, width: cw }} role="dialog" aria-label="The Ship's tour">
        <div className="flex items-baseline justify-between">
          <span className="ship-engraved text-[10px]">The tour · {i + 1} of {steps.length}</span>
          <button onClick={onClose} className="text-[11px] text-ink-faint hover:text-ink" title="Close the tour (Esc)">skip</button>
        </div>
        <h3 className="mt-1 font-display text-[16px] font-semibold text-ivory">{step.title}</h3>
        <p className="mt-1 text-[12.5px] leading-snug text-ink-dim">{step.body}</p>
        {step.anchor.kind === 'none' && <p className="mt-1 text-[11px] text-ink-faint">The fleet has none of these yet.</p>}
        <div className="mt-2.5 flex items-center gap-1.5">
          {steps.map((_, k) => <i key={k} className={`h-1.5 w-1.5 rounded-full ${k === i ? 'bg-neon' : 'bg-[#5b4325]'}`} />)}
          <span className="flex-1" />
          {i > 0 && <button className="brass-button" onClick={() => setI(i - 1)}>Back</button>}
          <button className="brass-button brass-button-on" onClick={() => (last ? onClose() : setI(i + 1))}>{last ? 'Done' : 'Next'}</button>
        </div>
      </div>
    </div>
  )
}
