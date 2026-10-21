// The add-a-kind form (theseus-s430x, R7c): `ontology.kind.add` through the confirmed write the Ontology view's other acts
// use. The fields are checked here as the daemon checks them (`lib/ontokinds.ts`), and the confirm carries the exact params
// it sends; the daemon still judges the call, and its refusal is shown as it said it.
import { useMemo, useState } from 'react'
import { Plus, Shapes } from 'lucide-react'
import type { OntologyKindResult } from '@protocol'
import { buildKindAdd, KIND_ORIGINS, KIND_RULES, kindAddAsk, NO_KIND_FORM, type KindForm } from '@/lib/ontokinds'
import { cn } from '@/lib/format'
import { Panel } from '@/components/ui'
import { Act, Refused, useOntologyWrite } from '@/components/OntologyParts'

const field = 'rounded-md bg-white/5 px-2.5 py-1.5 text-[12px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40 disabled:opacity-50'

/** `ontology.kind.add`: a new interpreted kind, its name, who may assign it, how many a session holds, where it nests, its
 *  precedence and rule, and up to five examples. `kinds` is the table as listed, for the parent and to refuse a name taken. */
export function AddKind({ kinds, disabled }: { kinds: readonly { name: string }[]; disabled: boolean }) {
  const { write, busy, refused } = useOntologyWrite()
  const [f, setF] = useState<KindForm>(NO_KIND_FORM)
  // The faults show once the reader has tried to add, or at once for a field they have typed into.
  const [tried, setTried] = useState(false)
  const set = <K extends keyof KindForm>(k: K, v: KindForm[K]) => setF((x) => ({ ...x, [k]: v }))
  const taken = useMemo(() => kinds.map((k) => k.name), [kinds])
  const built = useMemo(() => buildKindAdd(f, taken), [f, taken])
  const shows = (k: keyof KindForm): string | undefined => {
    const empty = k === 'assignedBy' ? false : f[k] === ''
    return tried || !empty ? built.errors[k] : undefined
  }
  const add = async () => {
    setTried(true)
    if (!built.params) return
    const out = await write<OntologyKindResult>(kindAddAsk(built.params), 'ontology.kind.add', built.params)
    if (out) { setF(NO_KIND_FORM); setTried(false) }
  }
  const pickOrigin = (o: string) => set('assignedBy', f.assignedBy.includes(o) ? f.assignedBy.filter((x) => x !== o) : KIND_ORIGINS.filter((x) => x === o || f.assignedBy.includes(x)))
  const err = (k: keyof KindForm) => (shows(k) ? <div role="alert" className="text-[11px] text-fault">{shows(k)}</div> : null)
  return (
    <Panel help="ontology.kindadd" title="add a kind" icon={<Shapes size={13} />}>
      <form className="flex flex-col gap-2 p-3" onSubmit={(e) => { e.preventDefault(); void add() }}>
        <input value={f.name} onChange={(e) => set('name', e.target.value)} placeholder="name (lowercase, e.g. expertise)" aria-label="kind name" disabled={disabled} className={cn(field, shows('name') && 'ring-fault/50')} />
        {err("name")}
        <textarea value={f.description} onChange={(e) => set('description', e.target.value)} rows={2} placeholder="what the kind is for (optional)" aria-label="kind description" disabled={disabled} className={field} />
        {err("description")}
        <div className="flex flex-wrap items-center gap-1.5 text-[11.5px] text-ink-faint" role="group" aria-label="assigned by">
          <span>assigned by</span>
          {KIND_ORIGINS.map((o) => (
            <button key={o} type="button" aria-pressed={f.assignedBy.includes(o)} disabled={disabled} onClick={() => pickOrigin(o)}
              className={cn('num rounded px-1.5 py-0.5 ring-1 ring-inset disabled:opacity-50', f.assignedBy.includes(o) ? 'bg-live/10 text-live ring-live/40' : 'ring-line hover:text-ink')}>{o}</button>
          ))}
        </div>
        {err("assignedBy")}
        <div className="grid grid-cols-2 gap-2">
          <input value={f.perSession} onChange={(e) => set('perSession', e.target.value)} placeholder="per session (3, or many)" aria-label="per session" disabled={disabled} className={field} />
          <input value={f.precedence} onChange={(e) => set('precedence', e.target.value)} placeholder="precedence (after the last)" aria-label="precedence" inputMode="numeric" disabled={disabled} className={field} />
        </div>
        {err("perSession")}
        {err("precedence")}
        <div className="grid grid-cols-2 gap-2">
          <select value={f.parent} onChange={(e) => set('parent', e.target.value)} aria-label="parent kind" disabled={disabled} className={field}>
            <option value="" className="bg-deck">parent: none</option>
            {kinds.map((k) => <option key={k.name} value={k.name} className="bg-deck">{k.name}</option>)}
          </select>
          <select value={f.rule} onChange={(e) => set('rule', e.target.value)} aria-label="rule" disabled={disabled} className={field}>
            <option value="" className="bg-deck">rule: chain</option>
            {KIND_RULES.map((r) => <option key={r} value={r} className="bg-deck">{r}</option>)}
          </select>
        </div>
        {err("parent")}
        {err("rule")}
        <textarea value={f.examples} onChange={(e) => set('examples', e.target.value)} rows={3} placeholder="examples, one a line (up to 5)" aria-label="kind examples" disabled={disabled} className={field} />
        {err("examples")}
        <Refused words={refused} />
        <div className="flex items-center gap-2">
          <Act onClick={() => void add()} busy={busy} off={disabled} title="ontology.kind.add: confirmed first, with the exact params it sends"><Plus size={13} /> Add kind</Act>
          {disabled && <span className="text-[11px] text-wait">the present only</span>}
        </div>
      </form>
    </Panel>
  )
}
