// What Theseus changed about itself (theseus-pw1q.4): `self.log`'s rows, newest first (the self steps' rows and
// today's self-changes: pack moves, learned packs, extensions, routing corrections, syntheses), each with what, why,
// its numbers and its undo, and the kill switch's state with a Halt button (`self.halt`, anyone's, confirmed first).
// Each self-join row carries a Revert button: the owner's veto (`self.revert`, one click; the daemon refuses anyone
// else's), which undoes the join off main and drops its backlog item for good.
// The resume is the owner's alone: `theseus self resume` (or "resume self" from the owner's Discord account), which the card names and never sends.
import { CARD_BODY } from '@/lib/cardfit'
import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Link } from 'react-router'
import { OctagonX } from 'lucide-react'
import type { SelfLogResult } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { ago } from '@/lib/format'
import { Empty, Field, Panel, Pill } from '@/components/ui'
import { kindWords, numbersLine, revertableMerge, revertedAlready, stateWords, switchLine } from '@/lib/selfchanges'
import { ShipsWheel } from '@/components/ShipsWheel'

export function SelfChangesCard({ now }: { now: number }) {
  const { data } = useRpc<SelfLogResult>('self.log', { limit: 12 }, 5000)
  const s = data?.state
  const w = s ? stateWords(s) : undefined
  return (
    <Panel help="systems.selfchanges" title="What Theseus changed about itself" icon={<OctagonX size={13} />} bodyClassName={CARD_BODY}>
      {!data && <Empty>reading the self log…</Empty>}
      {s && w && (
        <div className="mb-2 flex items-center gap-2">
          <Pill tone={w.tone}>{w.label}</Pill>
          <span className="text-[11px] text-ink-dim">{switchLine(s)}</span>
          <span className="ml-auto">{!s.halted || s.never_resumed ? <Halt /> : null}</span>
        </div>
      )}
      {data?.building && <Empty>The ledger's index is still being built after a start.</Empty>}
      {data && !data.building && !data.rows.length && <Empty>Theseus has changed nothing about itself yet.</Empty>}
      {data?.rows.map((r) => (
        <div key={r.position} className="mb-1.5 border-b border-white/5 pb-1.5">
          <Field label={`${kindWords(r)} · ${ago(r.at_unix_ms, now)}`}>
            {r.what}
            {r.why && <span className="text-ink-dim"> ({r.why})</span>}
          </Field>
          {numbersLine(r.numbers) && <Field label="numbers" mono>{numbersLine(r.numbers)}</Field>}
          {r.undo && <Field label="undo" mono>{r.undo}</Field>}
          {revertableMerge(r) && !revertedAlready(data?.rows ?? [], revertableMerge(r)!) && <Revert merge={revertableMerge(r)!} />}
        </div>
      ))}
      <div className="mt-1.5 flex gap-3 text-[11px]">
        {data?.more && <span className="text-ink-dim">older: theseus self log --since 30d</span>}
        <Link to="/ledger?kind=self.halted" className="text-live hover:underline">halts in the ledger →</Link>
      </div>
    </Panel>
  )
}

/** self.halt: every self step stops before its next phase, until the owner resumes. Confirmed first. */
function Halt() {
  const qc = useQueryClient()
  const [busy, setBusy] = useState(false)
  const halt = async () => {
    const why = window.prompt('Halt all self-directed work? Only the owner resumes it (theseus self resume). Why, in a few words:')
    if (why === null) return
    setBusy(true)
    try { await call('self.halt', why.trim() ? { why: why.trim() } : {}); await qc.invalidateQueries() } catch (e: any) { window.alert(e?.message ?? String(e)) } finally { setBusy(false) }
  }
  return (
    <button onClick={halt} disabled={busy} className="rounded px-1.5 py-0.5 text-[10.5px] text-fault ring-1 ring-fault/30 hover:bg-fault/10 disabled:opacity-50">
      {busy ? <ShipsWheel label="halting" /> : 'Halt'}
    </button>
  )
}

/** self.revert: the owner's veto of one self-join. It runs main's gate on the revert, so it can take minutes; the daemon says no to anyone but the owner. */
function Revert({ merge }: { merge: string }) {
  const qc = useQueryClient()
  const [busy, setBusy] = useState(false)
  const revert = async () => {
    if (!window.confirm(`Revert self-join ${merge.slice(0, 12)} off main? Its backlog item is dropped and never filed again.`)) return
    setBusy(true)
    try { await call('self.revert', { merge }); await qc.invalidateQueries() } catch (e: any) { window.alert(e?.message ?? String(e)) } finally { setBusy(false) }
  }
  return (
    <button onClick={revert} disabled={busy} className="mt-1 rounded px-1.5 py-0.5 text-[10.5px] text-fault ring-1 ring-fault/30 hover:bg-fault/10 disabled:opacity-50">
      {busy ? <><ShipsWheel label="reverting" /> reverting</> : 'Revert'}
    </button>
  )
}
