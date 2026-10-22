// The ontology pack's counts as the cockpit reads them (theseus-wzvxp, R7d): how many judgments `onto.v1` made, how many acted
// (filed a category or attached a project) and how many only asked (the confirm band): the day's activity. All from the
// ledger's own rows, so the Judgment view, whose `judge.list` reads only a window, can show the pack's whole record: a
// `judge.call` row per judgment and an `onto.filed` row per judgment that filed or asked. How much of it the owner took back
// is not counted here (theseus-ukmbn): the undo rate has one definition, the daemon's (`onto_undo::undo_rates`: Jev's
// writes per kind in the 7 days to the report, and the share an `ontology.undo` names), stored on the learning report's
// `onto.v1` pack as `undo`, and `ontoKindWords` words it. Pure, with no import but the protocol's types, so `node --test`
// runs its test (`cockpit/test/ontostats.test.ts`) as it is.
import type { LedgerEntry, UndoRate } from '@protocol'
import { localDay } from './learning.ts'

/** The ontology pack's name in a `judge.call` row. */
export const ONTO_PACK = 'onto.v1'

export interface OntoStats {
  /** `onto.v1` judgments: every `judge.call` row of the pack, whatever its outcome. */
  judgments: number
  /** Of those, the ones that wrote: at least one step filed a category or attached a project. */
  acts: number
  /** The ones that wrote nothing and asked: at least one step in the confirm band, none acted. */
  confirms: number
}

type D = Record<string, unknown>
const obj = (v: unknown): D => (v && typeof v === 'object' && !Array.isArray(v) ? (v as D) : {})
const str = (v: unknown): string | null => (typeof v === 'string' ? v : null)

/** A step's outcome in an `onto.filed` row that wrote. */
const ACTED = new Set(['filed', 'attached'])

/** The pack's counts over the ledger, or over the judgments made on one local day (`opts.day`, `2026-10-04`, the learning
 *  report's day): each of those judgments with its own filings, whenever they came. An `onto.filed` row of a judgment the
 *  rows do not hold is left out (a history that does not reach back to it), so acts and confirms never exceed the
 *  judgments. */
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
  for (const r of rows) {
    if (r.kind !== 'onto.filed') continue
    const d = obj(r.data)
    const id = str(d.judgment)
    if (id === null || !judgments.has(id)) continue
    const steps = (Array.isArray(d.steps) ? d.steps : []).map(obj)
    if (steps.some((s) => ACTED.has(str(s.outcome) ?? ''))) acted.add(id)
    else if (steps.some((s) => s.outcome === 'asked')) asked.add(id)
  }
  // A judgment that both acted and asked (one kind filed, another asked) is an act only.
  const confirms = [...asked].filter((id) => !acted.has(id)).length
  return { judgments: judgments.size, acts: acted.size, confirms }
}

const count = (n: number, one: string, many: string) => `${n.toLocaleString()} ${n === 1 ? one : many}`

/** The pack's one line, the day's activity: `ontology: 42 judgments · 9 acts · 5 confirms`. */
export function ontoLine(s: OntoStats): string {
  if (s.judgments === 0) return 'ontology: no judgments yet'
  return `ontology: ${count(s.judgments, 'judgment', 'judgments')} · ${count(s.acts, 'act', 'acts')} · ${count(s.confirms, 'confirm', 'confirms')}`
}

/** The stored report's undo rate per kind as one line, with its window, in the CLI's words (`theseus judge report`):
 *  `undo rate by kind (Jev's writes, 7 days to 2026-10-21): person 1 of 1 (100%) · topic 1 of 2 (50%)`; nothing when the
 *  report holds none. `end` is the report's day, where the window ends. */
export function ontoKindWords(undo: readonly UndoRate[], end: string): string {
  if (!undo.length) return ''
  return `undo rate by kind (Jev's writes, 7 days to ${end}): ${undo.map((u) => `${u.kind} ${u.undone} of ${u.writes} (${Math.round((u.rate ?? 0) * 100)}%)`).join(' · ')}`
}
