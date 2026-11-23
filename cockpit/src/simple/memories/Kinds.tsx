// Memories, people and topics: a list on the left and the chosen one's page on the right. A person's page is who they
// are (their name, how they are reached, the role Theseus has on file) and the conversations they are in; a topic's page
// is where it sits in the tree, what Theseus is told to do under it, what is filed beneath it, and its conversations.
// The chosen one is `?sel=`, the list's words `?f=`, so any page is a link. Both read the ontology; neither writes it:
// adding, merging and guidance stay on the Configuration page and the Ship's Ontology page.
import type { ReactNode } from 'react'
import { Link, useSearchParams } from 'react-router'
import { ListTree, Search as SearchIcon, UserRound } from 'lucide-react'
import type { OntologyCategory, OntologyListResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { count } from '@/lib/explorer'
import { Empty, Panel } from '@/components/ui'
import { handlesLine, peopleOf } from '@/lib/people'
import { childrenOf, exploreTopic, holdersOf, labelOf, lineOf, listHref, pathOf, searchHref, topicHref, topicsOf } from './find'
import { Conversations, LinkRow, useCategories } from './parts'

/** The most rows the list draws; a longer one says to narrow it. */
const LISTED = 200

export function People() {
  return <Kinds kind="person" />
}

export function Topics() {
  return <Kinds kind="topic" />
}

const WORDS = {
  person: { many: 'People', one: 'person', list: 'Everyone Theseus has on file', filter: 'Find a person by name or handle', none: 'Theseus has no people on file yet.', miss: 'Nobody matches those words.' },
  topic: { many: 'Topics', one: 'topic', list: 'Everything Theseus sorts conversations under', filter: 'Find a topic', none: 'Theseus has no topics on file yet.', miss: 'No topic matches those words.' },
} as const

function Kinds({ kind }: { kind: 'person' | 'topic' }) {
  const [params, setParams] = useSearchParams()
  const f = params.get('f') ?? ''
  const sel = params.get('sel')
  const { data } = useCategories()
  const cats = data?.categories ?? []
  const all = cats.filter((c) => c.kind === kind)
  const list = kind === 'person' ? peopleOf(cats, f) : topicsOf(cats, f)
  const picked = all.find((c) => c.id === sel)
  const w = WORDS[kind]
  const filter = (t: string) => setParams((p) => { if (t) p.set('f', t); else p.delete('f'); return p }, { replace: true })
  return (
    <div className="grid grid-cols-1 gap-4 pb-6 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
      <Panel help="simple.memories.list" title={`${w.many} · ${data ? count(all.length) : '…'}`} icon={kind === 'person' ? <UserRound size={13} /> : <ListTree size={13} />}
        className="lg:max-h-[calc(100vh-190px)]" bodyClassName="flex flex-col">
        <label className="flex items-center gap-2 border-b border-line px-3 py-2 text-ink-faint">
          <SearchIcon size={13} aria-hidden />
          <input value={f} onChange={(e) => filter(e.target.value)} placeholder={w.filter} aria-label={w.filter}
            className="min-w-0 flex-1 bg-transparent text-[13px] text-ink outline-none placeholder:text-ink-faint" />
        </label>
        <div className="min-h-0 flex-1 overflow-auto p-2" role="list" aria-label={w.many.toLowerCase()}>
          {!data ? <Empty>reading…</Empty>
            : !all.length ? <Empty>{w.none}</Empty>
            : !list.length ? <Empty>{w.miss}</Empty>
            : list.slice(0, LISTED).map((c) => (
              <LinkRow key={c.id} to={listHref(kind, c.id, f)} current={c.id === sel} title={c.name} sub={lineOf(cats, c)} />
            ))}
          {list.length > LISTED && <div className="px-3 py-2 text-[12px] text-ink-faint">{count(list.length - LISTED)} more: type to narrow the list</div>}
        </div>
      </Panel>
      {picked ? (kind === 'person' ? <PersonPage c={picked} /> : <TopicPage c={picked} cats={cats} />)
        : (
          <Panel help="simple.memories.page" title={`Choose a ${w.one}`}>
            <div className="p-4 text-[13px] text-ink-dim">{w.list}. Choose one to see its page: {kind === 'person' ? 'how Theseus knows them and the conversations they are in' : 'where it sits, what is filed beneath it and its conversations'}.</div>
          </Panel>
        )}
    </div>
  )
}

/** A category's sessions: one read of every membership, shared by the pages and held for a while. */
function useHolders(c: OntologyCategory) {
  const { data, error } = useRpc<OntologyListResult>('ontology.list', { memberships: true }, 0)
  return { holders: data ? holdersOf(data.memberships, c.id) : null, error }
}

function HeldConversations({ c }: { c: OntologyCategory }) {
  const { holders, error } = useHolders(c)
  return (
    <Panel help="simple.memories.conversations" title={`Conversations${holders ? ` · ${count(holders.total)}` : ''}`}>
      {error ? <div className="p-4 text-[12.5px] text-fault">Theseus could not read these: {(error as Error).message}</div>
        : !holders ? <Empty>reading…</Empty>
        : <Conversations imported={holders.imported} native={holders.native} total={holders.total} />}
    </Panel>
  )
}

/** One fact of a page: its name over its value, which may run to several lines. */
function Fact({ label, children }: { label: string; children: ReactNode }) {
  return (
    <div className="min-w-0">
      <div className="simple-eyebrow mb-0.5">{label}</div>
      <div className="break-words text-[13px] text-ink">{children}</div>
    </div>
  )
}

function PersonPage({ c }: { c: OntologyCategory }) {
  const handles = handlesLine(c)
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <Panel help="simple.memories.page" title={c.name} icon={<UserRound size={13} />}>
        <div className="grid grid-cols-1 gap-x-4 gap-y-3 p-4 text-[13px] sm:grid-cols-2">
          <Fact label="How Theseus reaches them">{handles || 'No handle on file'}</Fact>
          <Fact label="Filed by">{c.added_by}</Fact>
          <Fact label="Their role, as Theseus has it">{c.description || 'None written'}</Fact>
          <Fact label="Conversations">{count(c.members)}</Fact>
        </div>
        <div className="flex gap-4 border-t border-line px-4 py-2.5 text-[12.5px]">
          <Link to={searchHref(c.name)} className="text-live hover:underline">Search for “{c.name}”</Link>
        </div>
      </Panel>
      <HeldConversations c={c} />
    </div>
  )
}

function TopicPage({ c, cats }: { c: OntologyCategory; cats: OntologyCategory[] }) {
  const up = pathOf(cats, c).slice(0, -1)
  const below = childrenOf(cats, c.id)
  return (
    <div className="flex min-w-0 flex-col gap-4">
      <Panel help="simple.memories.page" title={c.name} icon={<ListTree size={13} />}>
        <div className="flex flex-col gap-3 p-4 text-[13px]">
          {up.length > 0 && (
            <nav aria-label="where this topic sits" className="flex flex-wrap items-center gap-1 text-[12.5px] text-ink-faint">
              {up.map((p) => <span key={p.id} className="flex items-center gap-1"><Link to={topicHref(p.id)} className="text-live hover:underline">{p.name}</Link><span aria-hidden>/</span></span>)}
              <span className="text-ink">{c.name}</span>
            </nav>
          )}
          <Fact label="What it is">{c.description || 'No description written'}</Fact>
          {c.guidance && c.guidance.text && (
            <Fact label={`What Theseus is told under it (version ${c.guidance.version})`}><span className="whitespace-pre-wrap">{c.guidance.text}</span></Fact>
          )}
          <Fact label="Conversations">{count(c.members)}</Fact>
        </div>
        <div className="flex flex-wrap gap-4 border-t border-line px-4 py-2.5 text-[12.5px]">
          <Link to={searchHref(c.name)} className="text-live hover:underline">Search for “{c.name}”</Link>
          <Link to={exploreTopic(labelOf(cats, c))} className="text-live hover:underline">Browse everything under it among the imported conversations</Link>
        </div>
      </Panel>
      {below.length > 0 && (
        <Panel help="simple.memories.below" title={`Filed beneath it · ${below.length}`}>
          <div className="p-2">{below.map((b) => <LinkRow key={b.id} to={topicHref(b.id)} title={b.name} sub={lineOf(cats, b)} />)}</div>
        </Panel>
      )}
      <HeldConversations c={c} />
    </div>
  )
}
