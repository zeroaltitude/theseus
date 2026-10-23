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

/** The sticky profile a session holds, from the ledger's `session.profile_set` rows: the newest one's `to` (null: let go). */
export function stickyOf(rows: readonly Pick<LedgerEntry, 'kind' | 'session_id' | 'data'>[], sessionId: string): string | null {
  for (let i = rows.length - 1; i >= 0; i--) {
    const r = rows[i]
    if (r.kind === 'session.profile_set' && r.session_id === sessionId) return ((r.data as { to?: string | null } | null)?.to ?? null) || null
  }
  return null
}

export const MODES: ReadonlyArray<{ id: SessionMode; label: string; words: string }> = [
  { id: 'ask', label: 'Ask', words: 'asks before it changes anything' },
  { id: 'edits', label: 'Edits', words: 'edits files without asking, asks for the rest' },
  { id: 'plan', label: 'Plan', words: 'reads and plans, changes nothing' },
  { id: 'auto', label: 'Auto', words: 'acts without asking: the floor still asks' },
]

/** Auto asks first: it widens what the session may do without a question. */
export const modeAsk = (m: SessionMode): string | undefined => (m === 'auto' ? 'Set this session to Auto? It then acts without asking (the floor still asks).' : undefined)

/** What a refused `session.profile` or a `note` says in the toast, in the daemon's words. */
export function profileToast(r: { refused?: string; note?: string; profile: string | null; previous: string | null }, asked: string | null): { text: string; undo: string | null | undefined } {
  if (r.refused) return { text: r.refused, undo: undefined }
  const what = asked ? `from now on: ${asked}` : 'back to the default'
  return { text: `${what}${r.note ? ` · ${r.note}` : ''}`, undo: r.profile === r.previous ? undefined : r.previous }
}
