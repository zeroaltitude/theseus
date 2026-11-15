// The ontology's controls: retire a kind (`ontology.kind.retire`, which the daemon allows only for a kind nothing holds, and
// refuses otherwise in words), and answer the proposals one at a time (`ontology.proposal.accept` and `.reject`; the Ontology
// tab answers them in bulk and the people's own). Retiring asks first; accepting takes a second press.
import { useState } from 'react'
import { Vote } from 'lucide-react'
import type { OntologyListResult, OntologyProposal, OntologyProposalsResult } from '@protocol'
import { Empty, Panel } from '@/components/ui'
import { useRpc } from '@/lib/rpc'
import { ActButton, FIELD } from '../ActButton'
import { acceptWords, needsName, proposalName, retireKindWords } from './logic'

/** How many proposals this panel lists: the rest are the Ontology tab's. */
const SHOWN = 8

function Proposal({ p }: { p: OntologyProposal }) {
  const [typed, setTyped] = useState('')
  const name = needsName(p) ? typed.trim() : proposalName(p)
  return (
    <li className="flex flex-wrap items-center gap-x-3 gap-y-1 py-2">
      <span className="min-w-[260px] flex-1 text-[13px] text-ink">{needsName(p) ? 'A new topic' : <>“{proposalName(p)}”</>} <span className="text-[12px] text-ink-faint">for {p.session_title ?? p.session_id}, {Math.round(p.confidence * 100)}% sure ({p.band})</span></span>
      {needsName(p) && <input value={typed} onChange={(e) => setTyped(e.target.value)} placeholder="name this topic" aria-label="name this topic" className={`${FIELD} w-[190px]`} />}
      <ActButton verb="proposal_accept" params={{ judgment: p.judgment, ...(needsName(p) ? { topic: name } : {}) }} label="Accept" effect={acceptWords(p, name)} disabled={!name} />
      <ActButton verb="proposal_reject" params={{ judgment: p.judgment }} label="Reject" effect="Reject this proposal; Theseus may propose it again." tone="plain" />
    </li>
  )
}

export function Kinds() {
  const { data: onto } = useRpc<OntologyListResult>('ontology.list', {}, 15_000)
  const { data: props } = useRpc<OntologyProposalsResult>('ontology.proposals', { limit: 25 }, 15_000)
  // Theseus's own kinds (those it was built with) are never taken away; only a kind someone added is offered.
  const kinds = (onto?.kinds ?? []).filter((k) => k.added_by !== 'seed' && k.basis !== 'given')
  return (
    <Panel help="simple.config.kinds" title="Kinds, and proposals one at a time" icon={<Vote size={13} />} bodyClassName="flex flex-col gap-4 p-4">
      <section className="flex flex-col gap-2">
        <h3 className="simple-eyebrow">Retire a kind</h3>
        {!onto ? <Empty>reading…</Empty> : !kinds.length ? <p className="text-[13px] text-ink-faint">No kind has been added, and Theseus’s own kinds are never taken away, so there is nothing to retire.</p> : (
          <ul className="grid grid-cols-1 gap-x-8 md:grid-cols-2 xl:grid-cols-3">
            {kinds.map((k) => (
              <li key={k.name} className="flex items-center gap-3 border-b border-line py-1.5">
                <span className="min-w-0 flex-1 truncate text-[13px] text-ink" title={k.description}>{k.name}</span>
                <ActButton verb="kind_retire" params={{ name: k.name }} label="Retire" effect={retireKindWords(k.name)} tone="fault" submit="Yes, retire it" />
              </li>
            ))}
          </ul>
        )}
      </section>
      <section className="flex flex-col gap-2 border-t border-line pt-3">
        <h3 className="simple-eyebrow">Proposed topics · {(props?.proposals.length ?? 0) + (props?.more ?? 0)}</h3>
        {!props?.proposals.length ? <p className="text-[13px] text-ink-faint">Nothing is waiting for an answer.</p> : (
          <>
            <ul className="divide-y divide-line">{props.proposals.slice(0, SHOWN).map((p) => <Proposal key={p.judgment} p={p} />)}</ul>
            {props.proposals.length + props.more > SHOWN && <p className="text-[12px] text-ink-faint">The first {SHOWN} are here; the Ontology tab lists them all and answers them in bulk.</p>}
          </>
        )}
      </section>
    </Panel>
  )
}
