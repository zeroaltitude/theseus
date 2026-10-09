// A session's memberships (step 21c, theseus-8kk.2): the ones it holds now (given from its place, and interpreted), each
// with its origin and as-of, beside what its newest compilation's manifest recorded. A change not yet compiled reads
// "applies at the next recompile". Adding or removing a topic or a person (theseus-wy7y: grouped and searchable, a person
// by name or handle) is `ontology.membership.set`, confirmed first.
import { useMemo, useState } from 'react'
import { Link } from 'react-router'
import { Shapes, X } from 'lucide-react'
import type { CompilationInfo, OntologyMembershipResult, OntologyMembershipSetParams } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { guidanceWords, nothingPending, pendingOf, recordedOf, type Pending } from '@/lib/ontology'
import { cn, stamp } from '@/lib/format'
import { handlesLine, pulldownGroups } from '@/lib/people'
import { Pill } from '@/components/ui'
import { Act, Refused, useInPast, useOntology, useOntologyWrite } from '@/components/OntologyParts'

const NONE: Pending = { added: [], removed: [], guidance: [] }

/** What waits for this session's next recompile, and the compilation it is measured against. */
export function useMembershipState(sessionId: string) {
  const { data: list } = useOntology(sessionId)
  const { data: comps } = useRpc<{ compilations: CompilationInfo[] }>('compilation.list', { session_id: sessionId, n: 50 }, 10_000)
  const newest = useMemo(() => comps?.compilations.find((c) => c.current) ?? comps?.compilations[0] ?? null, [comps])
  const pending = useMemo(
    () => (list && newest ? pendingOf(list.memberships, list.categories, list.kinds, newest.manifest) : NONE),
    [list, newest],
  )
  return { list, newest, pending }
}

/** The note beside Recompile: what applies at the next recompile, or nothing. */
export function PendingNote({ sessionId }: { sessionId: string }) {
  const { pending } = useMembershipState(sessionId)
  if (nothingPending(pending)) return null
  return (
    <Pill tone="wait" title={pendingLines(pending).join('\n')}>
      ontology changes apply at the next recompile
    </Pill>
  )
}

function pendingLines(p: Pending): string[] {
  return [...p.added.map((c) => `+ ${c}`), ...p.removed.map((c) => `− ${c}`), ...p.guidance.map(guidanceWords)]
}

export function MembershipsPanel({ sessionId }: { sessionId: string }) {
  const { list, newest, pending } = useMembershipState(sessionId)
  const past = useInPast()
  const { write, busy, refused } = useOntologyWrite()
  const [topic, setTopic] = useState('')
  const [q, setQ] = useState('')
  const recorded = useMemo(() => recordedOf(newest?.manifest), [newest])
  if (!list) return null
  const mine = list.memberships
  const groups = pulldownGroups(list.categories, new Set(mine.map((m) => m.category)), q)
  const set = (p: Partial<OntologyMembershipSetParams>, ask: string) =>
    write<OntologyMembershipResult>(ask, 'ontology.membership.set', { session_id: sessionId, add: [], remove: [], ...p })
  const add = async (id: string) => {
    if (!id) return
    if (await set({ add: [id] }, `Add this session to ${id}?\n\nIt applies at the session’s next recompile.`)) setTopic('')
  }
  const remove = (id: string) => set({ remove: [id] }, `Take this session out of ${id}?\n\nIt applies at the session’s next recompile.`)
  return (
    <div>
      <div className="panel-title mb-1 flex items-center gap-1.5"><Shapes size={12} /> memberships · what the next compile would use
        <Link to="/ontology" className="num ml-auto text-[10.5px] normal-case tracking-normal text-ink-faint hover:text-live">the ontology →</Link>
      </div>
      {past && <div className="num mb-1 text-[11px] text-wait">the present, not the ship’s log’s moment; adding and removing are off until LIVE</div>}
      <table className="w-full text-[11.5px]">
        <thead className="text-[10px] uppercase tracking-wider text-ink-faint"><tr><th className="py-1 text-left">category</th><th className="px-1 text-left">origin</th><th className="px-1 text-left">as of</th><th className="px-1 text-left">compiled</th><th /></tr></thead>
        <tbody>
          {mine.map((m) => {
            const compiled = recorded.memberships.some((r) => r.category === m.category && r.origin === m.origin)
            return (
              <tr key={`${m.category}:${m.origin}`} className="border-t border-line/50">
                <td className="num py-1 text-ink">{m.category}</td>
                <td className="px-1 text-ink-faint">{m.origin}{m.confidence != null ? ` · ${m.confidence.toFixed(2)}` : ''}</td>
                <td className="num px-1 text-ink-faint">{stamp(m.as_of_ms)}</td>
                <td className={cn('px-1', compiled ? 'text-ink-faint' : 'text-wait')}>{compiled ? 'yes' : 'at the next recompile'}</td>
                <td className="text-right">
                  {(m.origin === 'operator' || m.origin === 'import') && (
                    <button type="button" disabled={busy || past} onClick={() => void remove(m.category)} title={past ? 'return to LIVE to change it' : `take the session out of ${m.category}`}
                      className="rounded p-0.5 text-ink-faint hover:text-fault disabled:cursor-not-allowed disabled:opacity-40"><X size={12} /></button>
                  )}
                </td>
              </tr>
            )
          })}
          {pending.removed.map((c) => (
            <tr key={`gone:${c}`} className="border-t border-line/50 text-ink-faint"><td className="num py-1 line-through">{c}</td><td className="px-1" colSpan={2}>recorded by the compilation, no longer held</td><td className="px-1 text-wait" colSpan={2}>drops at the next recompile</td></tr>
          ))}
          {!mine.length && !pending.removed.length && <tr><td colSpan={5} className="py-2 text-ink-faint">this session belongs to no category</td></tr>}
        </tbody>
      </table>
      <div className="mt-2 flex items-center gap-2">
        <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="search" aria-label="search topics and people" disabled={busy || past}
          className="w-28 rounded-md bg-white/5 px-2 py-1 text-[12px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40 disabled:opacity-50" />
        <select value={topic} onChange={(e) => setTopic(e.target.value)} disabled={busy || past} aria-label="topic or person to add"
          className="min-w-0 flex-1 rounded-md bg-white/5 px-2 py-1 text-[12px] text-ink outline-none ring-1 ring-line focus:ring-live/40 disabled:opacity-50">
          <option value="" className="bg-deck">add a topic or a person…</option>
          {groups.map((g) => (
            <optgroup key={g.kind} label={g.label} className="bg-deck">
              {g.items.map((c) => <option key={c.id} value={c.id} className="bg-deck">{c.kind === 'person' ? `${c.name} · ${handlesLine(c) || c.id}` : c.id}</option>)}
            </optgroup>
          ))}
        </select>
        <Act onClick={() => void add(topic)} busy={busy} off={past || !topic} title="ontology.membership.set: confirmed first">Add</Act>
      </div>
      <div className="mt-1.5"><Refused words={refused} /></div>
      <div className="num mt-2 text-[11px] text-ink-faint" title={newest ? `compilation ${newest.compilation_id}` : undefined}>
        {!newest ? 'no compilation yet' : recorded.known ? `the newest compilation recorded ${recorded.memberships.length} membership${recorded.memberships.length === 1 ? '' : 's'} and ${recorded.guidance.length} guidance block${recorded.guidance.length === 1 ? '' : 's'}${recorded.guidance.length ? `: ${recorded.guidance.map((g) => `${g.category} v${g.version}`).join(', ')}` : ''}` : 'the newest compilation recorded no memberships or guidance'}
      </div>
      {!nothingPending(pending) && (
        <div className="mt-1 rounded-md bg-wait/10 px-2.5 py-1.5 text-[11.5px] text-wait ring-1 ring-wait/30">
          applies at the next recompile (the Recompile control, or the next turn’s own):
          {pendingLines(pending).map((l) => <div key={l} className="num">{l}</div>)}
        </div>
      )}
    </div>
  )
}
