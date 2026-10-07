// The key's lines (theseus-hnof): one line per shape on the chart, its plain word and its sea word (both from `words.ts`,
// as every label, card and tour step says them), how many of it the chart holds now, and which vessels and lights are it,
// so resting on a line lights every one of them and dims the rest. Pure, so the key and a test read the same lines.
import type { ShipModel } from './model'
import { SHAPES, type Shape } from './words.ts'

export type KeyGroup = 'The fleet' | 'A ship' | 'Its oars' | 'Its state'
export type Glyph =
  | 'harbour' | 'ship' | 'boat' | 'bench' | 'message' | 'model' | 'oar' | 'oar-failed' | 'oar-waiting' | 'gear' | 'shield'
  | 'web' | 'sail' | 'lantern' | 'flare' | 'anchor' | 'planks' | 'chain' | 'sea'

export interface KeyLine {
  /** The shape it names (its words are `SHAPES[id]`). */
  id: Shape
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
  const line = (id: Shape, group: KeyGroup, glyph: Glyph, n: number, vessels: string[], lit: string[]): KeyLine =>
    ({ id, group, glyph, word: SHAPES[id].word, sea: SHAPES[id].sea, count: n, vessels, lights: lit })
  const lines: KeyLine[] = [
    line('place', 'The fleet', 'harbour', m.formations.length, [], []),
    line('session', 'The fleet', 'ship', V.filter((v) => v.kind === 'conversation').length, ids((i) => V[i].kind === 'conversation'), []),
    line('task', 'The fleet', 'boat', V.filter((v) => v.kind === 'task').length, ids((i) => V[i].kind === 'task'), []),
    line('turn', 'A ship', 'bench', m.benches.length, ids((i) => V[i].benches.length > 0), []),
    line('message', 'A ship', 'message', L.filter((l) => l.kind === 'user').length, [], lightIds((i) => L[i].kind === 'user')),
    line('model', 'A ship', 'model', L.filter((l) => l.kind === 'model').length, [], lightIds((i) => L[i].kind === 'model')),
    line('planks', 'A ship', 'planks', V.reduce((a, v) => a + v.goldPlanks, 0), ids((i) => V[i].goldPlanks > 0), []),
    line('call', 'Its oars', 'oar', L.filter((l) => l.kind === 'call').length, [], lightIds((i) => L[i].kind === 'call')),
    line('failed', 'Its oars', 'oar-failed', failedCalls.size, [], [...failedCalls]),
    line('waiting', 'Its oars', 'oar-waiting', L.filter((l) => l.waiting).length, [], lightIds((i) => !!L[i].waiting)),
    line('job', 'Its oars', 'gear', L.filter((l) => l.kind === 'call' && l.running).length, [], lightIds((i) => L[i].kind === 'call' && !!L[i].running)),
    line('l1', 'Its oars', 'shield', L.filter((l) => l.kind === 'call' && l.l1).length, [], lightIds((i) => L[i].kind === 'call' && !!L[i].l1)),
    line('web', 'Its oars', 'web', L.filter((l) => l.external).length, [], resultCalls((i) => !!L[i].external)),
    line('working', 'Its state', 'sail', V.filter((v) => v.rig === 'sail').length, ids((i) => V[i].rig === 'sail'), []),
    line('needs', 'Its state', 'lantern', V.filter((v) => v.rig === 'lantern').length, ids((i) => V[i].rig === 'lantern'), []),
    line('down', 'Its state', 'flare', V.filter((v) => v.rig === 'flare').length, ids((i) => V[i].rig === 'flare'), []),
    line('idle', 'Its state', 'anchor', V.filter((v) => v.rig === 'anchor').length, ids((i) => V[i].rig === 'anchor'), []),
    line('held', 'Its state', 'chain', V.filter((v) => v.hold).length, ids((i) => !!V[i].hold), []),
  ]
  return lines
}

/** Whether a line lights anything on the chart. */
export const lights = (k: KeyLine) => k.vessels.length > 0 || k.lights.length > 0
