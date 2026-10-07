// The context explorer (theseus-7n3e): what Theseus knows and what a turn sees. Its holdings (the owner's sessions, the
// imported episodes and their nodes, the topics, the books' state, the index), then four tabs: the imported episodes
// (`import.sessions`: filtered, counted by facet, a page at a time, each opened to its provenance, summary and
// messages), a turn's context (`context.explain`, in components/ContextTurn.tsx), asking the index (`memory.search`),
// and the books. Read only: no control here writes. Every filter, the tab, the page and the episode open are in the
// address, so any view deep-links. The pure parts are lib/explorer.ts.
import { useMemo, useState, type ReactNode } from 'react'
import { useSearchParams } from 'react-router'
import { BookOpen, ChevronDown, ChevronRight, Database, Eye, EyeOff, FileText, Layers, Library, ListTree, MessagesSquare, Search, Shapes, Telescope, X } from 'lucide-react'
import type { Health, ImportFacet, ImportListResult, ImportSessionsResult, ImportedEpisode, NodeInfo, OntologyListResult, SessionHistoryResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { cn, stamp } from '@/lib/format'
import { CATEGORICAL } from '@/lib/viz'
import { useMode } from '@/lib/mode'
import { daylightColor } from '@/lib/daylight'
import { useAsOf } from '@/lib/timemachine'
import { Empty, Field, Panel, Pill } from '@/components/ui'
import { ChartPanel, StatTile, TipArea, TipBody, TipTarget } from '@/components/ChartPanel'
import { useInPast } from '@/components/OntologyParts'
import { AskIndex, TurnContext } from '@/components/ContextTurn'
import {
  PAGE, TABS, ancestors, count, filtersOf, monthBins, namedParams, ontologyPaths, paramsOf, sensitivityWords, tabOf, topicTree, veiled, when,
  type FilterKey, type Tab,
} from '@/lib/explorer'

type Patch = Record<string, string | null | undefined>
type SetParams = (patch: Patch) => void

/** The address, patched: a null or empty value takes its key away. A filter's change goes back to the first page. */
function useAddress(): [URLSearchParams, SetParams] {
  const [params, setParams] = useSearchParams()
  const set: SetParams = (patch) => setParams((p) => {
    for (const [k, v] of Object.entries(patch)) { if (v) p.set(k, v); else p.delete(k) }
    return p
  }, { replace: true })
  return [params, set]
}

const TAB_WORDS: Record<Tab, { word: string; icon: ReactNode }> = {
  episodes: { word: 'Episodes', icon: <Library size={13} /> },
  turn: { word: 'A turn’s context', icon: <Layers size={13} /> },
  search: { word: 'Ask the index', icon: <Search size={13} /> },
  books: { word: 'The books', icon: <BookOpen size={13} /> },
}

export default function Context() {
  const [params, set] = useAddress()
  const tab = tabOf(params)
  const past = useInPast()
  const asOf = useAsOf((s) => s.t)
  return (
    <div className="flex flex-col gap-3">
      {past && <div className="num px-1 text-[11.5px] text-wait">the ship’s log is at {stamp(asOf ?? 0)}; what Theseus knows keeps no past here, so this is the present</div>}
      <Holdings go={(t, patch) => set({ tab: t === 'episodes' ? null : t, ...patch })} />
      <nav className="flex flex-wrap items-center gap-1" aria-label="the explorer's tabs">
        {TABS.map((t) => (
          <button key={t} type="button" onClick={() => set({ tab: t === 'episodes' ? null : t })} aria-current={tab === t ? 'page' : undefined}
            className={cn('flex items-center gap-1.5 rounded-md px-2.5 py-1.5 text-[12px] ring-1 ring-inset transition-colors',
              tab === t ? 'bg-live/10 text-live ring-live/30' : 'text-ink-dim ring-line hover:text-ink')}>
            {TAB_WORDS[t].icon}{TAB_WORDS[t].word}
          </button>
        ))}
      </nav>
      {tab === 'episodes' && <Episodes params={params} set={set} />}
      {tab === 'turn' && <TurnContext session={params.get('session')} turn={params.get('turn')} set={set} />}
      {tab === 'search' && <AskIndex ask={params.get('ask') ?? ''} set={set} />}
      {tab === 'books' && <Books go={(book) => set({ tab: null, book, page: null, episode: null })} />}
    </div>
  )
}

// ---------------------------------------------------------------- holdings

/** What Theseus knows, in six numbers, each opening its tab. */
function Holdings({ go }: { go: (tab: Tab, patch?: Patch) => void }) {
  const { data: h } = useRpc<Health>('health', undefined, 10_000)
  const { data: tags } = useRpc<ImportListResult>('import.list', undefined, 30_000)
  const { data: all } = useRpc<ImportSessionsResult>('import.sessions', { limit: 1 }, 30_000)
  const { data: onto } = useRpc<OntologyListResult>('ontology.list', { memberships: false }, 60_000)
  const imported = (tags?.tags ?? []).reduce((a, t) => ({ s: a.s + t.sessions - t.erased, n: a.n + t.nodes, e: a.e + t.erased, src: a.src + Object.keys(t.sources).length }), { s: 0, n: 0, e: 0, src: 0 })
  const first = Math.min(...(tags?.tags ?? []).map((t) => t.first_ms ?? Infinity))
  const last = Math.max(...(tags?.tags ?? []).map((t) => t.last_ms ?? 0))
  const topics = all?.facets.topics ?? []
  const roots = topics.filter((t) => !t.value.includes('/')).length
  const ontoTopics = (onto?.categories ?? []).filter((c) => c.kind === 'topic').length
  const hinted = (all?.facets.books ?? []).reduce((a, b) => a + b.count, 0)
  const index = h?.index
  const day = (ms: number) => (Number.isFinite(ms) && ms > 0 ? new Date(ms).toISOString().slice(0, 10) : '—')
  return (
    <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-6">
      <StatTile label="own sessions" icon={<MessagesSquare size={12} />} value={h?.sessions ?? 0} format={count}
        hint={`${count(h?.turns ?? 0)} turns · open one’s context`} onClick={() => go('turn')} />
      <StatTile label="imported episodes" icon={<Library size={12} />} value={imported.s} format={count}
        hint={imported.s ? `${imported.src} sources · ${day(first)} to ${day(last)}${imported.e ? ` · ${count(imported.e)} erased` : ''}` : 'no import yet'} onClick={() => go('episodes')} />
      <StatTile label="their nodes" icon={<Database size={12} />} value={imported.n} format={count}
        hint="messages and summaries, recall’s testimony" onClick={() => go('episodes')} />
      <StatTile label="topics" icon={<ListTree size={12} />} value={topics.length} format={count}
        hint={ontoTopics ? `${roots} roots · ${count(ontoTopics)} in the ontology` : `${roots} roots, from the labels · none in the ontology yet`} onClick={() => go('episodes')} />
      <StatTile label="book hints" icon={<BookOpen size={12} />} value={hinted} format={count}
        hint="the books are not compiled yet" onClick={() => go('books')} />
      <StatTile label="indexed nodes" icon={<Telescope size={12} />} value={index?.status?.nodes ?? 0} format={count}
        hint={index ? `${index.state}${index.status?.vectors ? ` · vectors ${count(index.status.vectors.vectors)} of ${count(index.status.vectors.chunks)} chunks` : ''}` : 'reading…'} onClick={() => go('search')} />
    </div>
  )
}

// ---------------------------------------------------------------- episodes

const FACETS: { key: FilterKey; facet: 'tags' | 'sources' | 'places' | 'sensitivities' | 'books'; word: string }[] = [
  { key: 'source', facet: 'sources', word: 'source' },
  { key: 'place', facet: 'places', word: 'place' },
  { key: 'sens', facet: 'sensitivities', word: 'sensitivity' },
  { key: 'book', facet: 'books', word: 'book hint' },
  { key: 'tag', facet: 'tags', word: 'import' },
]

function Episodes({ params, set }: { params: URLSearchParams; set: SetParams }) {
  const filters = filtersOf(params)
  const sort = params.get('sort')
  const page = Math.max(1, Number(params.get('page') ?? 1) || 1)
  const episode = params.get('episode')
  const veilOn = params.get('veil') !== 'off'
  const [opened, setOpened] = useState<ReadonlySet<string>>(() => new Set())
  const open = (id: string) => setOpened((s) => new Set([...s, id]))
  // The query's key is its params' value, so a new object each render reads nothing again.
  const query = paramsOf(filters, sort, page)
  const { data, error, isFetching } = useRpc<ImportSessionsResult>('import.sessions', query, 30_000)
  const filter = (k: FilterKey, v: string | null) => set({ [k]: v, page: null })
  const pages = data ? Math.max(1, Math.ceil(data.total / PAGE)) : 1
  const picked = data?.episodes.find((e) => e.session_id === episode)
  return (
    <div className={cn('grid grid-cols-1 gap-3', episode ? 'xl:grid-cols-[250px_minmax(0,1fr)_minmax(0,1.05fr)]' : 'lg:grid-cols-[250px_minmax(0,1fr)]')}>
      <aside className="flex flex-col gap-3" aria-label="filters">
        <Panel title="filters" icon={<Shapes size={13} />} actions={Object.keys(filters).length ? <button type="button" className="text-[11px] text-ink-faint hover:text-ink" onClick={() => set({ tag: null, source: null, place: null, sens: null, book: null, topic: null, q: null, month: null, page: null })}>clear all</button> : undefined}>
          <Words key={filters.q ?? ''} initial={filters.q ?? ''} submit={(q) => filter('q', q.trim() || null)} />
          {FACETS.map((f) => <FacetGroup key={f.key} word={f.word} values={data?.facets[f.facet] ?? []} picked={filters[f.key]} pick={(v) => filter(f.key, v)} format={f.key === 'sens' ? (v) => sensitivityWords(v).word : undefined} />)}
          {filters.month && <div className="flex items-center gap-1.5 px-3 pb-2 text-[11.5px] text-ink-dim">as of <span className="num text-ink">{filters.month.replace('..', ' to ')}</span><button type="button" aria-label="clear the months" onClick={() => filter('month', null)} className="text-ink-faint hover:text-ink"><X size={11} /></button></div>}
        </Panel>
        <TopicPanel topics={data?.facets.topics ?? []} picked={filters.topic} pick={(v) => filter('topic', v)} />
      </aside>
      <div className="flex min-w-0 flex-col gap-3">
        <Timeline months={data?.facets.months ?? []} picked={filters.month} pick={(m) => filter('month', m)} />
        <Panel title={<>episodes · {data ? `${count(data.total)} of ${count(data.all)}` : '…'}</>} icon={<Library size={13} />}
          actions={<>
            <select aria-label="sort" value={sort ?? 'newest'} onChange={(e) => set({ sort: e.target.value === 'newest' ? null : e.target.value, page: null })}
              className="rounded-md bg-white/5 px-1.5 py-0.5 text-[11px] text-ink-dim ring-1 ring-line">
              <option value="newest">newest</option><option value="oldest">oldest</option><option value="longest">longest</option>
            </select>
            <button type="button" onClick={() => set({ veil: veilOn ? 'off' : null })} title={veilOn ? 'personal and partner-confidential text is veiled on screen until you open it' : 'the veil is off: every text shows'}
              className="flex items-center gap-1 rounded-md px-1.5 py-0.5 text-[11px] text-ink-dim ring-1 ring-line hover:text-ink">
              {veilOn ? <EyeOff size={11} /> : <Eye size={11} />}{veilOn ? 'veiled' : 'unveiled'}
            </button>
          </>}>
          {error ? <Empty>the daemon refused the read: {(error as Error).message}</Empty>
            : !data ? <Empty>reading the episodes…</Empty>
            : data.all === 0 ? <Empty>no imported episodes: <span className="num">theseus import openclaw</span> brings the owner’s history in</Empty>
            : (
              <>
                {data.withheld && <div className="px-3 pt-2 text-[11.5px] text-wait">{data.withheld}</div>}
                <ul className={cn('divide-y divide-line/50', isFetching && 'opacity-70')} aria-label="episodes">
                  {data.episodes.map((e) => <EpisodeRow key={e.session_id} e={e} picked={e.session_id === episode} veil={veiled(e, veilOn, opened)}
                    onOpen={() => set({ episode: e.session_id === episode ? null : e.session_id })} onTopic={(t) => filter('topic', t)} />)}
                  {!data.episodes.length && <li><Empty>no episode meets every filter</Empty></li>}
                </ul>
                <div className="num flex items-center gap-2 border-t border-line px-3 py-2 text-[11px] text-ink-faint">
                  <span>{data.total ? `${count(data.offset + 1)}–${count(data.offset + data.episodes.length)} of ${count(data.total)}` : '0'}</span>
                  <span title="what the read took, on the daemon">· {data.ms.toFixed(1)} ms{data.built_ms != null ? ` (indexed ${count(data.all)} in ${data.built_ms.toFixed(0)} ms)` : ''}</span>
                  <span className="ml-auto flex items-center gap-1">
                    <button type="button" disabled={page <= 1} onClick={() => set({ page: page > 2 ? String(page - 1) : null })} className="rounded px-2 py-0.5 ring-1 ring-line enabled:hover:text-ink disabled:opacity-40">previous</button>
                    <span>page {page} of {count(pages)}</span>
                    <button type="button" disabled={page >= pages} onClick={() => set({ page: String(page + 1) })} className="rounded px-2 py-0.5 ring-1 ring-line enabled:hover:text-ink disabled:opacity-40">next</button>
                  </span>
                </div>
              </>
            )}
        </Panel>
      </div>
      {episode && <EpisodeDetail key={episode} id={episode} listed={picked} veil={(e) => veiled(e, veilOn, opened)} onUnveil={() => open(episode)} close={() => set({ episode: null })} onTopic={(t) => filter('topic', t)} />}
    </div>
  )
}

/** The words box: its draft starts from the address's words, and the parent keys it on them, so a change from elsewhere
 *  (a cleared filter, a deep link) starts it over. */
function Words({ initial, submit }: { initial: string; submit: (q: string) => void }) {
  const [q, setQ] = useState(initial)
  return (
    <form className="p-2" onSubmit={(e) => { e.preventDefault(); submit(q) }}>
      <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="words in a title, a place, a topic" aria-label="words"
        className="w-full rounded-md bg-white/5 px-2.5 py-1.5 text-[12px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40" />
    </form>
  )
}

/** One facet's values, each with its count over what the other filters keep; a click picks it, a second clears it. */
function FacetGroup({ word, values, picked, pick, format }: { word: string; values: ImportFacet[]; picked?: string; pick: (v: string | null) => void; format?: (v: string) => string }) {
  const [all, setAll] = useState(false)
  if (!values.length && !picked) return null
  const shown = all ? values : values.slice(0, 6)
  return (
    <div className="border-t border-line/60 px-3 py-2">
      <div className="mb-1 text-[10px] uppercase tracking-wider text-ink-faint">{word}</div>
      <ul className="flex flex-col gap-0.5">
        {shown.map((v) => (
          <li key={v.value}>
            <button type="button" onClick={() => pick(picked === v.value ? null : v.value)} aria-pressed={picked === v.value}
              className={cn('flex w-full items-center gap-2 rounded px-1.5 py-0.5 text-left text-[11.5px] hover:bg-white/[0.04]', picked === v.value ? 'bg-live/10 text-live' : 'text-ink-dim')}>
              <span className="min-w-0 flex-1 truncate" title={v.value}>{format ? format(v.value) : v.value}</span>
              <span className="num text-ink-faint">{count(v.count)}</span>
            </button>
          </li>
        ))}
      </ul>
      {values.length > 6 && <button type="button" onClick={() => setAll(!all)} className="mt-0.5 px-1.5 text-[11px] text-ink-faint hover:text-ink">{all ? 'fewer' : `all ${values.length}`}</button>}
    </div>
  )
}

/** The topics, a tree from the labels' slash paths, every branch with its episodes; each says when the ontology holds it
 *  (theseus-anh3 makes the labels ontology topics). */
function TopicPanel({ topics, picked, pick }: { topics: ImportFacet[]; picked?: string; pick: (v: string | null) => void }) {
  const { data: onto } = useRpc<OntologyListResult>('ontology.list', { memberships: false }, 60_000)
  const paths = useMemo(() => ontologyPaths(onto?.categories ?? []), [onto])
  const [opened, setOpen] = useState<ReadonlySet<string>>(() => new Set())
  // The picked topic's branches are always open, so a deep link shows it.
  const open = useMemo(() => new Set([...opened, ...ancestors(picked)]), [opened, picked])
  const rows = useMemo(() => topicTree(topics, open, paths), [topics, open, paths])
  const toggle = (p: string) => setOpen((s) => { const n = new Set(s); if (n.has(p)) n.delete(p); else n.add(p); return n })
  const inOntology = paths.size
  return (
    <Panel title={<>topics · {count(topics.length)}</>} icon={<ListTree size={13} />}>
      <div className="px-3 pt-2 text-[11px] text-ink-faint">
        {inOntology ? `${count(inOntology)} topics are in the ontology; ◆ marks them` : 'from the episodes’ labels: the ontology holds no topic yet (theseus-anh3 brings them in)'}
      </div>
      {!rows.length ? <Empty>no topics</Empty> : (
        <ul className="max-h-[420px] overflow-auto p-2" aria-label="topic tree">
          {rows.map((r) => (
            <li key={r.path} className="flex items-center" style={{ paddingLeft: r.depth * 12 }}>
              {r.parent
                ? <button type="button" aria-label={open.has(r.path) ? `close ${r.path}` : `open ${r.path}`} onClick={() => toggle(r.path)} className="text-ink-faint hover:text-ink">{open.has(r.path) ? <ChevronDown size={12} /> : <ChevronRight size={12} />}</button>
                : <span className="inline-block w-3" />}
              <button type="button" onClick={() => pick(picked === r.path ? null : r.path)} aria-pressed={picked === r.path} title={r.path}
                className={cn('flex min-w-0 flex-1 items-center gap-1.5 rounded px-1 py-0.5 text-left text-[11.5px] hover:bg-white/[0.04]', picked === r.path ? 'bg-live/10 text-live' : 'text-ink-dim')}>
                <span className="min-w-0 flex-1 truncate">{r.name}</span>
                {r.ontology && <span className="text-gold" title={`in the ontology${r.members != null ? `: ${count(r.members)} members` : ''}`}>◆</span>}
                <span className="num text-ink-faint">{count(r.count)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </Panel>
  )
}

/** The episodes a month by their span's start, a column a month, gaps as empty months; a click keeps that month. Its
 *  table is the same months. */
function Timeline({ months, picked, pick }: { months: ImportFacet[]; picked?: string; pick: (m: string | null) => void }) {
  const bins = useMemo(() => monthBins(months), [months])
  const max = Math.max(1, ...bins.map((b) => b.count))
  const day = useMode((s) => s.mode) === 'light'
  const accent = day ? daylightColor(CATEGORICAL.dark[0]) : CATEGORICAL.dark[0]
  const total = bins.reduce((a, b) => a + b.count, 0)
  return (
    <ChartPanel id="asof" title={<>as of · episodes a month{bins.length ? ` · ${bins[0].value} to ${bins[bins.length - 1].value}` : ''}</>} icon={<FileText size={13} />} height={132}
      empty={bins.length ? undefined : 'no months'}
      table={{
        caption: 'imported episodes a month, by their span’s start', rows: [...bins].reverse(), rowKey: (b) => b.value,
        columns: [{ key: 'm', label: 'month', cell: (b) => b.value }, { key: 'n', label: 'episodes', num: true, cell: (b) => count(b.count) }, { key: 's', label: 'share', num: true, cell: (b) => `${total ? ((b.count / total) * 100).toFixed(1) : '0'}%` }],
      }}>
      <TipArea className="flex h-full flex-col">
        <div className="flex min-h-0 flex-1 items-end gap-px" role="group" aria-label="episodes a month">
          {bins.map((b) => {
            const on = !picked || picked === b.value || (picked.includes('..') && b.value >= picked.split('..')[0] && b.value <= picked.split('..')[1])
            return (
              <TipTarget key={b.value} label={`${b.value}: ${b.count} episodes`} onClick={() => pick(picked === b.value ? null : b.value)}
                className="viz-mark h-full min-w-[3px] flex-1 cursor-pointer"
                tip={<TipBody head={b.value} rows={[{ value: count(b.count), label: 'episodes began this month', color: accent, mark: 'rect' }]} foot={picked === b.value ? 'click again to clear' : 'click to keep this month'} />}>
                <span className="flex h-full w-full items-end">
                  <span className="block w-full rounded-t-[2px]" style={{ height: `${b.count ? Math.max(3, (b.count / max) * 100) : 0}%`, background: accent, opacity: on ? 1 : 0.28 }} />
                </span>
              </TipTarget>
            )
          })}
        </div>
        <div className="num mt-1 flex justify-between text-[10px] text-ink-faint">
          <span>{bins[0]?.value}</span><span>most: {count(max)} in a month</span><span>{bins[bins.length - 1]?.value}</span>
        </div>
      </TipArea>
    </ChartPanel>
  )
}

/** A sensitivity's chip: its words, its tone beside them. */
function Sensitivity({ s }: { s: string }) {
  const w = sensitivityWords(s)
  return <Pill tone={w.tone} title={`labelled ${w.word} by the import: a recorded fact, said here; the core gives imported text only to a private place`}>{w.word}</Pill>
}

function placeWords(e: ImportedEpisode): string {
  return e.place_name ? `${e.place_kind} · ${e.place_name}` : e.place_kind
}

function EpisodeRow({ e, picked, veil, onOpen, onTopic }: { e: ImportedEpisode; picked: boolean; veil: boolean; onOpen: () => void; onTopic: (t: string) => void }) {
  return (
    <li className={cn('px-3 py-2', picked && 'bg-live/[0.06]')}>
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[11.5px]">
        <span className="num text-ink-faint" title={`${when(e.start_ms)} to ${when(e.end_ms)}`}>{new Date(e.end_ms).toISOString().slice(0, 10)}</span>
        <span className="max-w-[24ch] truncate text-ink-dim" title={placeWords(e)}>{placeWords(e)}</span>
        <span className="text-ink-faint">{e.source}</span>
        <Sensitivity s={e.sensitivity} />
        {e.credential_redacted && <Pill tone="idle" title="the pipeline removed a credential from it">redacted</Pill>}
        {e.erased && <Pill tone="fault">erased</Pill>}
        <span className="num ml-auto text-ink-faint">{count(e.messages)} msgs</span>
      </div>
      <button type="button" onClick={onOpen} aria-expanded={picked} className="mt-1 block w-full text-left">
        {veil ? <span className="text-[12px] italic text-ink-faint">{sensitivityWords(e.sensitivity).word} · veiled on screen: open it to read</span>
          : <span className="line-clamp-2 text-[12.5px] text-ink">{e.summary_text ?? e.title ?? <span className="text-ink-faint">no summary</span>}</span>}
      </button>
      {e.topics.length > 0 && (
        <div className="mt-1 flex flex-wrap gap-1">
          {e.topics.slice(0, 4).map((t) => <button key={t} type="button" onClick={() => onTopic(t)} className="rounded bg-white/[0.04] px-1.5 py-px text-[10.5px] text-ink-faint ring-1 ring-inset ring-line hover:text-ink">{t}</button>)}
          {e.topics.length > 4 && <span className="text-[10.5px] text-ink-faint">+{e.topics.length - 4}</span>}
        </div>
      )}
    </li>
  )
}

/** The page of an episode's messages, oldest first, read a hundred at a time forward from its start. */
const MESSAGES = 100

/** An episode opened: its provenance, its labels, its summary with its cites, and its messages. */
function EpisodeDetail({ id, listed, veil: veilOf, onUnveil, close, onTopic }: { id: string; listed?: ImportedEpisode; veil: (e: ImportedEpisode) => boolean; onUnveil: () => void; close: () => void; onTopic: (t: string) => void }) {
  // An episode opened from elsewhere (a recalled note's link) may not be on the list's page: its row by its id.
  const { data: own } = useRpc<ImportSessionsResult>('import.sessions', namedParams([id]), 0, { enabled: !listed })
  const e = listed ?? own?.episodes[0]
  // Veiled until its labels are read, so a sensitive episode's text never shows before its label does.
  const veil = e ? veilOf(e) : true
  // The parent keys this on the episode, so another starts at its first page.
  const [after, setAfter] = useState<number[]>([0])
  const cursor = after[after.length - 1]
  const { data, error } = useRpc<SessionHistoryResult>('session.history', { session_id: id, after: cursor, n: MESSAGES }, 0)
  const nodes = data?.nodes ?? []
  const summary = nodes.find((n) => n.kind === 'imported_summary')
  const cites = new Set(((summary?.detail as { cites?: string[] } | null)?.cites) ?? [])
  return (
    <Panel title={veil ? `${e ? sensitivityWords(e.sensitivity).word : 'an'} episode · veiled` : e?.title ?? 'episode'} icon={<FileText size={13} />} className="min-w-0"
      actions={<button type="button" aria-label="close the episode" onClick={close} className="text-ink-faint hover:text-ink"><X size={13} /></button>}>
      <div className="flex max-h-[calc(100vh-220px)] flex-col gap-3 overflow-auto p-3">
        {e ? (
          <>
            <div className="flex flex-wrap items-center gap-1.5">
              <Sensitivity s={e.sensitivity} />
              {e.book && <Pill tone="model" title="the import's guess at the book this belongs in">book: {e.book}</Pill>}
              {e.partner && <Pill tone="fault">{e.partner}</Pill>}
              {e.triage && <Pill tone="idle" title={`the pipeline's triage: keep ${e.keep?.toFixed(2) ?? '—'}`}>{e.triage}</Pill>}
              {e.topics.map((t) => <button key={t} type="button" onClick={() => onTopic(t)} className="rounded bg-white/[0.04] px-1.5 py-px text-[10.5px] text-ink-dim ring-1 ring-inset ring-line hover:text-ink">{t}</button>)}
            </div>
            <div className="grid grid-cols-2 gap-x-4 gap-y-2 text-[12px]">
              <Field label="as of">{when(e.start_ms)} to {when(e.end_ms)}</Field>
              <Field label="where">{placeWords(e)}</Field>
              <Field label="source">{e.source}{e.agent ? ` · ${e.agent}` : ''}</Field>
              <Field label="messages">{count(e.messages)}{e.summary ? ' and a summary' : ''}</Field>
              <Field label="import" mono>{e.tag}</Field>
              <Field label="file and line" mono>{e.file.split('/').pop()}:{e.line}</Field>
              <Field label="imported" mono>{when(e.imported_at_ms)}</Field>
              <Field label="session" mono>{e.session_id.slice(0, 14)}…</Field>
            </div>
          </>
        ) : <div className="text-[11.5px] text-ink-faint">reading its labels…</div>}
        <div className="text-[11px] text-ink-faint">An imported session is the owner’s own history: closed and read-only, it takes no turn, and reaches a model only as recall’s testimony, from a private place.</div>
        {veil ? (
          <button type="button" onClick={onUnveil} className="flex items-center gap-2 rounded-md bg-white/[0.03] px-3 py-3 text-left text-[12px] text-ink-dim ring-1 ring-inset ring-line hover:text-ink">
            <EyeOff size={14} /> {sensitivityWords(e?.sensitivity ?? '').word}: its text is veiled on screen. Open it to read it here.
          </button>
        ) : (
          <>
            {summary && (
              <div>
                <div className="panel-title mb-1">summary <span className="num normal-case tracking-normal text-ink-faint">· {String((summary.detail as { model?: string } | null)?.model ?? '')} · cites {cites.size}</span></div>
                <p className="whitespace-pre-wrap text-[12.5px] leading-relaxed text-ink">{summary.text}</p>
              </div>
            )}
            <div>
              <div className="panel-title mb-1 flex items-center gap-2">messages {cursor ? <span className="num normal-case tracking-normal text-ink-faint">· a later page</span> : null}</div>
              {error ? <Empty>the daemon refused the read: {(error as Error).message}</Empty> : !data ? <Empty>reading…</Empty> : (
                <ol className="flex flex-col gap-2">
                  {nodes.filter((n) => n.kind !== 'imported_summary').map((n) => <Message key={n.node_id} n={n} cited={cites.has(n.node_id)} />)}
                </ol>
              )}
              <div className="mt-2 flex gap-2 text-[11px]">
                {after.length > 1 && <button type="button" onClick={() => setAfter(after.slice(0, -1))} className="rounded px-2 py-0.5 text-ink-faint ring-1 ring-line hover:text-ink">earlier</button>}
                {data?.next != null && <button type="button" onClick={() => setAfter([...after, data.next!])} className="rounded px-2 py-0.5 text-ink-faint ring-1 ring-line hover:text-ink">later messages</button>}
              </div>
            </div>
          </>
        )}
      </div>
    </Panel>
  )
}

const INTEGRITY: Record<string, string> = { operator: 'the operator’s words', agent: 'an agent’s words', outside: 'outside text' }

function Message({ n, cited }: { n: NodeInfo; cited: boolean }) {
  const d = (n.detail ?? {}) as { integrity?: string; idx?: number }
  const [all, setAll] = useState(false)
  const long = n.text.length > 900
  return (
    <li className={cn('rounded-md px-2.5 py-1.5 ring-1 ring-inset', cited ? 'bg-gold/[0.05] ring-gold/30' : 'ring-line/70', d.integrity === 'outside' && 'border-l-2 border-wait/60')}>
      <div className="num flex items-center gap-2 text-[10.5px] text-ink-faint">
        <span>#{d.idx ?? '—'}</span><span className="text-ink-dim">{n.author ?? ''}</span>
        {d.integrity && <span title="whose words these were, as the import recorded it">{INTEGRITY[d.integrity] ?? d.integrity}</span>}
        {cited && <span className="text-gold" title="the summary cites this message">cited</span>}
        <span className="ml-auto">{when(n.at_unix_ms)}</span>
      </div>
      <p className="mt-0.5 whitespace-pre-wrap break-words text-[12px] text-ink">{long && !all ? `${n.text.slice(0, 900)}…` : n.text}</p>
      {long && <button type="button" onClick={() => setAll(!all)} className="text-[11px] text-ink-faint hover:text-ink">{all ? 'less' : `all ${count(n.text.length)} characters`}</button>}
    </li>
  )
}

// ---------------------------------------------------------------- books

const BOOK_WORDS: Record<string, string> = {
  dictionary: 'terms and their meanings',
  encyclopedia: 'what is known about things',
  cookbook: 'procedures that worked',
  sop: 'the operator’s standing procedures',
  diary: 'what happened, day by day',
  casebook: 'decisions and what superseded them',
  register: 'tracked work',
}

/** The books, said honestly: not compiled. What is there now is the import's guess at where each episode belongs. */
function Books({ go }: { go: (book: string) => void }) {
  const { data } = useRpc<ImportSessionsResult>('import.sessions', { limit: 1 }, 30_000)
  const books = data?.facets.books ?? []
  const hinted = books.reduce((a, b) => a + b.count, 0)
  return (
    <div className="grid grid-cols-1 gap-3 lg:grid-cols-[minmax(0,2fr)_minmax(0,3fr)]">
      <Panel title="the books" icon={<BookOpen size={13} />}>
        <div className="flex flex-col gap-2 p-3 text-[12.5px] leading-relaxed text-ink-dim">
          <p><span className="text-ink">Not compiled yet.</span> The books are typed organizations of what Theseus knows: a dictionary, an encyclopedia, a cookbook, standing procedures, a diary, a casebook, a register. Their design is theseus-lqo6; the soul migration’s stages that fill them from the imported history are theseus-0lrr.7 (curated sources) and theseus-0lrr.8 (the build, the exam, the cut-over).</p>
          <p>Until then, recall reads the imported episodes themselves as testimony, and each episode carries the import’s guess at its book: {data ? <span className="num text-ink">{count(hinted)} of {count(data.all)}</span> : '…'} are hinted, on the right.</p>
        </div>
      </Panel>
      <ChartPanel id="books" title="book hints · episodes a book" icon={<Library size={13} />} height={Math.max(120, books.length * 30 + 16)}
        empty={books.length ? undefined : 'no hints'}
        table={{ caption: 'imported episodes by the book the import hinted', rows: books, rowKey: (b) => b.value,
          columns: [{ key: 'b', label: 'book', cell: (b) => b.value }, { key: 'w', label: 'what it holds', cell: (b) => BOOK_WORDS[b.value] ?? '' }, { key: 'n', label: 'episodes', num: true, cell: (b) => count(b.count) }] }}>
        <BookBars books={books} go={go} />
      </ChartPanel>
    </div>
  )
}

function BookBars({ books, go }: { books: ImportFacet[]; go: (book: string) => void }) {
  const max = Math.max(1, ...books.map((b) => b.count))
  const day = useMode((s) => s.mode) === 'light'
  const color = day ? daylightColor(CATEGORICAL.dark[0]) : CATEGORICAL.dark[0]
  return (
    <TipArea className="flex flex-col gap-1.5 p-1">
      {books.map((b) => (
        <TipTarget key={b.value} onClick={() => go(b.value)} label={`${b.value}: ${b.count} episodes`} className="viz-mark flex items-center gap-2 text-left"
          tip={<TipBody head={b.value} rows={[{ value: count(b.count), label: 'episodes hinted here', color, mark: 'rect' }]} foot="click to list them" />}>
          <span className="w-24 shrink-0 truncate text-[11.5px] text-ink-dim">{b.value}</span>
          <span className="h-3 rounded-r-[3px]" style={{ width: `${(b.count / max) * 70}%`, minWidth: 2, background: color }} />
          <span className="num text-[11px] text-ink">{count(b.count)}</span>
        </TipTarget>
      ))}
    </TipArea>
  )
}
