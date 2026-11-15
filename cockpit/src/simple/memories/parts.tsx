// The small pieces the Memories tabs share: the ontology's read, a row that is a link, and a list of conversations
// (imported episodes and Theseus's own sessions) read by their ids. Nothing here keeps state of its own: a read is
// the Ship's own read, so what the page says is what the explorer says.
import { useMemo, type ReactNode } from 'react'
import { Link } from 'react-router'
import type { ImportSessionsResult, ImportedEpisode, OntologyListResult, SessionListResult } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { short } from '@/lib/format'
import { Empty, Pill } from '@/components/ui'
import { namedParams, sensitivityWords, veiled, when } from '@/lib/explorer'
import { SHOWN, episodeHere, sessionHref } from './find'

const NONE: ReadonlySet<string> = new Set()

/** Every category (the people and the topics) without its memberships: one light read, shared by every tab. */
export function useCategories() {
  return useRpc<OntologyListResult>('ontology.list', { memberships: false }, 0)
}

/** A row that is a link: what it is, one line under it, and something at its right. */
export function LinkRow({ to, current, title, sub, aside }: { to: string; current?: boolean; title: ReactNode; sub?: ReactNode; aside?: ReactNode }) {
  return (
    <Link to={to} aria-current={current ? 'true' : undefined} className="simple-row">
      <span className="min-w-0 flex-1">
        <span className="simple-row-title block truncate">{title}</span>
        {sub ? <span className="simple-row-sub block truncate">{sub}</span> : null}
      </span>
      {aside}
    </Link>
  )
}

/** An imported episode as a row. A personal or partner-confidential one shows its label and not its words, as the
 *  explorer does: the words wait behind opening it. */
export function EpisodeRow({ e, words }: { e: ImportedEpisode; words?: boolean }) {
  const s = sensitivityWords(e.sensitivity)
  const hidden = veiled(e, true, NONE)
  const place = e.place_name ? `${e.place_kind} · ${e.place_name}` : e.place_kind
  const said = words ? e.summary_text ?? e.title : e.title
  return (
    <LinkRow to={episodeHere(e.session_id)}
      title={hidden ? <span className="italic text-ink-faint">{s.word} · its words wait until you open it</span> : (said ?? <span className="text-ink-faint">no summary</span>)}
      sub={`${e.end_ms ? when(e.end_ms).slice(0, 10) : 'undated'} · ${place} · ${e.messages} ${e.messages === 1 ? 'message' : 'messages'}`}
      aside={<Pill tone={s.tone}>{s.word}</Pill>} />
  )
}

/** A conversation list for the ids a person's or a topic's page holds: the newest few of each kind, read by their ids,
 *  and what is left said in a line. */
export function Conversations({ imported, native, total }: { imported: string[]; native: string[]; total: number }) {
  const shownImported = imported.slice(0, SHOWN)
  const shownNative = native.slice(0, SHOWN)
  const { data: eps, error } = useRpc<ImportSessionsResult>('import.sessions', namedParams(shownImported), 0, { enabled: shownImported.length > 0 })
  const { data: ses } = useRpc<SessionListResult>('session.list', { n: 200 }, 0, { enabled: shownNative.length > 0 })
  const byId = useMemo(() => new Map((eps?.episodes ?? []).map((e) => [e.session_id, e])), [eps])
  const own = useMemo(() => new Map((ses?.sessions ?? []).map((s) => [s.session_id, s])), [ses])
  if (!total) return <Empty>No conversation is filed here yet.</Empty>
  const left = total - shownImported.length - shownNative.length
  return (
    <div className="flex flex-col gap-0.5 p-2">
      {shownNative.length > 0 && <div className="simple-eyebrow px-3 pb-1 pt-1">Conversations with Theseus</div>}
      {shownNative.map((id) => {
        const s = own.get(id)
        return <LinkRow key={id} to={sessionHref(id)} title={s?.title ?? s?.label ?? `session ${short(id)}`} sub={s ? `${s.turns} ${s.turns === 1 ? 'turn' : 'turns'}` : 'not among the newest sessions'} />
      })}
      {shownImported.length > 0 && <div className="simple-eyebrow px-3 pb-1 pt-2">From imported history</div>}
      {error ? <div className="px-3 py-2 text-[12px] text-fault">Theseus could not read these: {(error as Error).message}</div>
        : shownImported.map((id) => {
          const e = byId.get(id)
          return e ? <EpisodeRow key={id} e={e} /> : <LinkRow key={id} to={episodeHere(id)} title={eps ? 'an episode that is no longer held' : 'reading…'} sub={short(id)} />
        })}
      {left > 0 && <div className="px-3 py-2 text-[12px] text-ink-faint">and {left} more, older than these</div>}
    </div>
  )
}
