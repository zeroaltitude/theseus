// Memories, search: one box over what Theseus remembers. The same words are put to three things at once and each answer
// is its own card: the people and topics that carry them, what Theseus's recall would bring to a turn for them
// (`memory.search`, which writes nothing and calls no model), and the imported conversations that hold them
// (`import.sessions`). With no words it says how much Theseus holds.
import { useState } from 'react'
import { Link, useSearchParams } from 'react-router'
import { Search as SearchIcon } from 'lucide-react'
import type { Health, ImportListResult, ImportSessionsResult, RecallItem, RecallManifest, SessionListResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { short } from '@/lib/format'
import { Empty, Panel, Pill } from '@/components/ui'
import { count, namedParams, sensitivityWords, veiled, when } from '@/lib/explorer'
import { Stat } from '../kit'
import { FOUND, episodeHere, exploreHref, excerpt, foundKinds, lineOf, listAll, pageHref, personHref, sessionHref, turnHere, wordsOf } from './find'
import { EpisodeRow, LinkRow, useCategories } from './parts'

const NONE: ReadonlySet<string> = new Set()
/** How many notes recall is asked for: enough to read, few enough to read at once. */
const NOTES = 20

export default function Search() {
  const [params, setParams] = useSearchParams()
  const q = wordsOf(params.get('q'))
  const ask = (t: string) => setParams((p) => { const w = wordsOf(t); if (w) p.set('q', w); else p.delete('q'); return p })
  return (
    <div className="flex flex-col gap-4 pb-6">
      <Panel help="simple.memories.search" title="Search what Theseus remembers" icon={<SearchIcon size={13} />}>
        <Box key={q} initial={q} submit={ask} />
        <p className="px-4 pb-3 text-[12px] text-ink-faint">
          A person’s name, a topic, a word from a conversation. Theseus looks in its own conversations and in the history that was imported, and says what it found in each.
        </p>
      </Panel>
      {q ? <Answers q={q} /> : <Holdings />}
    </div>
  )
}

function Box({ initial, submit }: { initial: string; submit: (t: string) => void }) {
  const [text, setText] = useState(initial)
  return (
    <form className="flex gap-2 p-3" onSubmit={(e) => { e.preventDefault(); submit(text) }}>
      <input value={text} onChange={(e) => setText(e.target.value)} placeholder="What do you want to find?" aria-label="words to search for" autoFocus={!initial}
        className="min-w-0 flex-1 rounded-lg bg-white/5 px-3 py-2 text-[14px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/40" />
      <button type="submit" className="brass-button">Search</button>
    </form>
  )
}

/** What Theseus holds, in five numbers: where a search is looking. */
function Holdings() {
  const { data: h } = useRpc<Health>('health', undefined, 0)
  const { data: tags } = useRpc<ImportListResult>('import.list', undefined, 0)
  const { data: onto } = useCategories()
  const cats = onto?.categories ?? []
  const episodes = (tags?.tags ?? []).reduce((a, t) => a + t.sessions - t.erased, 0)
  return (
    <Panel help="simple.memories.holdings" title="What Theseus holds">
      <div className="grid grid-cols-2 gap-2 p-3 sm:grid-cols-3 lg:grid-cols-5">
        <Stat n={count(h?.sessions ?? 0)} label="conversations with Theseus" title={`${count(h?.turns ?? 0)} turns in all`} />
        <Stat n={count(episodes)} label="imported conversations" />
        <Stat n={count(cats.filter((c) => c.kind === 'person').length)} label="people" />
        <Stat n={count(cats.filter((c) => c.kind === 'topic').length)} label="topics" />
        <Stat n={count(h?.index?.status?.nodes ?? 0)} label="notes in the index" title={h?.index ? `the index is ${h.index.state}` : undefined} />
      </div>
    </Panel>
  )
}

function Answers({ q }: { q: string }) {
  return (
    <>
      <Kinds q={q} />
      <Recalled q={q} />
      <Episodes q={q} />
    </>
  )
}

/** The people and topics that carry the words. */
function Kinds({ q }: { q: string }) {
  const { data } = useCategories()
  const cats = data?.categories ?? []
  const f = foundKinds(cats, q)
  if (data && !f.people.length && !f.topics.length) return null
  return (
    <Panel help="simple.memories.kinds" title="People and topics">
      {!data ? <Empty>reading…</Empty> : (
        <div className="grid grid-cols-1 gap-x-4 p-2 md:grid-cols-2">
          <div>
            {f.people.length > 0 && <div className="simple-eyebrow px-3 pb-1 pt-1">People</div>}
            {f.people.map((c) => <LinkRow key={c.id} to={personHref(c.id)} title={c.name} sub={lineOf(cats, c)} />)}
            {f.morePeople > 0 && <More n={f.morePeople} to={listAll('person', q)} what="people" />}
          </div>
          <div>
            {f.topics.length > 0 && <div className="simple-eyebrow px-3 pb-1 pt-1">Topics</div>}
            {f.topics.map((c) => <LinkRow key={c.id} to={pageHref(c)} title={c.name} sub={lineOf(cats, c)} />)}
            {f.moreTopics > 0 && <More n={f.moreTopics} to={listAll('topic', q)} what="topics" />}
          </div>
        </div>
      )}
    </Panel>
  )
}

function More({ n, to, what }: { n: number; to: string; what: string }) {
  return <Link to={to} className="block px-3 py-1.5 text-[12px] text-live hover:underline">{n} more {what} →</Link>
}

/** What recall would bring a turn for the words, each note with where it came from. */
function Recalled({ q }: { q: string }) {
  const { data, error, isFetching } = useRpc<RecallManifest>('memory.search', { query: q, k: NOTES }, 0)
  const hits = data?.admitted.filter((i) => i.text) ?? []
  const imported = [...new Set(hits.map((i) => i.session_id).filter((s) => s.startsWith('ses_ep')))]
  const { data: rows } = useRpc<ImportSessionsResult>('import.sessions', namedParams(imported), 0, { enabled: imported.length > 0 })
  const { data: ses } = useRpc<SessionListResult>('session.list', { n: 200 }, 0, { enabled: hits.some((i) => !i.session_id.startsWith('ses_ep')) })
  return (
    <Panel help="simple.memories.recalled" title={data ? `What Theseus would remember · ${hits.length}` : 'What Theseus would remember'}>
      {error ? <div className="p-4 text-[12.5px] text-fault">Theseus could not look this up: {(error as Error).message}</div>
        : !data ? <Empty>{isFetching ? 'looking…' : '—'}</Empty>
        : data.outcome !== 'ran' ? <div className="p-4 text-[12.5px] text-wait">Theseus’s index did not answer ({data.outcome}){data.why ? `: ${data.why}` : ''}. The conversations below were still searched.</div>
        : !hits.length ? <Empty>Nothing in Theseus’s memory answers these words.</Empty>
        : (
          <ul className="flex flex-col gap-0.5 p-2" aria-label="remembered notes">
            {hits.map((i) => <Note key={`${i.node_id}:${i.chunk}`} i={i} episode={rows?.episodes.find((e) => e.session_id === i.session_id)} title={ses?.sessions.find((s) => s.session_id === i.session_id)?.title} />)}
          </ul>
        )}
    </Panel>
  )
}

function Note({ i, episode, title }: { i: RecallItem; episode?: ImportSessionsResult['episodes'][number]; title?: string }) {
  const isEp = i.session_id.startsWith('ses_ep')
  // An imported note whose labels are not read yet is held back the way a personal one is: its words never show before its label does.
  const hidden = isEp && (!episode || veiled(episode, true, NONE))
  const s = episode ? sensitivityWords(episode.sensitivity) : null
  return (
    <li className="rounded-lg px-3 py-2 hover:bg-white/[0.04]">
      <div className="flex flex-wrap items-center gap-x-2 gap-y-1 text-[12px] text-ink-faint">
        <span className="text-ink-dim">{isEp ? `imported · ${episode?.place_name ?? episode?.place_kind ?? 'an episode'}` : title ?? `session ${short(i.session_id)}`}</span>
        {episode?.end_ms ? <span className="num">{when(episode.end_ms).slice(0, 10)}</span> : null}
        {s && <Pill tone={s.tone}>{s.word}</Pill>}
      </div>
      {hidden ? <p className="mt-1 text-[12.5px] italic text-ink-faint">{s ? `${s.word} · its words wait until you open it` : 'reading its label…'}</p>
        : <p className="mt-1 whitespace-pre-wrap text-[13px] leading-relaxed text-ink">{excerpt(i.text ?? '')}</p>}
      <div className="mt-1 flex gap-3 text-[12px]">
        {isEp ? <Link to={episodeHere(i.session_id)} className="text-live hover:underline">Open the conversation</Link>
          : <>
            <Link to={sessionHref(i.session_id)} className="text-live hover:underline">Open the session</Link>
            <Link to={turnHere(i.session_id)} className="text-live hover:underline">What its newest turn was shown</Link>
          </>}
      </div>
    </li>
  )
}

/** The imported conversations whose title, place or topics hold the words. */
function Episodes({ q }: { q: string }) {
  const { data, error } = useRpc<ImportSessionsResult>('import.sessions', { q, limit: FOUND, summaries: true }, 0)
  if (data && data.all === 0) return null
  return (
    <Panel help="simple.memories.episodes" title={data ? `Imported conversations · ${count(data.total)}` : 'Imported conversations'}
      actions={data && data.total > data.episodes.length ? <Link to={exploreHref(q)} className="text-[12px] text-live hover:underline">See all {count(data.total)} →</Link> : undefined}>
      {error ? <div className="p-4 text-[12.5px] text-fault">Theseus could not read these: {(error as Error).message}</div>
        : !data ? <Empty>reading…</Empty>
        : !data.episodes.length ? <Empty>No imported conversation has these words in its title, place or topics.</Empty>
        : (
          <div className="flex flex-col gap-0.5 p-2">
            {data.withheld && <div className="px-3 pb-1 text-[12px] text-wait">{data.withheld}</div>}
            {data.episodes.map((e) => <EpisodeRow key={e.session_id} e={e} words />)}
          </div>
        )}
    </Panel>
  )
}
