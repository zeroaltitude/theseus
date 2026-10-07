// Money spent and memory recalled, as they happen (theseus-hnof). When a turn ends, its cost rises from its ship as a
// gold coin and flies to the watch's "Spent today" plate, which counts it; a turn that recalled memory sparks violet at
// its bench. Real events only: the daemon's own turn.ended push (its cost_usd and recalled). Calm mode and
// prefers-reduced-motion fly nothing: the plate's number changes on its own.
import { useEffect, useRef } from 'react'
import type { TurnSubmitResult } from '@protocol'
import type { ShipEngine } from './engine'
import { usdShort } from './labels'
import { useCalm } from '@/lib/calm'
import { client } from '@/lib/rpc'

export function Coins({ engine, host }: { engine: ShipEngine | null; host: HTMLElement | null }) {
  const calm = useCalm((s) => s.calm)
  const layer = useRef<HTMLDivElement>(null)
  useEffect(() => {
    if (!engine || !host) return
    return client.onNotify((method, params) => {
      if (method !== 'turn.ended') return
      const p = params as TurnSubmitResult
      if ((p.recalled ?? 0) > 0 && p.turn_id) engine.markRecall(p.turn_id)
      const cost = p.cost_usd ?? 0
      const m = engine.model
      if (calm || !(cost > 0) || !m || !layer.current) return
      if (window.matchMedia?.('(prefers-reduced-motion: reduce)').matches) return
      const vi = m.byId.get(p.session_id)
      if (vi === undefined) return
      const now = engine.vesselNow(vi)
      const from = engine.project(now.x, 1.5, now.z)
      if (!from.on) return
      const o = host.getBoundingClientRect()
      const target = host.querySelector('[data-watch="spent"]') ?? host.querySelector('.ship-watch-slot')
      const r = target?.getBoundingClientRect()
      const to = r ? { x: r.left - o.left + Math.min(r.width / 2, 120), y: r.top - o.top + 24 } : { x: o.width - 160, y: 24 }
      const el = document.createElement('div')
      el.className = 'ship-coin'
      el.textContent = `+${usdShort(cost)}`
      el.style.transform = `translate(${from.x}px, ${from.y}px) translate(-50%, -50%)`
      layer.current.appendChild(el)
      // Rise first, then fly: two frames so the browser sees where it starts.
      requestAnimationFrame(() => requestAnimationFrame(() => {
        el.style.transform = `translate(${to.x}px, ${to.y}px) translate(-50%, -50%) scale(0.7)`
        el.style.opacity = '0.15'
      }))
      window.setTimeout(() => el.remove(), 1300)
    })
  }, [engine, host, calm])
  return <div ref={layer} className="pointer-events-none absolute inset-0 z-[36] overflow-hidden" aria-hidden />
}
