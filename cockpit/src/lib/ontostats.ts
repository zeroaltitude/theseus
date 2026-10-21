// The ontology pack's counts as the cockpit reads them (theseus-wzvxp, R7d): how many judgments `onto.v1` made, how many acted
// (filed a category or attached a project), how many only asked (the confirm band), and how many of its writes the
// operator took back. All from the ledger's own rows, so the Judgment view, whose `judge.list` reads only a window, can show
// the pack's whole record: a `judge.call` row per judgment, an `onto.filed` row per judgment that filed or asked, and an
// `ontology.undo` row per write taken back (a judgment's id is its write's). Pure, with no import but the protocol's types,
// so `node --test` runs its test (`cockpit/test/ontostats.test.ts`) as it is.
import type { LedgerEntry } from '@protocol'
import { localDay } from './learning.ts'

/** The ontology pack's name in a `judge.call` row. */
export const ONTO_PACK = 'onto.v1'

/** One kind's acts and the undos of them: `rate` is undos over acts. */
export interface KindTally { kind: string; acts: number; undos: number; rate: number | null }

export interface OntoStats {
  /** Per kind (`topic`, `person`, `project`, …), by name: the acts that filed or attached under it, and how many of those
   *  writes were taken back. A write that acted on two kinds counts once under each, so these may sum past `acts`. */
  kinds: KindTally[]
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

/** The pack's counts over the ledger, or over the judgments made on one local day (`opts.day`, `2026-10-04`, the learning
 *  report's day): each of those judgments with its own filings and the undos of its write, whenever they came. An
 *  `onto.filed` or `ontology.undo` row of a judgment the rows do not hold is left out (a history that does not reach back to
 *  it), so acts and confirms never exceed the judgments. */
export function ontoStatsOf(rows: readonly LedgerEntry[], opts: { day?: string } = {}): OntoStats {
  const judgments = new Set<string>()
  for (const r of rows) {
    if (r.kind !== 'judge.call') continue
    const d = obj(r.data)
    const id = str(d.id)
    if (id !== null && d.pack === ONTO_PACK && (opts.day === undefined || localDay(new Date(r.at_unix_ms)) === opts.day)) judgments.add(id)
  }
  const acted = new Set<string>()
  const asked = new Set<string>()
  // The kinds each acted judgment filed or attached under.
  const under = new Map<string, Set<string>>()
  for (const r of rows) {
    if (r.kind !== 'onto.filed') continue
    const d = obj(r.data)
    const id = str(d.judgment)
    if (id === null || !judgments.has(id)) continue
    const steps = (Array.isArray(d.steps) ? d.steps : []).map(obj)
    const wrote = steps.filter((s) => ACTED.has(str(s.outcome) ?? ''))
    if (wrote.length) {
      acted.add(id)
      const k = under.get(id) ?? new Set<string>()
      for (const s of wrote) k.add(str(s.kind) ?? 'unknown')
      under.set(id, k)
    } else if (steps.some((s) => s.outcome === 'asked')) asked.add(id)
  }
  const undone = new Set<string>()
  for (const r of rows) {
    if (r.kind !== 'ontology.undo') continue
    const w = str(obj(r.data).write)
    if (w !== null && acted.has(w)) undone.add(w)
  }
  // A judgment that both acted and asked (one kind filed, another asked) is an act only.
  const confirms = [...asked].filter((id) => !acted.has(id)).length
  const tally = new Map<string, KindTally>()
  for (const [id, kinds] of under) {
    for (const kind of kinds) {
      const t = tally.get(kind) ?? { kind, acts: 0, undos: 0, rate: null }
      t.acts++
      if (undone.has(id)) t.undos++
      tally.set(kind, t)
    }
  }
  const kinds = [...tally.values()].sort((a, b) => a.kind.localeCompare(b.kind)).map((t) => ({ ...t, rate: t.undos / t.acts }))
  return { judgments: judgments.size, acts: acted.size, confirms, undos: undone.size, kinds }
}

const count = (n: number, one: string, many: string) => `${n.toLocaleString()} ${n === 1 ? one : many}`

/** The pack's one line: `ontology: 42 judgments · 9 acts · 5 confirms · 2 undos (22% of acts)`. The rate is left off while
 *  there are no acts to take back. */
export function ontoLine(s: OntoStats): string {
  if (s.judgments === 0) return 'ontology: no judgments yet'
  const rate = s.acts > 0 ? ` (${Math.round((s.undos / s.acts) * 100)}% of acts)` : ''
  return `ontology: ${count(s.judgments, 'judgment', 'judgments')} · ${count(s.acts, 'act', 'acts')} · ${count(s.confirms, 'confirm', 'confirms')} · ${count(s.undos, 'undo', 'undos')}${rate}`
}

/** The undo rate per kind as one line: `undo rate by kind: person 1 of 1 (100%) · topic 1 of 2 (50%)`; nothing when no act
 *  was made. */
export function ontoKindWords(s: OntoStats): string {
  if (!s.kinds.length) return ''
  return `undo rate by kind: ${s.kinds.map((k) => `${k.kind} ${k.undos} of ${k.acts} (${Math.round((k.rate ?? 0) * 100)}%)`).join(' · ')}`
}
