// People in the Ontology view (theseus-wy7y): the people, searchable, each with their handles and sessions; a person's
// page (their sessions, and a merge of another person into them); adding a person; and Jev's proposals for topics and
// people, with select-all accept. Every write is a protocol method, confirmed first, and judged by the core. A person's
// description and guidance hold role facts (what they do and own, how work flows), never an evaluation of them.
import { useMemo, useState } from 'react'
import { Link } from 'react-router'
import { Check, Plus, Users, Vote } from 'lucide-react'
import type {
  OntologyCategory, OntologyCategoryAddParams, OntologyListResult, OntologyPersonMergeParams, OntologyPersonMerged,
  OntologyProposalAcceptAllParams, OntologyProposalAcceptAllResult, OntologyProposalsResult,
} from '@protocol'
import { useRpc } from '@/lib/rpc'
import { bulkable, handlesLine, peopleOf, proposalKind, selectAll } from '@/lib/people'
import { cn } from '@/lib/format'
import { Empty, Panel } from '@/components/ui'
import { Act, Refused, useOntologyWrite } from '@/components/OntologyParts'

const field = 'rounded-md bg-white/5 px-2.5 py-1.5 text-[12px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40 disabled:opacity-50'

/** The people, by sessions then name, searchable by name, id and handle; a row selects the person. */
export function PeoplePanel({ categories, selected, select }: { categories: OntologyCategory[]; selected: string | null; select: (id: string) => void }) {
  const [q, setQ] = useState('')
  const people = useMemo(() => peopleOf(categories, q), [categories, q])
  const all = useMemo(() => categories.filter((c) => c.kind === 'person').length, [categories])
  return (
    <Panel title={<>people · {all}</>} icon={<Users size={13} />}>
      <div className="flex flex-col gap-2 p-2">
        <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="search people by name or handle" aria-label="search people" className={field} />
        {people.length === 0 ? <Empty>{all ? 'no person answers that search' : 'no people yet: a DM’s person comes when it binds, `theseus import people` brings an import’s, and one is added here'}</Empty> : (
          <div className="max-h-80 overflow-y-auto"><table className="w-full text-[11.5px]">
            <tbody>
              {people.map((c) => (
                <tr key={c.id} onClick={() => select(c.id)} aria-selected={c.id === selected}
                  className={cn('cursor-pointer border-t border-line/50 hover:bg-white/[0.03]', c.id === selected && 'bg-live/10')}>
                  <td className="py-1"><span className="text-ink">{c.name}</span><div className="num text-[10px] text-ink-faint">{handlesLine(c) || c.id}</div></td>
                  <td className="num px-1 text-right text-ink-dim">{(c.members ?? 0) > 0 ? (c.members ?? 0).toLocaleString() : <span className="text-ink-faint">—</span>}</td>
                </tr>
              ))}
            </tbody>
          </table></div>
        )}
      </div>
    </Panel>
  )
}

/** A person's page: their handles, the sessions that hold them (one read of every stored membership, while it is open),
 *  and a merge of another person into them. */
export function PersonPage({ person, people, disabled }: { person: OntologyCategory; people: OntologyCategory[]; disabled: boolean }) {
  const { data } = useRpc<OntologyListResult>('ontology.list', {}, 30_000)
  const { write, busy, refused } = useOntologyWrite()
  const [other, setOther] = useState('')
  const sessions = useMemo(() => (data?.memberships ?? []).filter((m) => m.category === person.id), [data, person.id])
  const merge = async () => {
    if (!other) return
    const p: OntologyPersonMergeParams = { absorbed: other, survivor: person.id, undo: false }
    const out = await write<OntologyPersonMerged>(
      `Merge ${other} into ${person.name} (${person.id})?\n\n${person.id} keeps its id and takes the other’s handles, memberships and guidance. It can be undone (\`theseus ontology person merge ${other} --undo\`).`,
      'ontology.person.merge', p,
    )
    if (out) setOther('')
  }
  return (
    <Panel title={<>person · {person.name}</>} icon={<Users size={13} />}>
      <div className="flex flex-col gap-2 p-3 text-[12px]">
        <div className="num text-[11px] text-ink-faint">{person.id} · by {person.added_by}</div>
        {person.description && <div className="text-ink-dim">{person.description}</div>}
        <div className="flex flex-wrap gap-1">{(person.handles ?? []).map((h) => <span key={h} className="num rounded bg-white/5 px-1.5 py-0.5 text-[11px] text-ink-dim ring-1 ring-line">{h}</span>)}</div>
        <div className="panel-title mt-1">sessions · {data ? sessions.length : '…'}</div>
        <div className="max-h-48 overflow-y-auto">
          {sessions.map((m) => <Link key={m.session_id} to={`/session/${m.session_id}`} className="num block truncate text-[11px] text-ink-dim hover:text-live">{m.session_id} <span className="text-ink-faint">· {m.origin}</span></Link>)}
          {data && sessions.length === 0 && <div className="text-ink-faint">no session holds this person (a DM’s own session is given from its place)</div>}
        </div>
        <label className="mt-1 flex items-center gap-2 text-[12px] text-ink-faint">
          merge into this one
          <select value={other} onChange={(e) => setOther(e.target.value)} disabled={disabled || busy} aria-label="person to merge into this one" className={cn(field, 'min-w-0 flex-1')}>
            <option value="" className="bg-deck">pick a person…</option>
            {people.filter((c) => c.id !== person.id).map((c) => <option key={c.id} value={c.id} className="bg-deck">{c.name} · {c.id}</option>)}
          </select>
        </label>
        <Refused words={refused} />
        <div><Act onClick={() => void merge()} busy={busy} off={disabled || !other} title="ontology.person.merge: confirmed first">Merge</Act></div>
      </div>
    </Panel>
  )
}

/** `ontology.category.add` of a person, with their handles. */
export function AddPerson({ disabled }: { disabled: boolean }) {
  const { write, busy, refused } = useOntologyWrite()
  const [name, setName] = useState('')
  const [handles, setHandles] = useState('')
  const [description, setDescription] = useState('')
  const add = async () => {
    const hs = handles.split(/[\s,]+/).map((h) => h.trim()).filter(Boolean)
    const p: OntologyCategoryAddParams = { kind: 'person', name: name.trim(), handles: hs, ...(description.trim() ? { description: description.trim() } : {}) }
    const out = await write<OntologyCategory>(
      `Add the person “${p.name}”${hs.length ? ` with ${hs.join(', ')}` : ''}?\n\nA handle another person holds makes this that person.`,
      'ontology.category.add', p,
    )
    if (out) { setName(''); setHandles(''); setDescription('') }
  }
  return (
    <Panel title="add a person" icon={<Plus size={13} />}>
      <form className="flex flex-col gap-2 p-3" onSubmit={(e) => { e.preventDefault(); if (name.trim()) void add() }}>
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="name" aria-label="person name" disabled={disabled} className={field} />
        <input value={handles} onChange={(e) => setHandles(e.target.value)} placeholder="handles: slack:U…, discord:…, email:…" aria-label="person handles" disabled={disabled} className={field} />
        <input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="their role: what they do and own (never an evaluation)" aria-label="person description" disabled={disabled} className={field} />
        <Refused words={refused} />
        <div><Act onClick={() => { if (name.trim()) void add() }} busy={busy} off={disabled} title="ontology.category.add: confirmed first"><Plus size={13} /> Add person</Act></div>
      </form>
    </Panel>
  )
}

/** Jev's unanswered proposals, topics and people side by side: pick some, or select all of a kind, and accept them
 *  (`ontology.proposal.accept_all`). A proposal of a new category needs a name: `theseus ontology accept` takes it. */
export function ProposalsPanel({ disabled }: { disabled: boolean }) {
  const { data } = useRpc<OntologyProposalsResult>('ontology.proposals', { limit: 500 }, 15_000)
  const { write, busy, refused } = useOntologyWrite()
  const [kind, setKind] = useState<string | null>(null)
  const [picked, setPicked] = useState<Set<string>>(new Set())
  const ps = useMemo(() => (data?.proposals ?? []).filter((p) => kind === null || proposalKind(p) === kind), [data, kind])
  const toggle = (j: string) => setPicked((s) => { const n = new Set(s); if (n.has(j)) n.delete(j); else n.add(j); return n })
  const accept = async () => {
    const p: OntologyProposalAcceptAllParams = { min_confidence: 0, judgments: [...picked] }
    const out = await write<OntologyProposalAcceptAllResult>(`Accept ${picked.size} proposal${picked.size === 1 ? '' : 's'}?\n\nEach session joins what Jev proposed (yours, operator), at its next recompile.`, 'ontology.proposal.accept_all', p)
    if (out) setPicked(new Set())
  }
  return (
    <Panel title={<>proposals · {data?.proposals.length ?? '…'}{data?.more ? ` (+${data.more})` : ''}</>} icon={<Vote size={13} />}>
      <div className="flex flex-col gap-2 p-2 text-[11.5px]">
        <div className="flex items-center gap-2">
          {([null, 'topic', 'person'] as const).map((k) => (
            <button key={k ?? 'all'} type="button" onClick={() => setKind(k)} className={cn('rounded px-1.5 py-0.5', kind === k ? 'bg-live/15 text-live' : 'text-ink-faint hover:text-ink')}>{k ?? 'all'}</button>
          ))}
          <button type="button" className="ml-auto text-ink-faint hover:text-live" onClick={() => setPicked(new Set(selectAll(ps, kind)))}>select all</button>
          <Act onClick={() => void accept()} busy={busy} off={disabled || picked.size === 0} title="ontology.proposal.accept_all: confirmed first"><Check size={12} /> Accept {picked.size || ''}</Act>
        </div>
        <Refused words={refused} />
        {ps.length === 0 ? <Empty>no unanswered proposals{kind ? ` of ${kind}s` : ''}</Empty> : (
          <div className="max-h-72 overflow-y-auto"><table className="w-full">
            <tbody>
              {ps.map((p) => (
                <tr key={p.judgment} className="border-t border-line/50">
                  <td className="py-1 pr-1"><input type="checkbox" aria-label={`pick ${p.judgment}`} disabled={!bulkable(p)} checked={picked.has(p.judgment)} onChange={() => toggle(p.judgment)} /></td>
                  <td className="text-ink">{p.new_topic ? <span className="text-wait">a new topic</span> : p.topic_name ?? p.topic}<div className="num text-[10px] text-ink-faint">{proposalKind(p)} · {p.session_title ?? p.session_id}</div></td>
                  <td className="num px-1 text-right text-ink-dim">{p.confidence.toFixed(2)}</td>
                  <td className="px-1 text-ink-faint">{p.band}</td>
                </tr>
              ))}
            </tbody>
          </table></div>
        )}
      </div>
    </Panel>
  )
}
