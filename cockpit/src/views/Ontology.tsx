// The ontology (step 21c, theseus-8kk.2; M4 design §2.8): the kinds table, the category tree with each category's guidance, a
// guidance editor, and "add a topic". A view only: every write is a protocol method, confirmed first with the text it
// sends, and judged by the core. It reads the present (`ontology.list`); the time machine keeps no past of it, so while
// it shows a moment the view says so and its controls are off. The selected category is in the address (`?category=`).
// At an import's size (theseus-anh3: well over a hundred topics, tens of thousands of memberships) it reads the tree
// alone, each category with its count of sessions, and past `FOLD_AT` categories the tree starts folded at its roots.
import { useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'
import { ChevronDown, ChevronRight, Compass, ListTree, Pencil, Plus, Shapes } from 'lucide-react'
import type { OntologyCategory, OntologyCategoryAddParams, OntologyGuidance, OntologyGuidanceSetParams } from '@protocol'
import { ancestorsOf, FOLD_AT, parents, shown, treeOrder, withinCounts } from '@/lib/ontology'
import { cn, clock } from '@/lib/format'
import { useAsOf } from '@/lib/timemachine'
import { Empty, Panel, Pill } from '@/components/ui'
import { Act, Refused, useInPast, useOntology, useOntologyWrite } from '@/components/OntologyParts'

const field = 'rounded-md bg-white/5 px-2.5 py-1.5 text-[12px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40 disabled:opacity-50'

export default function Ontology() {
  const { data, isLoading, error } = useOntology(undefined, true)
  const past = useInPast()
  const asOf = useAsOf((s) => s.t)
  const [params, setParams] = useSearchParams()
  const selected = params.get('category')
  const rows = useMemo(() => treeOrder(data?.categories ?? []), [data])
  const within = useMemo(() => withinCounts(rows), [rows])
  const branches = useMemo(() => parents(rows), [rows])
  // The open rows: null until the reader folds or opens one, and then by default every branch open, or none past FOLD_AT.
  const [opened, setOpened] = useState<Set<string> | null>(null)
  const open = useMemo(() => {
    const s = new Set(opened ?? (rows.length > FOLD_AT ? [] : branches))
    for (const a of ancestorsOf(rows, selected)) s.add(a)
    return s
  }, [opened, rows, branches, selected])
  const visible = useMemo(() => shown(rows, open), [rows, open])
  const toggle = (id: string) => setOpened(() => { const s = new Set(open); if (s.has(id)) s.delete(id); else s.add(id); return s })
  const picked = rows.find((r) => r.category.id === selected)?.category ?? null
  const select = (id: string | null) => setParams((p) => { if (id) p.set('category', id); else p.delete('category'); return p }, { replace: true })

  if (!data) return <Panel title="ontology" bodyClassName="h-64"><Empty>{error ? `the daemon refused the read: ${(error as Error).message}` : isLoading ? 'reading the ontology…' : 'no ontology'}</Empty></Panel>
  return (
    <div className="flex flex-col gap-3">
      {past && <div className="num px-1 text-[11.5px] text-wait">the ship’s log is at {clock(asOf ?? 0)}; the ontology keeps no past, so this is the present, and its controls are off until you return to LIVE</div>}
      <Panel title={<>kinds · {data.kinds.length}</>} icon={<Shapes size={13} />}>
        <div className="overflow-x-auto p-2"><table className="w-full whitespace-nowrap text-[11.5px]">
          <thead className="text-[10px] uppercase tracking-wider text-ink-faint">
            <tr><th className="py-1 text-left">kind</th><th className="px-1 text-left">basis</th><th className="px-1 text-left">assigned by</th><th className="px-1 text-left">per session</th><th className="px-1 text-left">parent</th><th className="px-1 text-right">precedence</th><th className="px-1 text-left">rule</th><th className="px-1 text-right">v</th><th className="px-1 text-left">added by</th><th className="pl-1 text-left">what it is</th></tr>
          </thead>
          <tbody>
            {data.kinds.map((k) => (
              <tr key={k.name} className="border-t border-line/50">
                <td className="num py-1 text-ink">{k.name}</td>
                <td className="px-1"><Pill tone={k.basis === 'given' ? 'idle' : 'think'}>{k.basis}</Pill></td>
                <td className="num px-1 text-ink-dim">{k.assigned_by.join(', ') || '—'}</td>
                <td className="num px-1 text-ink-dim">{k.per_session}</td>
                <td className="num px-1 text-ink-faint">{k.parent ?? '—'}</td>
                <td className="num px-1 text-right text-ink-dim">{k.precedence}</td>
                <td className="num px-1 text-ink-dim">{k.rule}</td>
                <td className="num px-1 text-right text-ink-faint">{k.version}</td>
                <td className="px-1 text-ink-faint">{k.added_by}</td>
                <td className="max-w-[40ch] truncate pl-1 text-ink-faint" title={k.description}>{k.description}</td>
              </tr>
            ))}
          </tbody>
        </table></div>
      </Panel>
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        <Panel title={<>categories · {rows.length}{branches.size > 0 && (
          <span className="ml-2 normal-case tracking-normal">
            <button type="button" className="text-ink-faint hover:text-live" onClick={() => setOpened(new Set(branches))}>open all</button>
            <span className="text-ink-faint"> · </span>
            <button type="button" className="text-ink-faint hover:text-live" onClick={() => setOpened(new Set())}>fold all</button>
          </span>
        )}</>} icon={<ListTree size={13} />}>
          {rows.length === 0 ? <Empty>no categories yet: a place’s are made when it binds, and a topic is added here</Empty> : (
            <div className="overflow-x-auto p-2"><table className="w-full text-[11.5px]">
              <thead className="text-[10px] uppercase tracking-wider text-ink-faint">
                <tr><th className="py-1 text-left">category</th><th className="px-1 text-left">kind</th><th className="px-1 text-right" title="sessions whose memberships hold it; folded, with those below it">sessions</th><th className="px-1 text-left">added by</th><th className="px-1 text-left">description</th><th className="pl-1 text-left">guidance</th></tr>
              </thead>
              <tbody>
                {visible.map(({ category: c, depth }) => {
                  const branch = branches.has(c.id)
                  const folded = branch && !open.has(c.id)
                  return (
                  <tr key={c.id} onClick={() => select(c.id)} aria-selected={c.id === selected}
                    className={cn('cursor-pointer border-t border-line/50 hover:bg-white/[0.03]', c.id === selected && 'bg-live/10')}>
                    <td className="py-1" style={{ paddingLeft: (depth - 1) * 16 }}>
                      <span className="inline-flex items-center gap-1">
                        {branch
                          ? <button type="button" aria-label={folded ? `open ${c.id}` : `fold ${c.id}`} aria-expanded={!folded}
                              onClick={(e) => { e.stopPropagation(); toggle(c.id) }} className="rounded text-ink-faint hover:text-live">
                              {folded ? <ChevronRight size={12} /> : <ChevronDown size={12} />}
                            </button>
                          : <span className="inline-block w-3" />}
                        <span className="text-ink">{c.name}</span>
                      </span>
                      <div className="num pl-4 text-[10px] text-ink-faint">{c.id}</div>
                    </td>
                    <td className="num px-1 text-ink-dim">{c.kind}</td>
                    <td className="num px-1 text-right text-ink-dim">
                      {folded ? <span title={`${c.members ?? 0} in it, ${within.get(c.id) ?? 0} with those below it`}>{(within.get(c.id) ?? 0).toLocaleString()}<span className="text-ink-faint"> in all</span></span>
                        : (c.members ?? 0) > 0 ? (c.members ?? 0).toLocaleString() : <span className="text-ink-faint">—</span>}
                    </td>
                    <td className="whitespace-nowrap px-1 text-ink-faint">{c.added_by}</td>
                    <td className="px-1 text-ink-faint">{c.description}</td>
                    <td className="num pl-1 text-ink-dim">{c.guidance && c.guidance.text !== '' ? <span title={`digest ${c.guidance.digest}`}>v{c.guidance.version} · {c.guidance.digest}</span> : <span className="text-ink-faint">—</span>}</td>
                  </tr>
                  )
                })}
              </tbody>
            </table></div>
          )}
        </Panel>
        <div className="flex flex-col gap-3">
          <GuidanceEditor key={`${picked?.id ?? ''}:${picked?.guidance?.digest ?? ''}`} category={picked} disabled={past} />
          <AddTopic rows={rows.map((r) => r.category)} disabled={past} parentDefault={picked?.kind === 'topic' ? picked.id : ''} />
        </div>
      </div>
    </div>
  )
}

/** One category's guidance, replaced whole (`ontology.guidance.set`); empty text takes it away. The confirm carries the text. */
function GuidanceEditor({ category, disabled }: { category: OntologyCategory | null; disabled: boolean }) {
  const { write, busy, refused } = useOntologyWrite()
  const current = category?.guidance?.text ?? ''
  // The parent keys this on the category and its guidance, so a new selection, or a change from another surface, starts
  // the draft over from what is stored.
  const [text, setText] = useState(current)
  if (!category) return <Panel title="guidance" icon={<Pencil size={13} />}><Empty>pick a category to read or edit its guidance</Empty></Panel>
  const g = category.guidance
  const dirty = text !== current
  const save = async () => {
    const what = text.trim() === ''
      ? `Take away the guidance of ${category.id}?${current ? `\n\nIt now reads:\n\n${current}` : ''}`
      : `Set the guidance of ${category.id}${g ? ` (now v${g.version}, it becomes v${g.version + 1})` : ' (new)'}?\n\nIt will read:\n\n${text}\n\nA session that carries it recompiles once, at its next turn.`
    const p: OntologyGuidanceSetParams = { category: category.id, text }
    await write<OntologyGuidance>(what, 'ontology.guidance.set', p)
  }
  return (
    <Panel title={<>guidance · {category.name}</>} icon={<Pencil size={13} />}>
      <div className="flex flex-col gap-2 p-3">
        <div className="num text-[11px] text-ink-faint">{category.id} · {g && g.text !== '' ? `v${g.version} · ${g.digest} · by ${g.added_by}` : 'none yet'}</div>
        <textarea value={text} onChange={(e) => setText(e.target.value)} rows={8} disabled={disabled} aria-label={`guidance of ${category.id}`}
          placeholder="What the model should know in a session that belongs here. Empty text takes the guidance away." className={cn(field, 'font-mono')} />
        <Refused words={refused} />
        <div className="flex items-center gap-2">
          <Act onClick={() => void save()} busy={busy} off={disabled} title="ontology.guidance.set: confirmed first, with this text">
            {text.trim() === '' && current !== '' ? 'Take away' : 'Save guidance'}
          </Act>
          {dirty && <button onClick={() => setText(current)} className="text-[11px] text-ink-faint hover:text-ink">revert</button>}
          {disabled && <span className="text-[11px] text-wait">the present only</span>}
        </div>
      </div>
    </Panel>
  )
}

/** `ontology.category.add`: a topic, its id made from its name, under a parent or at the root. */
function AddTopic({ rows, disabled, parentDefault }: { rows: OntologyCategory[]; disabled: boolean; parentDefault: string }) {
  const { write, busy, refused } = useOntologyWrite()
  const [name, setName] = useState('')
  const [description, setDescription] = useState('')
  // The parent follows the selected topic until one is picked here.
  const [chosen, setParent] = useState<string | null>(null)
  const parent = chosen ?? parentDefault
  const topics = rows.filter((c) => c.kind === 'topic')
  const add = async () => {
    const p: OntologyCategoryAddParams = { kind: 'topic', name: name.trim(), ...(parent ? { parent } : {}), ...(description.trim() ? { description: description.trim() } : {}) }
    const out = await write<OntologyCategory>(
      `Add the topic “${p.name}”${parent ? ` under ${parent}` : ' at the root'}?${p.description ? `\n\nIt is described as: ${p.description}` : ''}`,
      'ontology.category.add', p,
    )
    if (out) { setName(''); setDescription('') }
  }
  return (
    <Panel title="add a topic" icon={<Plus size={13} />}>
      <form className="flex flex-col gap-2 p-3" onSubmit={(e) => { e.preventDefault(); if (name.trim()) void add() }}>
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder="name" aria-label="topic name" disabled={disabled} className={field} />
        <input value={description} onChange={(e) => setDescription(e.target.value)} placeholder="description (optional)" aria-label="topic description" disabled={disabled} className={field} />
        <label className="flex items-center gap-2 text-[12px] text-ink-faint">
          <Compass size={12} /> parent
          <select value={parent} onChange={(e) => setParent(e.target.value)} disabled={disabled} aria-label="parent topic" className={cn(field, 'min-w-0 flex-1')}>
            <option value="" className="bg-deck">none: a root</option>
            {topics.map((c) => <option key={c.id} value={c.id} className="bg-deck">{c.id}</option>)}
          </select>
        </label>
        <Refused words={refused} />
        <div className="flex items-center gap-2">
          <Act onClick={() => { if (name.trim()) void add() }} busy={busy} off={disabled} title="ontology.category.add: confirmed first"><Plus size={13} /> Add topic</Act>
          {disabled && <span className="text-[11px] text-wait">the present only</span>}
        </div>
      </form>
    </Panel>
  )
}
