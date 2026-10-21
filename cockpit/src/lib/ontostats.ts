// The ontology pack's counts as the cockpit reads them (theseus-wzvxp, R7d): how many judgments `onto.v1` made, how many acted
// (filed a category or attached a project), how many only asked (the confirm band), and how many of its writes the
// operator took back. All from the ledger's own rows, so the Judgment view, whose `judge.list` reads only a window, can show
// the pack's whole record: a `judge.call` row per judgment, an `onto.filed` row per judgment that filed or asked, and an
// `ontology.undo` row per write taken back (a judgment's id is its write's). Pure, with no import but the protocol's types,
// so `node --test` runs its test (`cockpit/test/ontostats.test.ts`) as it is.
import type { LedgerEntry } from '@protocol'

/** The ontology pack's name in a `judge.call` row. */
export const ONTO_PACK = 'onto.v1'

export interface OntoStats {
  /** `onto.v1` judgments: every `judge.call` row of the pack, whatever its outcome. */
  judgments: number
  /** Of those, the ones that wrote: at least one step filed a category or attached a project. */
  acts: number
  /** The ones that wrote nothing and asked: at least one step in the confirm band, none acted. */
  confirms: number
  /** The acts the operator took back: distinct `ontology.undo` writes that are one of the acts. */
  undos: number
}

type D = Record<string, unknown>
const obj = (v: unknown): D => (v && typeof v === 'object' && !Array.isArray(v) ? (v as D) : {})
const str = (v: unknown): string | null => (typeof v === 'string' ? v : null)

/** A step's outcome in an `onto.filed` row that wrote. */
const ACTED = new Set(['filed', 'attached'])

/** The pack's counts over the ledger. An `onto.filed` or `ontology.undo` row of a judgment the rows do not hold is left out
 *  (a history that does not reach back to it), so acts and confirms never exceed the judgments. */
export function ontoStatsOf(rows: readonly LedgerEntry[]): OntoStats {
  const judgments = new Set<string>()
  for (const r of rows) {
    if (r.kind !== 'judge.call') continue
    const d = obj(r.data)
    const id = str(d.id)
    if (id !== null && d.pack === ONTO_PACK) judgments.add(id)
  }
  const acted = new Set<string>()
  const asked = new Set<string>()
  for (const r of rows) {
    if (r.kind !== 'onto.filed') continue
    const d = obj(r.data)
    const id = str(d.judgment)
    if (id === null || !judgments.has(id)) continue
    const steps = (Array.isArray(d.steps) ? d.steps : []).map((s) => str(obj(s).outcome))
    if (steps.some((o) => o !== null && ACTED.has(o))) acted.add(id)
    else if (steps.includes('asked')) asked.add(id)
  }
  const undone = new Set<string>()
  for (const r of rows) {
    if (r.kind !== 'ontology.undo') continue
    const w = str(obj(r.data).write)
    if (w !== null && acted.has(w)) undone.add(w)
  }
  // A judgment that both acted and asked (one kind filed, another asked) is an act only.
  const confirms = [...asked].filter((id) => !acted.has(id)).length
  return { judgments: judgments.size, acts: acted.size, confirms, undos: undone.size }
}

const count = (n: number, one: string, many: string) => `${n.toLocaleString()} ${n === 1 ? one : many}`

/** The pack's one line: `ontology: 42 judgments · 9 acts · 5 confirms · 2 undos (22% of acts)`. The rate is left off while
 *  there are no acts to take back. */
export function ontoLine(s: OntoStats): string {
  if (s.judgments === 0) return 'ontology: no judgments yet'
  const rate = s.acts > 0 ? ` (${Math.round((s.undos / s.acts) * 100)}% of acts)` : ''
  return `ontology: ${count(s.judgments, 'judgment', 'judgments')} · ${count(s.acts, 'act', 'acts')} · ${count(s.confirms, 'confirm', 'confirms')} · ${count(s.undos, 'undo', 'undos')}${rate}`
}
