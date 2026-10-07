// The books, first cut (theseus-civ0): the imported episodes organized by the book the import hinted, read only. The seven
// books and the unsorted with their counts and spans (a chart with its table view); a book's episodes newest first, a
// page at a time, filtered by topic, source, and place from the book's facets; and an episode opened to its summary, its
// labels, and its messages (`session.history` of its imported session). It reads the present (`books.list`,
// `books.page`); the time machine keeps no past of the import, so while it shows a moment the view says so. The book,
// the filters, and the open episode are in the address (`?book=&topic=&source=&place=&episode=`). Nothing here moves
// but a bar easing to a new count, and that not in calm mode or under reduced motion.
import { useMemo } from 'react'
import { useSearchParams } from 'react-router'
import { useInfiniteQuery } from '@tanstack/react-query'
import { BookOpen, Filter, Library, ScrollText, X } from 'lucide-react'
import type { BookEpisode, BookFacet, BooksListResult, BooksPageResult, NodeInfo, SessionHistory } from '@protocol'
import { call, useConn, useRpc } from '@/lib/rpc'
import { cn } from '@/lib/format'
import { useCalm } from '@/lib/calm'
import { useAsOf } from '@/lib/timemachine'
import {
  BOOK_WORDS, FIRST_CUT, bookTitle, episodeWhen, facetOptions, filtersOf, filterWords, guarded, joined, labelWords,
  ordered, pageParams, shares, spanWords, type Filters,
} from '@/lib/books'
import { ChartPanel, TipArea, TipBody, TipTarget, type TableSpec } from '@/components/ChartPanel'
import { Empty, Panel, Pill } from '@/components/ui'

const field = 'min-w-0 rounded-md bg-white/5 px-2 py-1 text-[12px] text-ink outline-none ring-1 ring-line focus:ring-live/40'
const fmt = (n: number) => n.toLocaleString('en-US')

export default function Books() {
  const [params, setParams] = useSearchParams()
  const f = filtersOf(params)
  const episode = params.get('episode')
  const past = useAsOf((s) => s.t !== null)
  const { data: list, error } = useRpc<BooksListResult>('books.list', {}, 30_000)
  const set = (patch: Record<string, string | null>) => setParams((p) => {
    for (const [k, v] of Object.entries(patch)) { if (v) p.set(k, v); else p.delete(k) }
    return p
  }, { replace: true })
  const pick = (book: string) => set({ book, topic: null, source: null, place: null, episode: null })
  const q = useBookPages(f)
  const pages = q.data?.pages
  const episodes = useMemo(() => joined((pages ?? []).map((p) => p.episodes)), [pages])

  return (
    <div className="flex flex-col gap-3">
      <div className="flex items-start gap-2 rounded-md bg-think/10 px-3 py-2 text-[12px] text-ink-dim ring-1 ring-inset ring-think/30" role="note">
        <BookOpen size={14} className="mt-0.5 shrink-0 text-think" />
        <span>{FIRST_CUT}</span>
      </div>
      {past && <div className="num px-1 text-[11.5px] text-wait">the ship’s log shows a past moment; the books keep no past, so this is the present</div>}
      {list && !list.indexed && <div className="num px-1 text-[11.5px] text-wait">the daemon is still indexing the books after an upgrade: this answer read every imported session, and is slow until the index is whole</div>}
      <Shelf list={list} error={error as Error | null} book={f.book} onPick={pick} />
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        <BookPages f={f} q={q} episodes={episodes} selected={episode} onFilter={(k, v) => set({ [k]: v, episode: null })} onOpen={(sid) => set({ episode: sid })} />
        <EpisodePanel sessionId={episode} e={episodes.find((x) => x.session_id === episode)} onClose={() => set({ episode: null })} />
      </div>
    </div>
  )
}

interface ShelfRow { book: string; episodes: number; span: string; share: number }

/** The books as bars of their counts, each a button that opens it; its table view is the same rows. */
function Shelf({ list, error, book, onPick }: { list?: BooksListResult; error: Error | null; book: string; onPick: (b: string) => void }) {
  const calm = useCalm((s) => s.calm)
  const rows: ShelfRow[] = useMemo(() => {
    const books = ordered(list?.books ?? [])
    const s = shares(books)
    return books.map((b, i) => ({ book: b.book, episodes: b.episodes, span: spanWords(b), share: s[i] }))
  }, [list])
  const table: TableSpec<ShelfRow> = {
    caption: 'each book’s imported episodes and their span',
    rowKey: (r) => r.book,
    rows,
    columns: [
      { key: 'book', label: 'book', cell: (r) => bookTitle(r.book) },
      { key: 'episodes', label: 'episodes', num: true, cell: (r) => fmt(r.episodes) },
      { key: 'span', label: 'first to last start', cell: (r) => r.span },
      { key: 'what', label: 'what it holds', cell: (r) => BOOK_WORDS[r.book] ?? '' },
    ],
  }
  const empty = error ? `the daemon refused the read: ${error.message}` : !list ? 'reading the books…'
    : list.episodes === 0 ? 'no imported episodes yet: `theseus import openclaw <file>` brings them in' : undefined
  return (
    <ChartPanel id="books" title={<>the books · {fmt(list?.episodes ?? 0)} episodes</>} icon={<Library size={13} />} table={table} empty={empty}>
      <TipArea>
        <ul className="flex flex-col gap-1 p-1" aria-label="the books">
          {rows.map((r) => (
            <li key={r.book}>
              <TipTarget label={`${bookTitle(r.book)}: ${fmt(r.episodes)} episodes`} onClick={() => onPick(r.book)}
                tip={<TipBody head={bookTitle(r.book)} rows={[{ value: fmt(r.episodes), label: 'episodes' }]} foot={<>{r.span} · {BOOK_WORDS[r.book]}</>} />}
                className={cn('grid w-full grid-cols-[9rem_minmax(0,1fr)_5rem] items-center gap-2 rounded-md px-2 py-1 text-left text-[12px] hover:bg-white/[0.04]',
                  r.book === book && 'bg-live/10 ring-1 ring-inset ring-live/30')}>
                <span className={cn('truncate', r.book === book ? 'text-live' : 'text-ink')}>{bookTitle(r.book)}</span>
                <span className="relative h-3 rounded-sm bg-white/[0.04]">
                  <span className={cn('absolute inset-y-0 left-0 rounded-sm', r.book === 'unsorted' ? 'bg-ink-faint/60' : 'bg-live/70', !calm && 'motion-safe:transition-[width] motion-safe:duration-500')}
                    style={{ width: `${(r.share * 100).toFixed(1)}%` }} />
                </span>
                <span className="num text-right text-ink-dim">{fmt(r.episodes)}</span>
              </TipTarget>
            </li>
          ))}
        </ul>
      </TipArea>
    </ChartPanel>
  )
}

/** A book's pages under `f`, newest first, each read once and kept: `books.page` with the last page's cursor. */
function useBookPages(f: Filters) {
  const open = useConn((s) => s.status === 'open')
  return useInfiniteQuery({
    queryKey: ['books.page', f],
    queryFn: ({ pageParam }) => call<BooksPageResult>('books.page', pageParams(f, pageParam)),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.next ?? undefined,
    enabled: open,
    staleTime: 30_000,
  })
}

/** A book's episodes, newest first, a page at a time, with its filters. */
function BookPages({ f, q, episodes, selected, onFilter, onOpen }: {
  f: Filters; q: ReturnType<typeof useBookPages>; episodes: BookEpisode[]; selected: string | null
  onFilter: (k: 'topic' | 'source' | 'place', v: string | null) => void; onOpen: (sid: string) => void
}) {
  const pages = q.data?.pages ?? []
  const first = pages[0]
  const facets = first?.facets
  const last = pages[pages.length - 1]
  return (
    <Panel title={<>{bookTitle(f.book)} · {first ? fmt(first.total) : '…'} episodes</>} icon={<ScrollText size={13} />}
      actions={<span className="num text-[10.5px] normal-case tracking-normal text-ink-faint">{BOOK_WORDS[f.book]}</span>}>
      <div className="flex flex-wrap items-center gap-2 border-b border-line px-3 py-2 text-[12px]">
        <Filter size={12} className="text-ink-faint" />
        <Facet label="topic" list={facets?.topics} value={f.topic} onChange={(v) => onFilter('topic', v)} />
        <Facet label="source" list={facets?.sources} value={f.source} onChange={(v) => onFilter('source', v)} />
        <Facet label="place" list={facets?.places} value={f.place} onChange={(v) => onFilter('place', v)} />
        <span className="num ml-auto text-[11px] text-ink-faint">{filterWords(f)}{facets?.cut ? ' · facets cut to the 100 largest' : ''}</span>
      </div>
      {q.error ? <Empty>the daemon refused the read: {(q.error as Error).message}</Empty>
        : !first ? <Empty>reading the book…</Empty>
        : episodes.length === 0 ? <Empty>{first.next ? 'none among the episodes looked at so far' : `no episode in the ${bookTitle(f.book)} matches ${filterWords(f)}`}</Empty>
        : (
          <ol className="flex flex-col" aria-label={`the ${bookTitle(f.book)}'s episodes, newest first`}>
            {episodes.map((e) => <EpisodeRow key={e.session_id} e={e} on={e.session_id === selected} onOpen={() => onOpen(e.session_id)} />)}
          </ol>
        )}
      <div className="flex items-center gap-2 border-t border-line px-3 py-2 text-[11px] text-ink-faint">
        <span className="num">{fmt(episodes.length)} shown{first ? ` of ${fmt(first.total)}` : ''}{last ? ` · page read in ${last.ms.toFixed(1)} ms` : ''}</span>
        {q.hasNextPage && (
          <button type="button" onClick={() => void q.fetchNextPage()} disabled={q.isFetchingNextPage}
            className="ml-auto rounded-md px-2 py-1 text-[11px] text-live ring-1 ring-inset ring-live/40 hover:bg-live/10 disabled:opacity-50">
            {q.isFetchingNextPage ? 'reading…' : 'older episodes'}
          </button>
        )}
      </div>
    </Panel>
  )
}

function Facet({ label, list, value, onChange }: { label: string; list?: BookFacet[]; value?: string; onChange: (v: string | null) => void }) {
  const options = facetOptions(list, value)
  return (
    <label className="flex min-w-0 items-center gap-1 text-ink-faint">
      {label}
      <select value={value ?? ''} onChange={(e) => onChange(e.target.value || null)} aria-label={`filter by ${label}`} className={cn(field, 'max-w-[16rem]')}>
        <option value="" className="bg-deck">any</option>
        {options.map((o) => <option key={o.value} value={o.value} className="bg-deck">{o.value}{o.episodes ? ` (${fmt(o.episodes)})` : ''}</option>)}
      </select>
    </label>
  )
}

function EpisodeRow({ e, on, onOpen }: { e: BookEpisode; on: boolean; onOpen: () => void }) {
  return (
    <li className="border-t border-line/50 first:border-t-0">
      <button type="button" onClick={onOpen} aria-current={on || undefined}
        className={cn('flex w-full flex-col gap-0.5 px-3 py-2 text-left hover:bg-white/[0.03]', on && 'bg-live/10')}>
        <span className="num text-[10.5px] text-ink-faint">{episodeWhen(e)} · {e.messages} messages</span>
        <span className="line-clamp-2 text-[12.5px] text-ink">{e.summary ?? e.withheld ?? '(no summary)'}</span>
        <span className="flex flex-wrap gap-1">
          {e.topics?.slice(0, 4).map((t) => <Pill key={t} tone="think">{t}</Pill>)}
          <Pill tone={guarded(e.sensitivity) ? 'wait' : 'idle'}>{e.sensitivity}</Pill>
          <Pill>{e.source}</Pill>
        </span>
      </button>
    </li>
  )
}

/** An episode opened: its summary, its labels, and its messages, from its imported session. */
function EpisodePanel({ sessionId, e, onClose }: { sessionId: string | null; e?: BookEpisode; onClose: () => void }) {
  const { data: hist, error } = useRpc<SessionHistory>('session.history', { session_id: sessionId ?? '' }, 0, { enabled: !!sessionId })
  if (!sessionId) return <Panel title="episode" icon={<BookOpen size={13} />} className="self-start" bodyClassName="h-48"><Empty>pick an episode to read its summary, labels, and messages</Empty></Panel>
  const nodes: NodeInfo[] = hist?.nodes ?? []
  const messages = nodes.filter((n) => n.kind !== 'imported_summary')
  return (
    <Panel title={<>episode · {e ? episodeWhen(e) : sessionId}</>} icon={<BookOpen size={13} />} className="self-start xl:sticky xl:top-2"
      actions={<button type="button" onClick={onClose} aria-label="close the episode" className="text-ink-faint hover:text-ink"><X size={13} /></button>}>
      <div className="flex flex-col gap-3 p-3">
        {e && (
          <>
            <p className="text-[13px] leading-relaxed text-ink">{e.summary ?? e.withheld ?? '(no summary)'}</p>
            <dl className="grid grid-cols-[7rem_minmax(0,1fr)] gap-x-2 gap-y-0.5 text-[11.5px]">
              <dt className="text-ink-faint">book</dt><dd className="text-ink-dim">{bookTitle(e.book)}</dd>
              <dt className="text-ink-faint">labels</dt><dd className="text-ink-dim">{labelWords(e).join(' · ')}</dd>
              <dt className="text-ink-faint">topics</dt><dd className="text-ink-dim">{e.topics?.join(', ') || '—'}</dd>
              <dt className="text-ink-faint">import</dt><dd className="num text-ink-dim">{e.tag} · {e.episode_id.slice(0, 15)}…</dd>
              <dt className="text-ink-faint">session</dt><dd className="num truncate text-ink-dim">{e.session_id}</dd>
            </dl>
          </>
        )}
        <div className="panel-title">messages · {messages.length}</div>
        {error ? <Empty>the daemon refused the read: {(error as Error).message}</Empty>
          : !hist ? <Empty>reading the messages…</Empty>
          : messages.length === 0 ? <Empty>no messages</Empty>
          : (
            <ol className="flex max-h-[60vh] flex-col gap-2 overflow-y-auto pr-1">
              {messages.map((n) => (
                <li key={n.node_id} className="rounded-md bg-white/[0.03] px-2.5 py-1.5 ring-1 ring-inset ring-line/60">
                  <div className="num text-[10.5px] text-ink-faint">{n.author ?? n.kind} · {new Date(n.at_unix_ms).toISOString().slice(0, 16).replace('T', ' ')} UTC</div>
                  <div className="whitespace-pre-wrap break-words text-[12.5px] text-ink">{n.text}</div>
                </li>
              ))}
            </ol>
          )}
      </div>
    </Panel>
  )
}
