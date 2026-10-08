// The Ship's labels (theseus-logs; plain words, theseus-hnof): nameplates under the vessels, the places' names on their
// harbour rings, a label on each turn's bench once a vessel is drawn big, and, close in, a tag on each oar and message.
// HTML over the canvas, so text stays crisp and legible; placed imperatively each frame the camera moves, greedily so
// no two overlap (the more important wins). Every label says what its shape is in plain words: "session", "task",
// "turn 12", "fs.read · failed". The words are `words.ts`'s, as the key, the cards and the tour say them.
import type { ShipEngine } from './engine'
import type { Bench, Light, ShipModel } from './model'
import { benchLabel, harbourLine, keelTags, oarTag, outcome, plateLine, stateWord, usdShort, vesselNoun } from './words'
import { instrumentRects } from './instrumentRects'

export { usdShort }

/** A tag's words: an oar says its tool and how it went; a message says who wrote it, and a model call its model, only
 *  where that changes along its ship (`keelTags`). */
function tagText(l: Light, i: number, results: Map<string, number>, keel: Map<number, string>): string | null {
  if (l.kind === 'call') return oarTag(l.tool, outcome(l, !!l.toolUseId && results.has(l.toolUseId)))
  return keel.get(i) ?? null
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
  private benchTags: HTMLDivElement[] = []
  private model: ShipModel | null = null
  /** Each oar's result, by its tool_use id: an index into the model's lights. */
  private results = new Map<string, number>()
  /** The messages' and model calls' tags, where they say something new (`keelTags`). */
  private keel = new Map<number, string>()

  constructor(root: HTMLElement) {
    this.root = root
  }

  setModel(m: ShipModel) {
    this.model = m
    this.results = new Map()
    m.lights.forEach((l, i) => { if (l.kind === 'result' && l.toolUseId) this.results.set(l.toolUseId, i) })
    this.keel = keelTags(m.lights)
    const seen = new Set<string>()
    for (const v of m.vessels) {
      seen.add(v.id)
      let el = this.plates.get(v.id)
      if (!el) {
        el = document.createElement('div')
        el.className = 'ship-plate'
        el.innerHTML = '<div class="ship-plate-kind"></div><div class="ship-plate-title"></div><div class="ship-plate-sub"><i></i><span></span></div>'
        this.root.appendChild(el)
        this.plates.set(v.id, el)
      }
      const st = stateWord(v)
      el.dataset.rig = v.rig
      el.dataset.tone = st.tone
      el.dataset.task = v.kind === 'task' ? '1' : ''
      el.dataset.hold = v.hold ? '1' : ''
      el.dataset.life = v.life
      const kind = el.children[0] as HTMLElement
      const k = v.kind === 'task' ? `task${v.taskShort ? ` ${v.taskShort}` : ''}` : 'session'
      if (kind.textContent !== k) { kind.textContent = k; sizes.delete(el) }
      const title = el.children[1] as HTMLElement
      if (title.textContent !== v.title) { title.textContent = v.title; sizes.delete(el) }
      const sub = el.children[2].lastElementChild as HTMLElement
      const s = plateLine(v)
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
        el.innerHTML = '<span class="ship-formation-name"></span><span class="ship-formation-sub"></span>'
        this.root.appendChild(el)
        this.forms.set(f.key, el)
      }
      // The harbour's sessions, and the tasks in tow of them.
      let tasks = 0
      const towing = (i: number, d: number) => { for (const v of m.vessels) if (v.parentId === m.vessels[i].id && d < 8) { tasks++; towing(m.byId.get(v.id)!, d + 1) } }
      for (const i of f.members) towing(i, 0)
      const name = el.children[0] as HTMLElement
      const sub = el.children[1] as HTMLElement
      if (name.textContent !== f.label) { name.textContent = f.label; sizes.delete(el) }
      const t = harbourLine(f.members.length, tasks)
      if (sub.textContent !== t) { sub.textContent = t; sizes.delete(el) }
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
    this.reserved = instrumentRects(host, 6)
  }

  update(e: ShipEngine, selected: number, hovered: number) {
    const m = this.model
    if (!m) return
    const W = this.root.clientWidth
    const H = this.root.clientHeight
    this.measureReserved()
    const taken: Rect[] = [...this.reserved]
    // Plates: the selected and hovered first, then the biggest on screen.
    const cand: { i: number; px: number; x: number; y: number }[] = []
    m.vessels.forEach((v, i) => {
      const now = e.vesselNow(i)
      const ppu = e.pixelsPerUnit(now.x, now.z)
      const len = v.length * ppu
      const below = v.beam / 2 + 1.6 + v.beam * 0.55
      const p = e.project(now.x, 0, now.z + below)
      // Waiting for you and failed come before size: they are what the operator looks for.
      const pri = i === selected ? 1e9 : i === hovered ? 1e8 : len + (v.rig === 'lantern' ? 5e6 : v.rig === 'flare' ? 4e6 : v.rig === 'sail' ? 3e6 : 0)
      if (p.on && p.x > -80 && p.x < W + 80 && p.y > -20 && p.y < H + 20 && (len > 44 || i === selected || i === hovered || v.rig === 'lantern' || v.rig === 'flare')) cand.push({ i, px: pri, x: p.x, y: p.y })
    })
    cand.sort((a, b) => b.px - a.px)
    const shown = new Set<number>()
    // The plates that matter most (selected, hovered, waiting, failed, working) go down before the harbours' names; the
    // rest after them.
    const placePlate = (c: { i: number; px: number; x: number; y: number }) => {
      const el = this.plates.get(m.vessels[c.i].id)!
      el.style.display = ''
      const { w, h } = sizeOf(el)
      const r = { x0: c.x - w / 2, y0: c.y, x1: c.x + w / 2, y1: c.y + h }
      // The selected and hovered plates may cover other labels, never an instrument (their card says them too).
      if ((c.px < 1e8 ? taken : this.reserved).some((t) => overlaps(t, r))) { el.style.display = 'none'; return }
      taken.push(r)
      shown.add(c.i)
      el.style.transform = `translate(${c.x}px, ${c.y}px) translate(-50%, 0)`
      el.dataset.on = c.i === selected ? 'selected' : c.i === hovered ? 'hovered' : ''
    }
    const top = cand.slice(0, 60)
    for (const c of top) if (c.px >= 3e6) placePlate(c)

    // Places: on their ring's north side, while the ring is a size worth naming (from about 50 px across, so a small
    // screen's fleet still names its harbours); the biggest first, and none over another.
    const order = [...m.formations].sort((a, b) => b.members.length - a.members.length || a.key.localeCompare(b.key))
    for (const f of order) {
      const el = this.forms.get(f.key)!
      const ppu = e.pixelsPerUnit(f.x, f.z)
      const rpx = f.radius * ppu
      const p = e.project(f.x, 0, f.z - f.radius)
      let show = p.on && rpx > 24 && rpx < 2600 && p.y > 4 && p.y < H - 4
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
    for (const c of top) if (c.px < 3e6) placePlate(c)
    m.vessels.forEach((v, i) => { if (!shown.has(i)) this.plates.get(v.id)!.style.display = 'none' })

    // Benches: once a vessel is drawn big, each turn's label above its port rail ("turn 12 · working"), while they are
    // far enough apart to read; the running, waiting and failed turns first.
    let usedB = 0
    m.vessels.forEach((v, i) => {
      const now = e.vesselNow(i)
      const ppu = e.pixelsPerUnit(now.x, now.z)
      if (v.length * ppu < 520 || !v.benches.length) return
      const c = Math.cos(now.heading)
      const s = Math.sin(now.heading)
      const list = [...v.benches].sort((a, b) => {
        const A = m.benches[a]; const B = m.benches[b]
        const w = (x: Bench) => (x.waiting ? 4 : 0) + (x.running ? 3 : 0) + (x.failed ? 2 : 0)
        return w(B) - w(A) || B.n - A.n
      })
      for (const bi of list) {
        if (usedB >= 80) break
        const b = m.benches[bi]
        const lz = -v.beam * 0.5 - 0.4
        const p = e.project(now.x + b.x * c - lz * s, 0.2, now.z + b.x * s + lz * c)
        if (!p.on || p.x < 0 || p.x > W || p.y < 0 || p.y > H) continue
        const wide = b.half * 2 * ppu > 120
        const text = benchLabel(b, wide)
        const width = 10 + text.length * 6.4
        const r = { x0: p.x - width / 2, y0: p.y - 20, x1: p.x + width / 2, y1: p.y - 4 }
        if (taken.some((q) => overlaps(q, r))) continue
        taken.push(r)
        let el = this.benchTags[usedB]
        if (!el) {
          el = document.createElement('div')
          el.className = 'ship-bench'
          this.root.appendChild(el)
          this.benchTags.push(el)
        }
        if (el.textContent !== text) el.textContent = text
        el.dataset.state = b.waiting ? 'waiting' : b.running ? 'running' : b.failed ? 'failed' : ''
        el.style.display = ''
        el.style.transform = `translate(${p.x}px, ${p.y - 20}px) translate(-50%, 0)`
        usedB++
      }
    })
    for (let k = usedB; k < this.benchTags.length; k++) this.benchTags[k].style.display = 'none'

    // Tags on the oars and messages of a vessel drawn big, while they are far enough apart to read. An oar's tag sits
    // at its blade, where its result is.
    let used = 0
    const want: { l: Light; x: number; y: number; text: string }[] = []
    m.vessels.forEach((v, i) => {
      const now = e.vesselNow(i)
      const ppu = e.pixelsPerUnit(now.x, now.z)
      if (v.length * ppu < 900 || !v.nodes) return
      const spacing = (v.length * 0.76 * ppu) / Math.max(1, v.nodes * 0.62)
      if (spacing < 16) return
      const c = Math.cos(now.heading)
      const s = Math.sin(now.heading)
      for (let k = 0; k < m.lights.length; k++) {
        const l = m.lights[k]
        if (l.vessel !== i || l.kind === 'result') continue
        const text = tagText(l, k, this.results, this.keel)
        if (!text) continue
        let x = l.lx
        let z = l.lz
        if (l.kind === 'call') {
          const ri = l.toolUseId ? this.results.get(l.toolUseId) : undefined
          const r = ri !== undefined ? m.lights[ri] : undefined
          const side = Math.sign(l.lz) || 1
          x = r ? r.lx : l.lx - (v.beam * 0.55) * 0.42
          z = r ? r.lz : side * (v.beam * 0.5 + Math.max(1.1, v.beam * 0.55))
        }
        const p = e.project(now.x + x * c - z * s, 0.2, now.z + x * s + z * c)
        if (!p.on || p.x < 0 || p.x > W || p.y < 0 || p.y > H) continue
        want.push({ l, x: p.x, y: p.y, text })
      }
    })
    for (const t of want) {
      if (used >= 160) break
      const width = Math.min(190, 7 + t.text.length * 6.2)
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
      el.dataset.flag = t.l.waiting ? 'waiting' : t.l.failed ? 'failed' : t.l.running ? 'running' : t.l.l1 ? 'l1' : ''
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
    for (const el of this.benchTags) el.remove()
  }
}

export { vesselNoun }
