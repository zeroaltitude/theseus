// The key's lines (theseus-hnof): one line per shape on the chart, its plain word and its sea word, how many of it the
// chart holds now, and which vessels and lights are it, so resting on a line lights every one of them and dims the rest.
// Pure, so the key and a test read the same lines.
import type { ShipModel } from './model'

export type KeyGroup = 'The fleet' | 'A ship' | 'Its oars' | 'Its state'
export type Glyph =
  | 'harbour' | 'ship' | 'boat' | 'bench' | 'message' | 'model' | 'oar' | 'oar-failed' | 'oar-waiting' | 'gear' | 'shield'
  | 'web' | 'sail' | 'lantern' | 'flare' | 'anchor'

export interface KeyLine {
  id: string
  group: KeyGroup
  glyph: Glyph
  /** What it is, in plain words. */
  word: string
  /** How the chart draws it. */
  sea: string
  count: number
  /** What it lights on the chart (session ids, node ids); none for a line that lights nothing on its own. */
  vessels: string[]
  lights: string[]
}

export function keyLines(m: ShipModel): KeyLine[] {
  const ids = (f: (i: number) => boolean) => m.vessels.filter((_, i) => f(i)).map((v) => v.id)
  const lightIds = (f: (i: number) => boolean) => m.lights.filter((_, i) => f(i)).map((l) => l.id)
  // An oar is drawn from its call: a result's mark lights its call.
  const callOf = new Map<string, string>()
  for (const l of m.lights) if (l.kind === 'call' && l.toolUseId) callOf.set(l.toolUseId, l.id)
  const resultCalls = (f: (i: number) => boolean) => {
    const out = new Set<string>()
    m.lights.forEach((l, i) => {
      if (l.kind !== 'result' || !f(i)) return
      const c = l.toolUseId ? callOf.get(l.toolUseId) : undefined
      out.add(c ?? l.id)
    })
    return [...out]
  }
  const L = m.lights
  const V = m.vessels
  const failedCalls = new Set([...resultCalls((i) => !!L[i].failed), ...lightIds((i) => L[i].kind === 'call' && !!L[i].failed)])
  const lines: KeyLine[] = [
    { id: 'place', group: 'The fleet', glyph: 'harbour', word: 'a place', sea: 'harbour ring', count: m.formations.length, vessels: [], lights: [] },
    { id: 'session', group: 'The fleet', glyph: 'ship', word: 'a session', sea: 'ship', count: V.filter((v) => v.kind === 'conversation').length, vessels: ids((i) => V[i].kind === 'conversation'), lights: [] },
    { id: 'task', group: 'The fleet', glyph: 'boat', word: 'a task', sea: 'boat in tow', count: V.filter((v) => v.kind === 'task').length, vessels: ids((i) => V[i].kind === 'task'), lights: [] },
    { id: 'turn', group: 'A ship', glyph: 'bench', word: 'a turn', sea: 'bench; newest at the bow', count: m.benches.length, vessels: ids((i) => V[i].benches.length > 0), lights: [] },
    { id: 'message', group: 'A ship', glyph: 'message', word: 'a message', sea: 'ivory lamp', count: L.filter((l) => l.kind === 'user').length, vessels: [], lights: lightIds((i) => L[i].kind === 'user') },
    { id: 'model', group: 'A ship', glyph: 'model', word: 'a model call', sea: 'violet lamp', count: L.filter((l) => l.kind === 'model').length, vessels: [], lights: lightIds((i) => L[i].kind === 'model') },
    { id: 'call', group: 'Its oars', glyph: 'oar', word: 'a tool call', sea: 'oar; the blade is its result', count: L.filter((l) => l.kind === 'call').length, vessels: [], lights: lightIds((i) => L[i].kind === 'call') },
    { id: 'failed', group: 'Its oars', glyph: 'oar-failed', word: 'failed', sea: 'rose blade ✕, pennant', count: failedCalls.size, vessels: [], lights: [...failedCalls] },
    { id: 'waiting', group: 'Its oars', glyph: 'oar-waiting', word: 'waits for you', sea: 'amber blade, lamp', count: L.filter((l) => l.waiting).length, vessels: [], lights: lightIds((i) => !!L[i].waiting) },
    { id: 'job', group: 'Its oars', glyph: 'gear', word: 'a job running', sea: 'brass gear, turning', count: L.filter((l) => l.kind === 'call' && l.running).length, vessels: [], lights: lightIds((i) => L[i].kind === 'call' && !!L[i].running) },
    { id: 'l1', group: 'Its oars', glyph: 'shield', word: 'sandboxed (L1)', sea: 'hex shield', count: L.filter((l) => l.kind === 'call' && l.l1).length, vessels: [], lights: lightIds((i) => L[i].kind === 'call' && !!L[i].l1) },
    { id: 'web', group: 'Its oars', glyph: 'web', word: 'text from the web', sea: 'magenta blade', count: L.filter((l) => l.external).length, vessels: [], lights: resultCalls((i) => !!L[i].external) },
    { id: 'working', group: 'Its state', glyph: 'sail', word: 'working', sea: 'sail up, oars rowing', count: V.filter((v) => v.rig === 'sail').length, vessels: ids((i) => V[i].rig === 'sail'), lights: [] },
    { id: 'needs', group: 'Its state', glyph: 'lantern', word: 'waiting for you', sea: 'lantern lit', count: V.filter((v) => v.rig === 'lantern').length, vessels: ids((i) => V[i].rig === 'lantern'), lights: [] },
    { id: 'down', group: 'Its state', glyph: 'flare', word: 'failed or over budget', sea: 'flare up', count: V.filter((v) => v.rig === 'flare').length, vessels: ids((i) => V[i].rig === 'flare'), lights: [] },
    { id: 'idle', group: 'Its state', glyph: 'anchor', word: 'idle', sea: 'at anchor', count: V.filter((v) => v.rig === 'anchor').length, vessels: ids((i) => V[i].rig === 'anchor'), lights: [] },
  ]
  return lines
}

/** Whether a line lights anything on the chart. */
export const lights = (k: KeyLine) => k.vessels.length > 0 || k.lights.length > 0
