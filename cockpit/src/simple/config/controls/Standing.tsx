// Standing rules, the control: add one, keep or dismiss what Theseus proposed from a persona's notes, and answer what it asked
// about a rule it was unsure of. (Revoking a rule is on the Standing rules tab.) A rule changes how Theseus behaves from the
// next turn, so adding and keeping ask first.
import { useState } from 'react'
import { Pin } from 'lucide-react'
import type { StandingKind, StandingListResult, StandingProposalsResult } from '@protocol'
import { Panel } from '@/components/ui'
import { when } from '@/lib/explorer'
import { scopeWords } from '@/lib/standing'
import { useRpc } from '@/lib/rpc'
import { ActButton, FIELD } from '../ActButton'
import { ANSWERS, KINDS, keepWords, standingWords } from './logic'

export function Standing() {
  const { data: proposals } = useRpc<StandingProposalsResult>('standing.proposals', {}, 15_000)
  const { data: list } = useRpc<StandingListResult>('standing.list', {}, 10_000)
  const [kind, setKind] = useState<StandingKind>('order')
  const [text, setText] = useState('')
  const open = (proposals?.proposals ?? []).filter((p) => p.state === 'open')
  return (
    <Panel help="simple.config.standing" title="Standing rules: add, and answer what Theseus noticed" icon={<Pin size={13} />} bodyClassName="flex flex-col gap-4 p-4">
      <section className="flex flex-col gap-2">
        <h3 className="simple-eyebrow">Add a standing rule</h3>
        <div className="flex flex-wrap gap-2">
          <select value={kind} onChange={(e) => setKind(e.target.value as StandingKind)} aria-label="kind of rule" className={FIELD}>
            {KINDS.map(([k, words]) => <option key={k} value={k}>{words}</option>)}
          </select>
          <input value={text} onChange={(e) => setText(e.target.value)} placeholder="the rule, in your words" aria-label="the rule" className={`${FIELD} min-w-[320px] flex-1`} />
          <ActButton verb="standing_add" params={{ kind, text: text.trim(), scope: { kind: 'everywhere' } }} label="Add it everywhere" effect={standingWords(kind, text, true)} disabled={!text.trim()} submit="Yes, add it"
            onDone={() => setText('')} words={(r) => `added as ${r.record?.id ?? 'a new rule'}`} />
        </div>
      </section>
      <section className="flex flex-col gap-2 border-t border-line pt-3">
        <h3 className="simple-eyebrow">Proposed from a persona’s notes · {open.length}</h3>
        {!open.length && <p className="text-[13px] text-ink-faint">Nothing is waiting to be kept.</p>}
        {open.map((p) => (
          <div key={p.id} className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <span className="min-w-[280px] flex-1 text-[13px] text-ink">{p.text}<span className="ml-2 text-[12px] text-ink-faint">{p.kind}, for {p.persona}, from {p.from}</span></span>
            <ActButton verb="standing_keep" params={{ id: p.id }} label="Keep" effect={keepWords(p.text)} submit="Yes, keep it" words={(r) => `kept as ${r.record?.id ?? 'a standing rule'}`} />
            <ActButton verb="standing_dismiss" params={{ id: p.id }} label="Dismiss" effect="Dismiss this proposal; Theseus will not propose it again." tone="plain" />
          </div>
        ))}
      </section>
      <section className="flex flex-col gap-2 border-t border-line pt-3">
        <h3 className="simple-eyebrow">Theseus asks · {list?.asks.length ?? 0}</h3>
        {!list?.asks.length && <p className="text-[13px] text-ink-faint">Theseus has no question about a rule.</p>}
        {list?.asks.map((a) => (
          <div key={a.id} className="flex flex-wrap items-center gap-x-3 gap-y-1">
            <span className="min-w-[280px] flex-1 text-[13px] text-ink">“{a.text}”<span className="ml-2 text-[12px] text-ink-faint">{a.kind}, would hold {scopeWords(a.scope)}, {when(a.at_ms)}; asked because {a.why}</span></span>
            {ANSWERS.map(([answer, words]) => <ActButton key={answer} verb="standing_answer" params={{ id: a.id, answer }} label={words} effect={`${words}: Theseus ${answer === 'not' ? 'drops this and will not keep it' : 'keeps this as a standing rule'}.`} tone={answer === 'not' ? 'plain' : 'live'} />)}
          </div>
        ))}
      </section>
    </Panel>
  )
}
