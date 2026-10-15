// The owner card's proposals (theseus-slvl.6): what Theseus would add to the card from the owner's imported history,
// each a question to Keep, Edit or decline (Not this). A read of `card.list` while on screen; the answers are
// `card.answer`, which the core judges as the owner's act from a private place (this connection). The job is the CLI's
// (`theseus card propose`); the panel only shows and answers.
import { useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { Pin } from 'lucide-react'
import type { CardListResult, CardProposal } from '@protocol'
import { call, useRpc } from '@/lib/rpc'
import { Empty, Field, Panel, Pill } from '@/components/ui'
import { answeredLine, asksUntil, declineQuestion, restsOn, summary } from '@/lib/card'

export function CardProposals() {
  const [now] = useState(Date.now)
  const { data } = useRpc<CardListResult>('card.list', undefined, 10_000)
  return (
    <Panel help="context.card" title={<>card · {data?.open.length ?? 0} waiting</>} icon={<Pin size={13} />}>
      {!data && <Empty>reading the proposals…</Empty>}
      {data && !data.open.length && <Empty>No proposals wait. <span className="num">theseus card propose</span> reads your history for some.</Empty>}
      {data?.open.map((p) => <Proposal key={p.id} p={p} now={now} />)}
      {data && <div className="mt-2 text-[11px] text-ink-faint">{summary(data)}</div>}
      {data && [...data.kept, ...data.declined].slice(0, 8).map((p) => (
        <div key={p.id} className="mt-1 text-[11.5px] text-ink-dim"><span className="num text-ink-faint" title={p.id}>{p.id.slice(-8)}</span> {answeredLine(p)}</div>
      ))}
    </Panel>
  )
}

function Proposal({ p, now }: { p: CardProposal; now: number }) {
  const qc = useQueryClient()
  const [busy, setBusy] = useState(false)
  const answer = async (option: string, text?: string) => {
    setBusy(true)
    try { await call('card.answer', { id: p.id, option, text }); await qc.invalidateQueries() } catch (e: any) { window.alert(e?.message ?? String(e)) } finally { setBusy(false) }
  }
  const edit = () => {
    const text = window.prompt('Keep these words instead:', p.text)
    if (text && text.trim()) void answer('edit', text.trim())
  }
  const button = 'rounded px-1.5 py-0.5 text-[10.5px] ring-1 hover:bg-white/5 disabled:opacity-50'
  return (
    <div className="mb-2 border-b border-white/5 pb-2">
      <div className="flex items-center gap-2">
        <span className="num text-[11.5px] text-ink-dim" title={p.id}>{p.id.slice(-8)}</span>
        <Pill tone={p.scope.kind === 'everywhere' ? 'ok' : 'live'}>{p.kind}</Pill>
      </div>
      <div className="mt-1 whitespace-pre-wrap break-words text-[12.5px]">{p.question}</div>
      <Field label="rests on" mono>{restsOn(p)}</Field>
      <Field label="asks" mono>{asksUntil(p, now)}</Field>
      <div className="mt-1 flex gap-2">
        <button onClick={() => answer('keep')} disabled={busy} className={`${button} text-ok ring-ok/30`}>Keep</button>
        <button onClick={edit} disabled={busy} className={`${button} text-ink-dim ring-line`}>Edit</button>
        <button onClick={() => { if (window.confirm(declineQuestion(p))) void answer('decline') }} disabled={busy} className={`${button} text-fault ring-fault/30`}>Not this</button>
      </div>
    </div>
  )
}
