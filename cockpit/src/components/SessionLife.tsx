// A session's state on screen (theseus-emqx): its badge (live, quiet, retired and why), its links ("Replaced by …",
// "Replaces …"), and Retire and Reopen, each confirmed first and judged by the core as the owner's act
// (`session.retire`, `session.reopen`). Shared by the Ship's cards, the session deck and the Fleet.
import { useState } from 'react'
import { Link } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import type { SessionLink, SessionRetired, SessionState } from '@protocol'
import { call } from '@/lib/rpc'
import { cn, stamp } from '@/lib/format'
import { REASON_WORDS } from '@/lib/sessionState'

const TONE: Record<SessionState, string> = {
  live: 'border-live/50 text-live',
  quiet: 'border-line text-ink-dim',
  retired: 'border-line text-ink-faint',
}
const SEA: Record<SessionState, string> = { live: 'at sea', quiet: 'at anchor', retired: 'laid up' }

export function StateBadge({ state, retired, className }: { state?: SessionState; retired?: SessionRetired; className?: string }) {
  const st = state ?? 'live'
  const why = st === 'retired' && retired ? ` · ${REASON_WORDS[retired.reason]}` : ''
  const title = st === 'live' ? 'Live: a turn in the last day, or working, or waiting for you'
    : st === 'quiet' ? 'Quiet: no turn in the last day'
      : `Retired${retired ? ` (${REASON_WORDS[retired.reason]}) on ${stamp(retired.at_ms)}` : ''}. Nothing is deleted; Reopen brings it back.`
  return (
    <span title={title} className={cn('num inline-flex items-center gap-1 rounded border px-1.5 text-[10.5px] leading-[16px]', TONE[st], className)}>
      {st}{why} <span className="text-ink-faint">· {SEA[st]}</span>
    </span>
  )
}

/** "Replaced by <session> on <date>" and "Replaces <session>", each a link to the other session. */
export function SessionLinks({ by, replaces, titleOf, to }: { by?: SessionLink; replaces?: SessionLink; titleOf?: (id: string) => string | undefined; to?: (id: string) => string }) {
  if (!by && !replaces) return null
  const href = to ?? ((id: string) => `/session/${id}`)
  const name = (id: string) => titleOf?.(id) ?? id
  return (
    <div className="flex flex-col gap-0.5 text-[11.5px]">
      {by && <span className="text-ink-dim">⚑ Replaced by <Link className="text-brass underline-offset-2 hover:underline" to={href(by.session_id)}>{name(by.session_id)}</Link> on {stamp(by.at_ms)}{by.place ? ` (${by.place})` : ''}</span>}
      {replaces && <span className="text-ink-dim">Replaces <Link className="text-brass underline-offset-2 hover:underline" to={href(replaces.session_id)}>{name(replaces.session_id)}</Link></span>}
    </div>
  )
}

/** Retire (or Reopen, for a retired session), confirmed first. Off under the time machine (`disabled`). */
export function RetireButton({ sessionId, state, retired, disabled }: { sessionId: string; state?: SessionState; retired?: SessionRetired; disabled?: boolean }) {
  const qc = useQueryClient()
  const [busy, setBusy] = useState(false)
  const reopen = state === 'retired' || (!!retired && retired.reason !== 'empty')
  const act = async () => {
    const ask = reopen
      ? `Reopen ${sessionId}? It shows as live or quiet again. A replaced session keeps its links, and its place stays on the newer one.`
      : `Retire ${sessionId}? It leaves the Ship's default view, and its place's next message starts a fresh session. Nothing is deleted; Reopen brings it back.`
    if (!window.confirm(ask)) return
    setBusy(true)
    try {
      await call(reopen ? 'session.reopen' : 'session.retire', { session_id: sessionId })
      await qc.invalidateQueries()
    } catch (e: unknown) {
      window.alert((e as { message?: string })?.message ?? String(e))
    } finally {
      setBusy(false)
    }
  }
  return (
    <button type="button" className="brass-button" disabled={busy || disabled} onClick={act}
      title={disabled ? 'The time machine shows the past: its acts are off' : reopen ? 'Reopen: clear its retirement (session.reopen)' : 'Retire by hand: nothing is deleted (session.retire)'}>
      {reopen ? 'Reopen' : 'Retire'}
    </button>
  )
}
