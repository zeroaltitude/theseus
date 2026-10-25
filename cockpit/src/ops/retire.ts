// Old nodes retire softly (theseus-99l9z): the Fleet, the Operations page's Graph lens and the Actions page's task graph
// show what is live and recent, and a checkbox group brings each retired class back. The owner, 2026-10-24: "soft
// retire old nodes, as it's very busy, but have a checkbox option to include ended/retired/stale etc".
//
// Nothing is deleted and no counter moves: a hidden node is left out of the drawing only, and every counter counts the
// work set (`boardRows`), never what is drawn. One module for the three views, pure: `npm test` runs it
// (test/retire.test.ts), and it imports only `ops/state.ts`'s buckets.
//
// The five classes, each an item's one class (the first rule that fits):
//   live      needs you, or working: never hidden, whatever its age
//   ended     `bucket === 'ended'`, of any age
//   retired   the session's own `retired` flag (a live one is never retired: it reads live above)
//   -         touched within the cutoff (`days`, 7 by default) and not one of the above: shown
//   rest      `bucket === 'rest'` older than the cutoff: a conversation between exchanges. Its own class, NOT ended
//             (the owner: "resting is not done -- i mean, stale and done is def differnt")
//   -         scheduled for a time still ahead: shown, it is waiting with work
//   idle      open and quiet: scheduled, paused or to-do, untouched past the cutoff but within `STALE_FACTOR` cutoffs
//   stale     the same, untouched for more than `STALE_FACTOR` cutoffs
import type { SessionInfo, TaskRecord } from '@protocol'
import type { OpsRow } from './board.ts'
import { bucketOfCard, bucketOfSession, type StateBucket } from './state.ts'

export const CLASSES = ['ended', 'retired', 'stale', 'idle', 'rest'] as const
export type RetireClass = (typeof CLASSES)[number]
export type Shown = Readonly<Record<RetireClass, boolean>>

export const DAY_MS = 86_400_000
/** The cutoff, in days: anything touched within it is recent. */
export const DEFAULT_DAYS = 7
export const DAY_CHOICES: readonly number[] = [1, 3, 7, 14, 30]
/** Idle becomes stale after this many cutoffs. */
export const STALE_FACTOR = 4

export const NONE_SHOWN: Shown = { ended: false, retired: false, stale: false, idle: false, rest: false }
export const ALL_SHOWN: Shown = { ended: true, retired: true, stale: true, idle: true, rest: true }

/** What a node is classed by, whatever it is (a work-set row, a session, a task record). */
export interface Retirable {
  bucket: StateBucket
  retired?: boolean
  /** When it was last touched (ms). */
  touchedMs: number
  /** When a scheduled node is due. */
  dueMs?: number
}

/** The class an item retires into, or null when it is live or recent. The bucket is read, never a state string. */
export function classOf(x: Retirable, now: number, days = DEFAULT_DAYS): RetireClass | null {
  if (x.bucket === 'needs' || x.bucket === 'working') return null
  if (x.bucket === 'ended') return 'ended'
  if (x.retired) return 'retired'
  const age = now - x.touchedMs
  if (age <= days * DAY_MS) return null
  if (x.bucket === 'rest') return 'rest'
  if (x.bucket === 'scheduled' && x.dueMs != null && x.dueMs > now) return null
  return age > STALE_FACTOR * days * DAY_MS ? 'stale' : 'idle'
}

/** Is the item hidden under this choice? */
export const isHidden = (x: Retirable, shown: Shown, now: number, days = DEFAULT_DAYS): boolean => {
  const c = classOf(x, now, days)
  return c != null && !shown[c]
}

/** How many items each class holds, shown or not: the checkboxes' numbers. */
export function tally<T>(xs: readonly T[], item: (x: T) => Retirable, now: number, days = DEFAULT_DAYS): Record<RetireClass, number> {
  const out: Record<RetireClass, number> = { ended: 0, retired: 0, stale: 0, idle: 0, rest: 0 }
  for (const x of xs) { const c = classOf(item(x), now, days); if (c) out[c]++ }
  return out
}

/** The items split into what is drawn and what is hidden. Both together are the input: nothing is dropped. */
export function split<T>(xs: readonly T[], item: (x: T) => Retirable, shown: Shown, now: number, days = DEFAULT_DAYS): { shown: T[]; hidden: T[] } {
  const out = { shown: [] as T[], hidden: [] as T[] }
  for (const x of xs) (isHidden(item(x), shown, now, days) ? out.hidden : out.shown).push(x)
  return out
}

/** The hidden ids, less every ancestor of a node that stays drawn (its parent chain stays, as context) and less the
 *  `keepIds` (the focused and the picked). */
export function hiddenIds<T>(xs: readonly T[], id: (x: T) => string, parent: (x: T) => string | undefined, item: (x: T) => Retirable,
  shown: Shown, now: number, days = DEFAULT_DAYS, keepIds: Iterable<string> = []): Set<string> {
  const parts = split(xs, item, shown, now, days)
  const hidden = new Set(parts.hidden.map(id))
  const by = new Map(xs.map((x) => [id(x), x]))
  const spare = (start: string | undefined) => {
    for (let at = start, n = 0; at != null && n < 64; n++) {
      hidden.delete(at)
      const x = by.get(at)
      at = x ? parent(x) : undefined
    }
  }
  for (const x of parts.shown) spare(parent(x))
  for (const k of keepIds) spare(k)
  return hidden
}

// ---- the choice, kept in the address and in this browser ----

export interface Choice { show: Shown; days: number }
export const DEFAULT_CHOICE: Choice = { show: NONE_SHOWN, days: DEFAULT_DAYS }

export const SHOW_PARAM = 'show'
export const DAYS_PARAM = 'older'
export const storageKey = (view: string): string => `cockpit.retire.${view}`

export interface Store { getItem: (k: string) => string | null; setItem: (k: string, v: string) => void }

const showOf = (names: Iterable<string>): Shown => {
  const set = new Set(names)
  return Object.fromEntries(CLASSES.map((c) => [c, set.has(c)])) as unknown as Shown
}
const namesOf = (s: Shown): string[] => CLASSES.filter((c) => s[c])
const daysOf = (raw: unknown): number | undefined => {
  const n = typeof raw === 'string' ? Number(raw) : typeof raw === 'number' ? raw : NaN
  return Number.isInteger(n) && n >= 1 && n <= 365 ? n : undefined
}

/** What this browser kept for the view, or the default; a bad value is the default. */
export function readStored(store: Store | undefined, view: string): Choice {
  try {
    const o = JSON.parse(store?.getItem(storageKey(view)) ?? 'null') as { show?: unknown; days?: unknown } | null
    return { show: showOf(Array.isArray(o?.show) ? (o.show as unknown[]).filter((s): s is string => typeof s === 'string') : []), days: daysOf(o?.days) ?? DEFAULT_DAYS }
  } catch { return DEFAULT_CHOICE }
}

/** The choice: the address wins (a shared link keeps what its sender saw), field by field; else this browser's; else
 *  the default. `?show=` is the checked classes, comma-separated, or `none`; `?older=` the cutoff in days. */
export function readChoice(params: URLSearchParams, store: Store | undefined, view: string): Choice {
  const kept = readStored(store, view)
  const raw = params.get(SHOW_PARAM)
  return {
    show: raw == null ? kept.show : showOf(raw.split(',')),
    days: daysOf(params.get(DAYS_PARAM)) ?? kept.days,
  }
}

/** The address with the choice written; every other parameter stays. */
export function writeParams(params: URLSearchParams, c: Choice): URLSearchParams {
  const next = new URLSearchParams(params)
  const names = namesOf(c.show)
  next.set(SHOW_PARAM, names.length ? names.join(',') : 'none')
  if (c.days === DEFAULT_DAYS) next.delete(DAYS_PARAM)
  else next.set(DAYS_PARAM, String(c.days))
  return next
}

/** Keep the choice in this browser for the view; with no storage it lasts as long as the address. */
export function writeStored(store: Store | undefined, view: string, c: Choice): void {
  try { store?.setItem(storageKey(view), JSON.stringify({ show: namesOf(c.show), days: c.days })) } catch { /* the address stands */ }
}

/** The chip's words: "+214 older". */
export const olderWords = (n: number): string => `+${n} older`

export const CLASS_WORDS: Readonly<Record<RetireClass, string>> = {
  ended: 'ended', retired: 'retired', stale: 'stale', idle: 'idle', rest: 'at rest',
}

// ---- the three views' items ----

/** A work-set row: its own bucket (`boardRows`), its session's retired flag, its last touch. */
export const rowItem = (r: OpsRow): Retirable => ({ bucket: r.bucket, retired: r.session?.retired != null, touchedMs: r.activityMs, dueMs: r.dueMs })

/** A session of the Fleet: the bucket of its attention and execution, as the time machine's board reads it. */
export const sessionItem = (s: SessionInfo): Retirable => ({ bucket: bucketOfSession(s), retired: s.retired != null, touchedMs: s.last_active_ms })

/** A task record of the Actions page's task graph. */
export const recordItem = (t: TaskRecord, now: number): Retirable => ({ bucket: bucketOfCard({ kind: 'step', record: t }, now), touchedMs: t.updated_at_ms })
