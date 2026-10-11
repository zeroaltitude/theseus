// What Theseus changed about itself (theseus-pw1q.4): `self.log`'s rows, newest first (the self steps' rows and
// today's self-changes: pack moves, learned packs, extensions, routing corrections, syntheses), each with what, why,
// its numbers and its undo, and the kill switch's state with a Halt button (`self.halt`, anyone's, confirmed first).
// The resume is the owner's alone: `theseus self resume` (or "resume self" from the owner's Discord account), which the card names and never sends.
import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Link } from 'react-router'
import { OctagonX } from 'lucide-react'
import type { SelfLogResult } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { ago } from '@/lib/format'
import { Empty, Field, Panel, Pill } from '@/components/ui'
import { kindWords, numbersLine, stateWords, switchLine } from '@/lib/selfchanges'

export function SelfChangesCard({ now }: { now: number }) {
  const { data } = useRpc<SelfLogResult>('self.log', { limit: 12 }, 5000)
  const s = data?.state
  const w = s ? stateWords(s) : undefined
  return (
    <Panel title="What Theseus changed about itself" icon={<OctagonX size={13} />} bodyClassName="px-3.5 py-2.5">
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
      {busy ? '…' : 'Halt'}
    </button>
  )
}
