// The standing records on the Context page (theseus-slvl.4): the owner's orders, preferences and facts by kind, each
// with its scope, since, source and Revoke. Pure parts only; the panel is `components/StandingRecords.tsx`.
import type { StandingInfo, StandingScope } from '@protocol'

/** The kinds, in the order the model reads them, with the title a group carries. */
export const KINDS = [
  { kind: 'order', title: 'Orders', gloss: 'followed' },
  { kind: 'preference', title: 'Preferences', gloss: 'honoured where they can be' },
  { kind: 'fact', title: 'Facts about the owner', gloss: 'known' },
] as const

/** A scope in words: where the record holds. */
export function scopeWords(s: StandingScope): string {
  switch (s.kind) {
    case 'everywhere': return 'everywhere'
    case 'session': return `session ${s.of}`
    case 'place': return `place ${s.of}`
    case 'surface': return `on ${s.of}`
  }
}

/** The live records by kind, oldest first within a kind; a kind with none is left out. Ended and expired records
 *  (an `all` list's) are not shown here: the ledger keeps them. */
export function groupsOf(records: readonly StandingInfo[]) {
  return KINDS.map((k) => ({
    ...k,
    rows: records
      .filter((r) => r.state === 'live' && r.kind === k.kind)
      .sort((a, b) => a.since_ms - b.since_ms || (a.id < b.id ? -1 : 1)),
  })).filter((g) => g.rows.length)
}

/** `until`, as a record that lapses by itself says it. */
export function lapses(r: StandingInfo, now: number): string | null {
  if (r.until_ms == null) return null
  return r.until_ms > now ? `lapses ${new Date(r.until_ms).toISOString().slice(0, 16).replace('T', ' ')} UTC` : 'lapsed'
}

/** What Revoke asks first. */
export function revokeQuestion(r: StandingInfo): string {
  return `Revoke ${r.id} (${r.kind}, ${scopeWords(r.scope)})? It leaves every prompt of its scope at the next turn.`
}

/** The tally's words: how many compiles of a record's scope carried it. A surface's records ride the request's
 *  tail, which no manifest lists, so they have no tally. */
export function tallyWords(r: StandingInfo): string {
  if (r.scope.kind === 'surface') return 'in the tail of that surface’s turns'
  if (r.compiles === 0) return 'not compiled yet'
  return r.missing ? `${r.compiles - r.missing} of ${r.compiles} compiles carried it` : `all ${r.compiles} compiles carried it`
}
