// The owner's standing records (theseus-slvl.4): what every prompt of a scope carries until it is revoked, by kind,
// each with its scope, since, source and Revoke. A read of `standing.list`, only while it is on screen; Revoke is
// `standing.revoke`, confirmed first, which the core judges as the owner's act from a private place (this connection).
import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Pin } from 'lucide-react'
import type { StandingInfo, StandingListResult } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { when } from '@/lib/explorer'
import { Empty, Field, Panel, Pill } from '@/components/ui'
import { groupsOf, lapses, revokeQuestion, scopeWords, tallyWords } from '@/lib/standing'
import { ShipsWheel } from '@/components/ShipsWheel'

export function StandingRecords() {
  const [now] = useState(Date.now)
  const { data } = useRpc<StandingListResult>('standing.list', undefined, 10_000)
  const groups = groupsOf(data?.records ?? [])
  return (
    <Panel help="context.standing" title={<>standing · {groups.reduce((n, g) => n + g.rows.length, 0)}</>} icon={<Pin size={13} />}>
      {!data && <Empty>reading the records…</Empty>}
      {data && !groups.length && <Empty>No standing records. <span className="num">theseus standing add "never push to main"</span> makes an order.</Empty>}
      {groups.map((g) => (
        <div key={g.kind} className="mb-3">
          <div className="panel-title mb-1">{g.title} <span className="text-ink-faint normal-case">· {g.gloss}</span></div>
          {g.rows.map((r) => <Record key={r.id} r={r} now={now} />)}
        </div>
      ))}
      <div className="text-[11px] text-ink-faint">A record in the terminal, voice, Discord, cockpit or MCP scope rides the tail of that surface’s turns, so the system prompt is the same on all of them.</div>
    </Panel>
  )
}

function Record({ r, now }: { r: StandingInfo; now: number }) {
  const lapse = lapses(r, now)
  return (
    <div className="mb-2 border-b border-white/5 pb-2">
      <div className="flex items-center gap-2">
        <span className="num text-[11.5px] text-ink-dim" title={r.id}>{r.id}</span>
        <Pill tone={r.scope.kind === 'everywhere' ? 'ok' : 'live'}>{scopeWords(r.scope)}</Pill>
        <span className="ml-auto"><Revoke r={r} /></span>
      </div>
      <div className="mt-1 whitespace-pre-wrap break-words text-[12.5px]">{r.text}</div>
      <Field label="since" mono>{when(r.since_ms)}{lapse && ` · ${lapse}`}</Field>
      <Field label="source" mono>{r.source}</Field>
      <Field label="carried" mono>{tallyWords(r)}</Field>
    </div>
  )
}

/** standing.revoke: the record leaves every prompt of its scope at the next turn. Confirmed first. */
function Revoke({ r }: { r: StandingInfo }) {
  const qc = useQueryClient()
  const [busy, setBusy] = useState(false)
  const revoke = async () => {
    if (!window.confirm(revokeQuestion(r))) return
    setBusy(true)
    try { await call('standing.revoke', { id: r.id }); await qc.invalidateQueries() } catch (e: any) { window.alert(e?.message ?? String(e)) } finally { setBusy(false) }
  }
  return (
    <button onClick={revoke} disabled={busy} className="rounded px-1.5 py-0.5 text-[10.5px] text-fault ring-1 ring-fault/30 hover:bg-fault/10 disabled:opacity-50">
      {busy ? <ShipsWheel label="revoking" /> : 'Revoke'}
    </button>
  )
}
