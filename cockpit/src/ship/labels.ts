// The Ship's labels (theseus-logs): engraved nameplates under the vessels, the places' names on their mooring rings,
// and, close in, a tag on each light. HTML over the canvas, so text stays crisp and legible; placed imperatively
// each frame the camera moves, greedily so no two overlap (the more important wins).
import type { ShipEngine } from './engine'
import type { Light, ShipModel, Vessel } from './model'

const RIG_WORD: Record<Vessel['rig'], string> = { anchor: 'at anchor', sail: 'under sail', lantern: 'lantern lit', flare: 'flare up' }

export function rigWords(v: Vessel): string {
  const a = v.attention?.label
  if (v.rig === 'flare') return `flare up · ${v.state}`
  if (v.rig === 'lantern') return a && a !== 'needs you' ? `lantern lit · ${a}` : 'lantern lit · waits for you'
  if (v.rig === 'sail') return a && a !== 'working' ? `under sail · ${a}` : 'under sail'
  return a && !['ready', 'idle'].includes(a) ? `at anchor · ${a}` : RIG_WORD.anchor
}

export function usdShort(n: number): string {
  return n === 0 ? '$0' : n < 0.01 ? `$${n.toFixed(4)}` : n < 1 ? `$${n.toFixed(3)}` : `$${n.toFixed(2)}`
}

function tagText(l: Light): string | null {
  if (l.kind === 'call') return l.tool ?? 'tool'
  if (l.kind === 'model') return (l.model ?? 'model').replace(/^claude-/, '')
  if (l.kind === 'user') return l.author ? l.author.replace(/^[a-z]+:/, '@') : 'operator'
  if (l.external) return 'external text'
  // A sandboxed job a cancel verified gone (18a): stopped, not failed.
  if (l.collapsedAt !== undefined) return 'stopped · verified'
  if (l.failed) return 'failed'
  return null
}

interface Rect { x0: number; y0: number; x1: number; y1: number }
const overlaps = (a: Rect, b: Rect) => a.x0 < b.x1 && b.x0 < a.x1 && a.y0 < b.y1 && b.y0 < a.y1

/** An element's size, measured once after its text changes (a layout read per frame would thrash). */
const sizes = new WeakMap<HTMLElement, { w: number; h: number }>()
function sizeOf(el: HTMLElement): { w: number; h: number } {
  let s = sizes.get(el)
  if (!s || !s.w) { s = { w: el.offsetWidth, h: el.offsetHeight }; sizes.set(el, s) }
  return s
}

export class LabelLayer {
  private root: HTMLElement
  private plates = new Map<string, HTMLDivElement>()
  private forms = new Map<string, HTMLDivElement>()
  private tags: HTMLDivElement[] = []
  private model: ShipModel | null = null

  constructor(root: HTMLElement) {
    this.root = root
  }

  setModel(m: ShipModel) {
    this.model = m
    const seen = new Set<string>()
    for (const v of m.vessels) {
      seen.add(v.id)
      let el = this.plates.get(v.id)
      if (!el) {
        el = document.createElement('div')
        el.className = 'ship-plate'
        el.innerHTML = '<div class="ship-plate-title"></div><div class="ship-plate-sub"><i></i><span></span></div>'
        this.root.appendChild(el)
        this.plates.set(v.id, el)
      }
      el.dataset.rig = v.rig
      el.dataset.task = v.kind === 'task' ? '1' : ''
      el.dataset.hold = v.hold ? '1' : ''
      const title = el.firstElementChild as HTMLElement
      const t = v.kind === 'task' && v.taskShort ? `${v.title} · task ${v.taskShort}` : v.title
      if (title.textContent !== t) { title.textContent = t; sizes.delete(el) }
      const sub = el.lastElementChild!.lastElementChild as HTMLElement
      const s = `${rigWords(v)} · ${v.nodes} ${v.nodes === 1 ? 'light' : 'lights'} · ${usdShort(v.cost)}${v.hold ? ' · chained' : ''}`
      if (sub.textContent !== s) { sub.textContent = s; sizes.delete(el) }
    }
    for (const [id, el] of this.plates) if (!seen.has(id)) { el.remove(); this.plates.delete(id) }
    const fseen = new Set<string>()
    for (const f of m.formations) {
      fseen.add(f.key)
      let el = this.forms.get(f.key)
      if (!el) {
        el = document.createElement('div')
        el.className = 'ship-formation'
        this.root.appendChild(el)
        this.forms.set(f.key, el)
      }
      const t = `${f.label} · ${f.members.length}`
      if (el.textContent !== t) { el.textContent = t; sizes.delete(el) }
    }
    for (const [k, el] of this.forms) if (!fseen.has(k)) { el.remove(); this.forms.delete(k) }
  }

  /** The UI over the canvas (title, controls, instruments, key, porthole): no label goes under it. Re-measured at
   *  most every half second. */
  private reserved: Rect[] = []
  private reservedAt = 0
  private measureReserved() {
    const now = performance.now()
    if (now - this.reservedAt < 500) return
    this.reservedAt = now
    const host = this.root.parentElement
    if (!host) return
    const o = host.getBoundingClientRect()
    this.reserved = [...host.querySelectorAll<HTMLElement>('[data-ship-ui]')].map((el) => {
      const r = el.getBoundingClientRect()
      return { x0: r.left - o.left - 6, y0: r.top - o.top - 6, x1: r.right - o.left + 6, y1: r.bottom - o.top + 6 }
    })
  }

  update(e: ShipEngine, selected: number, hovered: number) {
    const m = this.model
    if (!m) return
    const W = this.root.clientWidth
    const H = this.root.clientHeight
    this.measureReserved()
    const taken: Rect[] = [...this.reserved]
    // Places: on their ring's north side, while the ring is a size worth naming; the biggest first, and none over
    // another.
    const order = [...m.formations].sort((a, b) => b.members.length - a.members.length || a.key.localeCompare(b.key))
    for (const f of order) {
      const el = this.forms.get(f.key)!
      const ppu = e.pixelsPerUnit(f.x, f.z)
      const rpx = f.radius * ppu
      const p = e.project(f.x, 0, f.z - f.radius)
      let show = p.on && rpx > 46 && rpx < 2600 && p.y > 4 && p.y < H - 4
      if (show) {
        el.style.display = ''
        const w = sizeOf(el).w
        const r = { x0: p.x - w / 2 - 6, y0: p.y - 30, x1: p.x + w / 2 + 6, y1: p.y - 6 }
        if (taken.some((t) => overlaps(t, r))) show = false
        else {
          el.style.transform = `translate(${p.x}px, ${p.y - 8}px) translate(-50%, -100%)`
          taken.push(r)
        }
      }
      if (!show) el.style.display = 'none'
    }
    // Plates: the selected and hovered first, then the biggest on screen.
    const cand: { i: number; px: number; x: number; y: number }[] = []
    m.vessels.forEach((v, i) => {
      const now = e.vesselNow(i)
      const ppu = e.pixelsPerUnit(now.x, now.z)
      const len = v.length * ppu
      const below = v.beam / 2 + 1.6 + v.beam * 0.55
      const p = e.project(now.x, 0, now.z + below)
      const pri = i === selected ? 1e9 : i === hovered ? 1e8 : len
      if (p.on && p.x > -80 && p.x < W + 80 && p.y > -20 && p.y < H + 20 && (len > 54 || i === selected || i === hovered)) cand.push({ i, px: pri, x: p.x, y: p.y })
    })
    cand.sort((a, b) => b.px - a.px)
    const shown = new Set<number>()
    for (const c of cand.slice(0, 60)) {
      const el = this.plates.get(m.vessels[c.i].id)!
      el.style.display = ''
      const { w, h } = sizeOf(el)
      const r = { x0: c.x - w / 2, y0: c.y, x1: c.x + w / 2, y1: c.y + h }
      if (c.px < 1e8 && taken.some((t) => overlaps(t, r))) { el.style.display = 'none'; continue }
      taken.push(r)
      shown.add(c.i)
      el.style.transform = `translate(${c.x}px, ${c.y}px) translate(-50%, 0)`
      el.dataset.on = c.i === selected ? 'selected' : c.i === hovered ? 'hovered' : ''
    }
    m.vessels.forEach((v, i) => { if (!shown.has(i)) this.plates.get(v.id)!.style.display = 'none' })
    // Tags on the lights of a vessel drawn big, while they are far enough apart to read.
    let used = 0
    const want: { l: Light; x: number; y: number; text: string }[] = []
    m.vessels.forEach((v, i) => {
      const now = e.vesselNow(i)
      const ppu = e.pixelsPerUnit(now.x, now.z)
      if (v.length * ppu < 900 || !v.nodes) return
      const spacing = (v.length * 0.76 * ppu) / Math.max(1, v.nodes * 0.62)
      if (spacing < 20) return
      for (let k = 0; k < m.lights.length; k++) {
        const l = m.lights[k]
        if (l.vessel !== i) continue
        const text = tagText(l)
        if (!text) continue
        const w = e.lightWorldNow(k)
        if (!w) continue
        const p = e.project(w.x, w.y, w.z)
        if (!p.on || p.x < 0 || p.x > W || p.y < 0 || p.y > H) continue
        want.push({ l, x: p.x, y: p.y, text })
      }
    })
    for (const t of want) {
      if (used >= 160) break
      const width = Math.min(150, 7 + t.text.length * 6.2)
      const r = { x0: t.x + 6, y0: t.y - 17, x1: t.x + 6 + width, y1: t.y - 3 }
      if (taken.some((q) => overlaps(q, r))) continue
      taken.push(r)
      let el = this.tags[used]
      if (!el) {
        el = document.createElement('div')
        el.className = 'ship-tag'
        this.root.appendChild(el)
        this.tags.push(el)
      }
      el.textContent = t.text
      el.dataset.kind = t.l.kind
      el.dataset.flag = t.l.external ? 'external' : t.l.failed ? 'failed' : t.l.l1 ? 'l1' : ''
      el.style.display = ''
      el.style.transform = `translate(${t.x + 6}px, ${t.y - 17}px)`
      used++
    }
    for (let k = used; k < this.tags.length; k++) this.tags[k].style.display = 'none'
  }

  dispose() {
    for (const el of this.plates.values()) el.remove()
    for (const el of this.forms.values()) el.remove()
    for (const el of this.tags) el.remove()
  }
}

