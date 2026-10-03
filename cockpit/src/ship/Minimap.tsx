// The porthole (theseus-logs): the whole fleet in a brass-rimmed circle, the view's footprint on it, and a click or a
// drag to sail the view there. Canvas 2D, redrawn only when the view or the fleet changes.
import { useEffect, useImperativeHandle, useRef, type Ref } from 'react'
import type { ShipEngine } from './engine'
import type { ShipModel } from './model'

const RIG_COLOR: Record<string, string> = { anchor: '#b08d57', sail: '#22d3ee', lantern: '#fbbf24', flare: '#fb7185' }

export interface MinimapHandle { draw: () => void }

export function Minimap({ engine, model, selected, size = 156, ref }: { engine: ShipEngine | null; model: ShipModel | null; selected: number; size?: number; ref?: Ref<MinimapHandle> }) {
  const canvas = useRef<HTMLCanvasElement>(null)
  const frame = useRef({ cx: 0, cz: 0, scale: 1 })

  const draw = () => {
    const c = canvas.current
    if (!c || !model) return
    const dpr = Math.min(2, window.devicePixelRatio || 1)
    if (c.width !== size * dpr) { c.width = size * dpr; c.height = size * dpr }
    const g = c.getContext('2d')!
    g.setTransform(dpr, 0, 0, dpr, 0, 0)
    g.clearRect(0, 0, size, size)
    const R = size / 2
    const b = model.bounds
    const span = Math.max(b.maxX - b.minX, b.maxZ - b.minZ, 40) * 1.18
    const scale = (size - 18) / span
    const cx = (b.minX + b.maxX) / 2
    const cz = (b.minZ + b.maxZ) / 2
    frame.current = { cx, cz, scale }
    const X = (x: number) => R + (x - cx) * scale
    const Y = (z: number) => R + (z - cz) * scale
    g.save()
    g.beginPath()
    g.arc(R, R, R - 3, 0, Math.PI * 2)
    g.clip()
    const bg = g.createRadialGradient(R, R * 0.8, 4, R, R, R)
    bg.addColorStop(0, '#0f2338')
    bg.addColorStop(1, '#040b15')
    g.fillStyle = bg
    g.fillRect(0, 0, size, size)
    // The chart's grid, and the places' rings.
    g.strokeStyle = 'rgba(34,211,238,0.07)'
    g.lineWidth = 1
    for (let k = 0; k <= 8; k++) {
      const p = (k / 8) * size
      g.beginPath(); g.moveTo(p, 0); g.lineTo(p, size); g.stroke()
      g.beginPath(); g.moveTo(0, p); g.lineTo(size, p); g.stroke()
    }
    g.strokeStyle = 'rgba(214,165,72,0.28)'
    for (const f of model.formations) {
      g.beginPath(); g.arc(X(f.x), Y(f.z), Math.max(2, f.radius * scale), 0, Math.PI * 2); g.stroke()
    }
    for (const t of model.tethers) {
      const a = engine?.vesselNow(t.from) ?? model.vessels[t.from]
      const d = engine?.vesselNow(t.to) ?? model.vessels[t.to]
      g.strokeStyle = t.live ? 'rgba(34,211,238,0.6)' : 'rgba(214,165,72,0.35)'
      g.beginPath(); g.moveTo(X(a.x), Y(a.z)); g.lineTo(X(d.x), Y(d.z)); g.stroke()
    }
    model.vessels.forEach((v, i) => {
      const now = engine?.vesselNow(i) ?? v
      const len = Math.max(3, v.length * scale)
      const c2 = Math.cos(now.heading)
      const s2 = Math.sin(now.heading)
      g.strokeStyle = RIG_COLOR[v.rig]
      g.lineWidth = i === selected ? 3 : Math.max(1.4, v.beam * scale * 0.8)
      g.shadowColor = RIG_COLOR[v.rig]
      g.shadowBlur = v.rig === 'anchor' ? 0 : 5
      g.beginPath()
      g.moveTo(X(now.x - (c2 * len) / scale / 2), Y(now.z - (s2 * len) / scale / 2))
      g.lineTo(X(now.x + (c2 * len) / scale / 2), Y(now.z + (s2 * len) / scale / 2))
      g.stroke()
    })
    g.shadowBlur = 0
    // The view's footprint on the sea.
    if (engine) {
      const fp = engine.footprint()
      g.beginPath()
      fp.forEach(([x, z], k) => (k ? g.lineTo(X(x), Y(z)) : g.moveTo(X(x), Y(z))))
      g.closePath()
      g.fillStyle = 'rgba(34,211,238,0.07)'
      g.fill()
      g.strokeStyle = 'rgba(34,211,238,0.85)'
      g.lineWidth = 1.2
      g.stroke()
    }
    g.restore()
  }

  useImperativeHandle(ref, () => ({ draw }))
  useEffect(draw)

  const toWorld = (e: React.PointerEvent) => {
    const r = canvas.current!.getBoundingClientRect()
    const { cx, cz, scale } = frame.current
    return { x: cx + (e.clientX - r.left - size / 2) / scale, z: cz + (e.clientY - r.top - size / 2) / scale }
  }
  const drag = useRef(false)
  return (
    <div className="ship-porthole" style={{ width: size + 16, height: size + 16 }} title="The whole fleet: click or drag to sail the view there">
      <canvas
        ref={canvas}
        style={{ width: size, height: size }}
        onPointerDown={(e) => { drag.current = true; e.currentTarget.setPointerCapture(e.pointerId); const p = toWorld(e); engine?.panTo(p.x, p.z) }}
        onPointerMove={(e) => { if (drag.current) { const p = toWorld(e); engine?.panTo(p.x, p.z) } }}
        onPointerUp={() => { drag.current = false }}
      />
    </div>
  )
}
