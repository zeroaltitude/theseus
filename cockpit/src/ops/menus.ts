// The model and mode menus (theseus-jid7t.6), pure: what each profile row says and whether the floor greys it, and the
// sticky choice a session holds. The daemon decides (`session.profile`'s `refused`); the menu greys what it would
// refuse, by `floor.ts`, and never hides what it would allow.
import type { DataMark, LedgerEntry, ProfileInfo, SessionInfo, SessionMode } from '@protocol'
import { greyOf } from './floor.ts'
import type { Price } from '../lib/money.ts'

export interface ProfileRow {
  name: string
  model: string
  origin: string
  live: boolean
  /** The session's profile now. */
  current: boolean
  /** Sessions already on it, the menu's "N on it". */
  onIt: number
  /** "~4× the cost", "~2× cheaper" or '' when the catalog prices one of the two unknown. */
  hint: string
  /** The floor's words when the session's data refuses this profile; null when it runs. */
  grey: string | null
}

/** A profile's cost against a base one: the sum of its input and output rates. */
export function costHint(p: Price | undefined, base: Price | undefined): string {
  if (!p || !base) return ''
  const a = p.input + p.output
  const b = base.input + base.output
  if (!(a > 0) || !(b > 0)) return ''
  const r = a / b
  if (r >= 1.5) return `~${Math.round(r)}× the cost`
  if (r <= 1 / 1.5) return `~${Math.round(1 / r)}× cheaper`
  return ''
}

export function profileRows(profiles: readonly ProfileInfo[], sessions: readonly SessionInfo[], mark: DataMark | undefined, current: string | undefined, prices: ReadonlyMap<string, Price>): ProfileRow[] {
  const base = current ? prices.get(profiles.find((p) => p.name === current)?.model ?? '') : undefined
  return profiles.map((p) => ({
    name: p.name, model: p.model, origin: p.origin ?? 'unknown', live: p.live, current: p.name === current,
    onIt: sessions.filter((s) => s.profile === p.name && !s.retired).length,
    hint: p.name === current ? '' : costHint(prices.get(p.model), base),
    grey: greyOf(mark, p.name, p),
  }))
}

/** The newest `session.profile_set` row of a session: what it was set to (null: let go) and when. */
function newestSet(rows: readonly Pick<LedgerEntry, 'kind' | 'session_id' | 'data' | 'at_unix_ms'>[], sessionId: string): { to: string | null; at: number } | null {
  for (let i = rows.length - 1; i >= 0; i--) {
    const r = rows[i]
    if (r.kind === 'session.profile_set' && r.session_id === sessionId) return { to: ((r.data as { to?: string | null } | null)?.to ?? null) || null, at: r.at_unix_ms ?? 0 }
  }
  return null
}

/** The sticky profile a session holds, from the ledger's `session.profile_set` rows: the newest one's `to` (null: let go). */
export function stickyOf(rows: readonly Pick<LedgerEntry, 'kind' | 'session_id' | 'data' | 'at_unix_ms'>[], sessionId: string): string | null {
  return newestSet(rows, sessionId)?.to ?? null
}

/** When the daemon wrote the session's newest `session.profile_set` row (0 if none): the clock `hold` stamps a choice
 *  with, so `stickyWith` compares the daemon's time with the daemon's, never with this browser's. */
export function heldAt(rows: readonly Pick<LedgerEntry, 'kind' | 'session_id' | 'data' | 'at_unix_ms'>[], sessionId: string): number {
  return newestSet(rows, sessionId)?.at ?? 0
}

/** What this page just set (`at`, its clock), which shows until the ledger's own newer row arrives: the ledger is read
 *  a little behind the daemon, and a choice should show when it is made. A row written after the choice wins. */
export function stickyWith(rows: readonly Pick<LedgerEntry, 'kind' | 'session_id' | 'data' | 'at_unix_ms'>[], sessionId: string, held?: { to: string | null; at: number }): string | null {
  const row = newestSet(rows, sessionId)
  return held && (!row || held.at >= row.at) ? held.to : row?.to ?? null
}

/** What a refused `session.profile` or a `note` says in the toast, in the daemon's words. */
export function profileToast(r: { refused?: string; note?: string; profile: string | null; previous: string | null }, asked: string | null): { text: string; undo: string | null | undefined } {
  if (r.refused) return { text: r.refused, undo: undefined }
  const what = asked ? `from now on: ${asked}` : 'back to the default'
  return { text: `${what}${r.note ? ` · ${r.note}` : ''}`, undo: r.profile === r.previous ? undefined : r.previous }
}

export const MODES: ReadonlyArray<{ id: SessionMode; label: string; words: string }> = [
  { id: 'ask', label: 'Ask', words: 'asks before it changes anything' },
  { id: 'edits', label: 'Edits', words: 'edits files without asking, asks for the rest' },
  { id: 'plan', label: 'Plan', words: 'reads and plans, changes nothing' },
  { id: 'auto', label: 'Auto', words: 'acts without asking: the floor still asks' },
]

/** Auto asks first: it widens what the session may do without a question. */
export const modeAsk = (m: SessionMode): string | undefined => (m === 'auto' ? 'Set this session to Auto? It then acts without asking (the floor still asks).' : undefined)

