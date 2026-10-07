// The Ship's flare for a failure (theseus-1skt): a failure flares its ship once, whichever source tells the page first.
// Three can tell it:
//
//   turn.failed, the push          for the sessions the Ship watches (those that work or wait for you)
//   execution.changed into failed   for every session; it names no turn
//   turn.failed, the ledger's row   for every session, from the page's one copy of the ledger, a few seconds later:
//                                   the only word of a turn that fails before it runs (a model the daemon cannot
//                                   price) in a session the Ship does not watch
//
// A failed turn is one failure however many sources say it: its session and its turn, which the push and the row both
// name. execution.changed names no turn, so it and a failure of its session within SAME_FAILURE_MS are one, judged on
// the daemon's clock when both say their time (the execution's frame, the row), else on the page's (two pushes). A row
// from before the page opened is history, not news. Pure, like the horn's cue table (`sound.ts`), which hears the same
// rows.
import type { LedgerEntry } from '@protocol'

/** A failure, as one source tells it. */
export interface Failure {
  session: string
  /** The failed turn, when the source names it (the push and the row do; execution.changed does not). */
  turn?: string | null
  /** When the page heard it (ms, the page's clock). */
  heard: number
  /** When it failed on the daemon's clock, when the source says (the execution's frame, the row; the push does not). */
  at?: number
}

/** What the flares remember: the failed turns already flared, and each session's last flare. */
export interface Flares {
  turns: Set<string>
  last: Map<string, { heard: number; at?: number; turn: string | null }>
}

export const newFlares = (): Flares => ({ turns: new Set(), last: new Map() })

/** Two sources that say a failure of one session this close together (ms), one of them naming no turn, say one
 *  failure: the horn's own window for a session (`sound.ts`). */
export const SAME_FAILURE_MS = 10_000

/** The failed turns remembered, the newest: a turn's push and its row come seconds apart. */
const KEEP = 1000

/** Whether a failure flares its ship: the first time the page hears of it, from whichever source (and remember it). */
export function flareOf(f: Flares, x: Failure): boolean {
  const key = x.turn ? `${x.session} ${x.turn}` : null
  if (key) {
    if (f.turns.has(key)) return false
    if (f.turns.size >= KEEP) f.turns.delete(f.turns.values().next().value!)
    f.turns.add(key)
  }
  const was = f.last.get(x.session)
  if (was && (!key || was.turn === null)) {
    const apart = x.at !== undefined && was.at !== undefined ? x.at - was.at : x.heard - was.heard
    if (Math.abs(apart) < SAME_FAILURE_MS) {
      // The same failure. Once a source names its turn, another failed turn of the session flares again.
      if (key) f.last.set(x.session, { ...was, turn: key })
      return false
    }
  }
  f.last.set(x.session, { heard: x.heard, at: x.at, turn: key })
  return true
}

/** The sessions whose ships new ledger rows flare: each turn.failed row from `since` (when the page opened), once with
 *  the push or the execution's change that may have told it first. `heard`: now, on the page's clock. */
export function flaresOfRows(f: Flares, rows: readonly Pick<LedgerEntry, 'kind' | 'session_id' | 'turn_id' | 'at_unix_ms'>[], since: number, heard: number): string[] {
  const out: string[] = []
  for (const r of rows) {
    if (r.kind !== 'turn.failed' || !r.session_id || r.at_unix_ms < since) continue
    if (flareOf(f, { session: r.session_id, turn: r.turn_id, heard, at: r.at_unix_ms })) out.push(r.session_id)
  }
  return out
}
