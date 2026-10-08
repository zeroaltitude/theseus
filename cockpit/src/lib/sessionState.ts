// A session's state (theseus-emqx): live, quiet or retired, as the daemon derives it (theseus-protocol's
// `sessions::derive`), and the Ship's filter over it. Pure: `npm test` runs it (test/sessionState.test.ts).
//
// - Live: a turn within the window (`live_window_ms`, 24 hours by default), or busy (it runs, is queued, or waits on
//   you) whatever its age.
// - Quiet: not retired, and no turn within the window.
// - Retired: superseded (its place moved to a newer session), retired by hand, or empty (no turn, past the grace).
//
// The daemon names each session's state in `session.list`; `deriveState` is the same rule for a past moment, which the
// time machine folds (`timemachine.ts`), with the windows `session.list` names. Nothing is ever deleted: the filter
// only chooses what the Ship draws, and a selected session always shows (a visitor), whatever the filter.
import type { RetiredReason, SessionInfo, SessionLink, SessionRetired, SessionState } from '@protocol'

export type ShipFilter = SessionState | 'all'

export const FILTERS: { key: ShipFilter; word: string; title: string }[] = [
  { key: 'live', word: 'Live', title: 'At sea: a turn in the last day, or working, or waiting for you' },
  { key: 'quiet', word: 'Quiet', title: 'At anchor in the roads: no turn in the last day' },
  { key: 'retired', word: 'Retired', title: 'Laid up in harbour: replaced by a newer session, retired by hand, or never used. Nothing is deleted.' },
  { key: 'all', word: 'All', title: 'Every session, whatever its state' },
]

/** The address's `st`: absent is Live, the default. */
export function filterOf(param: string | null | undefined): ShipFilter {
  return param === 'quiet' || param === 'retired' || param === 'all' ? param : 'live'
}

/** The `st` a filter writes: none for Live, so a link with no parameter shows Live. */
export function paramOf(f: ShipFilter): string | null {
  return f === 'live' ? null : f
}

export interface StateRule { live_window_ms: number; empty_grace_ms: number }
export const DEFAULT_RULE: StateRule = { live_window_ms: 24 * 3_600_000, empty_grace_ms: 3_600_000 }

export interface StateOf {
  retired?: SessionRetired | null
  turns: number
  created_ms: number
  last_active_ms: number
  reopened_ms?: number | null
  busy: boolean
}

/** theseus-protocol's `sessions::derive`, line for line: the state at `now`, and the retirement that makes it so. */
export function deriveState(s: StateOf, rule: StateRule, now: number): { state: SessionState; retired?: SessionRetired } {
  const emptyAt = s.created_ms + rule.empty_grace_ms
  const derived: SessionRetired | undefined = s.retired ?? (s.turns === 0 && s.reopened_ms == null && now >= emptyAt ? { reason: 'empty', at_ms: emptyAt } : undefined)
  if (s.busy) return { state: 'live', ...(s.retired ? { retired: s.retired } : {}) }
  if (derived) return { state: 'retired', retired: derived }
  const active = Math.max(s.last_active_ms, s.created_ms, s.reopened_ms ?? 0)
  return { state: now - active < rule.live_window_ms ? 'live' : 'quiet' }
}

/** Busy: it runs or is queued, or it waits on you. */
export function busyOf(s: Pick<SessionInfo, 'execution_state' | 'pending_confirms' | 'attention'>): boolean {
  return s.execution_state === 'running' || s.execution_state === 'queued' || s.pending_confirms > 0 || s.attention?.level === 'needs_you'
}

/** A session's state: the daemon's, or Live from a daemon older than the states (it names none). */
export function stateOf(s: Pick<SessionInfo, 'state'>): SessionState {
  return s.state ?? 'live'
}

export const REASON_WORDS: Record<RetiredReason, string> = { superseded: 'superseded', empty: 'empty', by_hand: 'by hand' }

/** The badge's word: `live`, `quiet`, `retired · superseded`. */
export function badgeWord(s: Pick<SessionInfo, 'state' | 'retired'>): string {
  const st = stateOf(s)
  if (st === 'retired' && s.retired) return `retired · ${REASON_WORDS[s.retired.reason]}`
  return st
}

/** The badge's nautical word: at sea, at anchor, laid up. */
export function seaWordOf(s: Pick<SessionInfo, 'state'>): string {
  const st = stateOf(s)
  return st === 'live' ? 'at sea' : st === 'quiet' ? 'at anchor' : 'laid up'
}

export type Counts = Record<ShipFilter, number>

type Fleetish = Pick<SessionInfo, 'session_id' | 'kind' | 'state' | 'parent_session_id'>

/** How many conversations each filter shows (a task sails with its parent). */
export function countsOf(sessions: Fleetish[]): Counts {
  const c: Counts = { live: 0, quiet: 0, retired: 0, all: 0 }
  for (const s of sessions) {
    if (s.kind === 'task') continue
    c[stateOf(s)]++
    c.all++
  }
  return c
}

/** The sessions the Ship draws under `filter`: each conversation in its state, each task with the conversation that
 *  started it (or in its own state), and the visitors (the selected session, one the address flies to), with the
 *  sessions that started them, whatever their state. */
export function shownIds(sessions: Fleetish[], filter: ShipFilter, visitors: (string | null | undefined)[] = []): Set<string> {
  const byId = new Map(sessions.map((s) => [s.session_id, s]))
  const root = (s: Fleetish): Fleetish => {
    let r = s
    for (let i = 0; i < 16 && r.parent_session_id; i++) {
      const p = byId.get(r.parent_session_id)
      if (!p) break
      r = p
    }
    return r
  }
  const matches = (s: Fleetish) => filter === 'all' || stateOf(s) === filter
  const out = new Set<string>()
  for (const s of sessions) {
    if (matches(s) || (s.kind === 'task' && matches(root(s)))) out.add(s.session_id)
  }
  for (const v of visitors) {
    let s = v ? byId.get(v) : undefined
    for (let i = 0; s && i < 16; i++) {
      out.add(s.session_id)
      s = s.parent_session_id ? byId.get(s.parent_session_id) : undefined
    }
  }
  return out
}

/** "Replaced by ses_… on 8 Oct" and "Replaces ses_…": the links' words, with the other session's title when known. */
export function linkWords(link: SessionLink | undefined, way: 'by' | 'replaces', titleOf: (id: string) => string | undefined, date: (ms: number) => string): string | null {
  if (!link) return null
  const t = titleOf(link.session_id)
  const who = t ? `“${t}”` : link.session_id
  return way === 'by' ? `Replaced by ${who} on ${date(link.at_ms)}` : `Replaces ${who}`
}

/** The palette's order: every session, the retired ones after the others, each group as given. */
export function paletteOrder<T extends Pick<SessionInfo, 'state'>>(sessions: T[]): T[] {
  return [...sessions.filter((s) => stateOf(s) !== 'retired'), ...sessions.filter((s) => stateOf(s) === 'retired')]
}

/** What a search matches in a session: its title, its label, its old titles, and its id. */
export function searchText(s: Pick<SessionInfo, 'session_id' | 'title' | 'label' | 'title_was'>): string {
  return [s.title, s.label, ...(s.title_was ?? []), s.session_id].filter(Boolean).join(' ')
}

/** What the ledger says of a session's state (theseus-emqx), as the time machine folds it. */
export interface Life {
  retired?: SessionRetired
  supersededBy?: SessionLink
  supersedes?: SessionLink
  reopened?: number
}

/** The kinds of row `foldLife` reads. */
export const LIFE_KINDS = new Set(['session.superseded', 'session.retired', 'session.reopened'])

/** One row into the lives it changes, by session: `session.superseded` (both ways), `session.retired` and
 *  `session.reopened`. Each life is replaced, never mutated. */
export function foldLife(lives: Map<string, Life>, r: { kind: string; at_unix_ms: number; session_id?: string | null; data?: unknown }): void {
  const sid = r.session_id
  if (!sid || !LIFE_KINDS.has(r.kind)) return
  const d = (r.data ?? {}) as Record<string, unknown>
  const at = r.at_unix_ms
  const was = lives.get(sid) ?? {}
  if (r.kind === 'session.superseded' && typeof d.superseded_by === 'string') {
    const place = typeof d.place === 'string' ? d.place : undefined
    lives.set(sid, { ...was, supersededBy: { session_id: d.superseded_by, at_ms: at, ...(place ? { place } : {}) }, retired: was.retired ?? { reason: 'superseded', at_ms: at } })
    const next = lives.get(d.superseded_by) ?? {}
    lives.set(d.superseded_by, { ...next, supersedes: { session_id: sid, at_ms: at, ...(place ? { place } : {}) } })
  } else if (r.kind === 'session.retired') {
    if (!was.retired) lives.set(sid, { ...was, retired: { reason: (d.reason as RetiredReason) ?? 'by_hand', at_ms: at } })
  } else if (r.kind === 'session.reopened') {
    const { retired: _gone, ...rest } = was
    lives.set(sid, { ...rest, reopened: at })
  }
}

/** The windows a `session.list` answer names, or the defaults from a daemon older than the states. */
export function ruleOf(r: { live_window_ms?: number; empty_grace_ms?: number }): StateRule {
  return { live_window_ms: r.live_window_ms ?? DEFAULT_RULE.live_window_ms, empty_grace_ms: r.empty_grace_ms ?? DEFAULT_RULE.empty_grace_ms }
}
