// The Threads on the work board (theseus-5i8i): each live thread as a group (its title, a lock when sealed, state,
// persona, last activity, context size, today's dollars, warm or cold), its open tasks, questions and wakes under it,
// and a seal waiting on the owner; and a thread's own view (`/threads/:slug`): its card, working set and history links,
// its recent turns, and its tasks. The groups come from one `work.watch` (`lib/threadwatch.ts`), the record from
// `thread.show`; the grouping is `lib/threadboard.ts`, the same rules as the CLI's and the TUI's. Under the time machine
// the list is the threads as the ledger's `thread.*` rows left them: the present's work is not folded, and it says so.
import { useMemo } from 'react'
import { Link, useParams } from 'react-router'
import { GitBranch, Lock, MessagesSquare, Snowflake, Sun } from 'lucide-react'
import type { NodeInfo, SessionHistoryResult, ThreadRow, WorkView } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { useAsOf } from '@/lib/timemachine'
import { useHistoryRows } from '@/lib/history'
import { useThreadBoard } from '@/lib/threadwatch'
import {
  boardNeeds, details, foldThreads, groupThreads, historyLinks, isWarm, totalNeeds, workingSetLines, type GroupedThread,
} from '@/lib/threadboard'
import { ago } from '@/lib/format'
import { AttentionPill, Empty, Panel, Pill } from '@/components/ui'

/** One open task, question or wake: its attention, its title, and the session it lives in. */
function Item({ v }: { v: WorkView }) {
  const body = (
    <span className="flex min-w-0 items-center gap-2 text-[12px]">
      <AttentionPill a={v.attention} state={v.state} />
      <span className="truncate text-ink">{v.title || v.attention.label}</span>
      <span className="shrink-0 text-ink-faint">{v.kind}</span>
    </span>
  )
  return v.session_id ? (
    <Link to={`/session/${v.session_id}`} className="block rounded px-1 py-0.5 hover:bg-white/5">{body}</Link>
  ) : (
    <div className="px-1 py-0.5">{body}</div>
  )
}

function Warmth({ t, now }: { t: GroupedThread; now: number }) {
  if (t.lastActiveMs === 0) return null
  const warm = isWarm(t.lastActiveMs, now)
  return (
    <Pill tone={warm ? 'ok' : 'idle'} title={warm ? 'Its cache is warm: a message costs little.' : 'Its cache is cold: a message rewrites its context.'}>
      {warm ? <Sun size={11} /> : <Snowflake size={11} />}{warm ? 'warm' : 'cold'}
    </Pill>
  )
}

/** A thread's group: the header, its seal waiting, and its open work. */
function Group({ t, now }: { t: GroupedThread; now: number }) {
  const g = t.group
  const rest = details(t, now).filter((d) => d !== g.state && d !== 'warm' && d !== 'cold')
  return (
    <div className="rounded-lg border border-line bg-black/20 p-3" data-thread={g.slug}>
      <div className="flex flex-wrap items-center gap-2">
        {g.sealed && <Lock size={13} className="text-ink-faint" aria-label="sealed" />}
        <Link to={`/threads/${g.slug}`} className="text-[14px] font-semibold text-ink hover:text-live">{g.title}</Link>
        <Pill tone={g.state === 'active' ? 'live' : 'idle'}>{g.state}</Pill>
        <Warmth t={t} now={now} />
        {t.needsYou > 0 && <Pill tone={boardNeeds(t) > 0 ? 'fault' : 'idle'} title={g.state === 'paused' ? 'Paused: not in the board’s count' : undefined}>● {t.needsYou} need{t.needsYou === 1 ? 's' : ''} you</Pill>}
        <span className="num ml-auto text-[11px] text-ink-faint">{rest.join(' · ')}</span>
      </div>
      {g.seal_pending && (
        <div className="mt-2 rounded-md bg-fault/10 px-2 py-1 text-[12px] text-ink ring-1 ring-fault/30">
          ● Seal recommended: {g.seal_pending} <span className="text-ink-faint">— the owner seals it</span>
        </div>
      )}
      <div className="mt-2 space-y-0.5">
        {t.items.length === 0 ? <div className="px-1 text-[12px] text-ink-faint">Nothing open.</div> : t.items.map((v) => <Item key={v.id} v={v} />)}
      </div>
    </div>
  )
}

/** The list: each live thread, needs-you first. */
export default function Threads() {
  const now = useTick(15_000)
  const asOf = useAsOf((s) => s.t)
  const { board, ready, error } = useThreadBoard()
  const history = useHistoryRows(asOf === null)
  const grouped = useMemo(() => groupThreads(Object.values(board.views).sort((a, b) => a.state_since_ms - b.state_since_ms), Object.values(board.groups)), [board])
  const then = useMemo(() => (asOf === null ? null : foldThreads(history.rows, asOf)), [asOf, history.rows])

  if (then) {
    return (
      <div className="space-y-3 p-4">
        <Panel title="Threads then" icon={<GitBranch size={14} />} help="threads.list">
          <div className="space-y-1 p-3">
            <p className="text-[12px] text-ink-faint">As the ledger’s thread rows left them at this moment. The work under each thread is the present’s and is not folded.</p>
            {then.length === 0 ? <Empty>No thread then.</Empty> : then.map((t) => (
              <div key={t.slug} className="flex items-center gap-2 text-[13px]">
                <Link to={`/threads/${t.slug}`} className="text-ink hover:text-live">{t.title}</Link>
                <Pill tone={t.state === 'active' ? 'live' : 'idle'}>{t.state}</Pill>
              </div>
            ))}
          </div>
        </Panel>
      </div>
    )
  }
  return (
    <div className="space-y-3 p-4">
      <Panel
        title="Threads"
        icon={<GitBranch size={14} />}
        help="threads.list"
        actions={<span className="num text-[11px] text-ink-faint">{grouped.threads.length} live · {totalNeeds(grouped)} need you on the board</span>}
      >
        <div className="space-y-2 p-3">
          {error && <p className="text-[12px] text-fault">The board could not be read: {error}</p>}
          {!ready ? <Empty>Reading the board…</Empty> : grouped.threads.length === 0 ? (
            <Empty>No thread is live. <code className="mx-1">theseus thread new</code> makes one.</Empty>
          ) : grouped.threads.map((t) => <Group key={t.group.slug} t={t} now={now} />)}
          {ready && grouped.loose.length > 0 && (
            <p className="px-1 text-[12px] text-ink-faint">{grouped.loose.filter((v) => v.parent == null).length} conversations are in no thread: <Link to="/fleet" className="text-live">Fleet</Link>.</p>
          )}
        </div>
      </Panel>
    </div>
  )
}

/** The last turns of a thread's session: what was said, newest last. */
function Turns({ session }: { session: string }) {
  const { data } = useRpc<SessionHistoryResult>('session.history', { session_id: session, n: 12 }, 5000)
  const said = (data?.nodes ?? []).filter((n: NodeInfo) => (n.kind === 'user_message' || n.kind === 'assistant_message') && n.text.trim() !== '')
  if (said.length === 0) return <Empty>No messages yet.</Empty>
  return (
    <ol className="space-y-2 p-3 text-[12px]">
      {said.slice(-8).map((n) => (
        <li key={n.node_id}>
          <div className="text-ink-faint">{n.kind === 'user_message' ? 'You' : 'Theseus'} · {ago(n.at_unix_ms)}</div>
          <div className="line-clamp-4 whitespace-pre-wrap text-ink">{n.text}</div>
        </li>
      ))}
    </ol>
  )
}

/** One thread: its record, its work, and its recent turns. */
export function ThreadView() {
  const { slug = '' } = useParams()
  const now = useTick(15_000)
  const { board } = useThreadBoard()
  const { data: row, error } = useRpc<ThreadRow>('thread.show', { slug }, 5000)
  const grouped = useMemo(() => groupThreads(Object.values(board.views), Object.values(board.groups)), [board])
  const mine = grouped.threads.find((t) => t.group.slug === slug)
  const t = row?.thread
  if (error) return <div className="p-4"><Panel title="Thread" help={false}><Empty>{String((error as { message?: string }).message ?? error)}</Empty></Panel></div>
  if (!t) return <div className="p-4"><Empty>Reading the thread…</Empty></div>
  const links = historyLinks(t)
  const ws = workingSetLines(t.working_set ?? {})
  return (
    <div className="grid gap-3 p-4 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]">
      <div className="space-y-3">
        <Panel title={t.sealed ? `🔒 ${t.title}` : t.title} icon={<GitBranch size={14} />} help="threads.thread"
          actions={<Pill tone={t.state === 'active' ? 'live' : 'idle'}>{t.state}</Pill>}>
          <div className="space-y-2 p-3 text-[12px]">
            {t.goal && <p className="text-ink">{t.goal}</p>}
            {mine && <p className="num text-ink-faint">{details(mine, now).join(' · ')}</p>}
            {row && <p className="num text-ink-faint">{row.context_tokens != null ? `${row.context_tokens.toLocaleString()} tokens in context · ` : ''}${row.today_usd.toFixed(2)} today</p>}
            {t.seal_pending && <p className="text-fault">● Seal recommended: {t.seal_pending}</p>}
            {t.sealed && <p className="text-ink-faint">Sealed by {t.sealed.by}: {t.sealed.reason}</p>}
            {(t.topics?.length ?? 0) > 0 && <p className="text-ink-faint">Topics: {t.topics!.join(', ')}</p>}
            {links.length > 0 && (
              <p className="text-ink-faint">{links.map((l, i) => (
                <span key={`${l.label}:${l.slug}`}>{i > 0 && ' · '}{l.label} <Link to={`/threads/${l.slug}`} className="text-live">{l.slug}</Link></span>
              ))}</p>
            )}
          </div>
        </Panel>
        <Panel title="Card" help="threads.card">
          {(t.card?.length ?? 0) === 0 ? <Empty>No card yet.</Empty> : (
            <ul className="list-disc space-y-0.5 p-3 pl-6 text-[12px] text-ink">{t.card!.map((l, i) => <li key={i}>{l}</li>)}</ul>
          )}
        </Panel>
        <Panel title="Working set" help="threads.workingset">
          {ws.length === 0 ? <Empty>Nothing yet.</Empty> : <ul className="space-y-0.5 p-3 text-[12px] text-ink">{ws.map((l) => <li key={l}>{l}</li>)}</ul>}
        </Panel>
      </div>
      <div className="space-y-3">
        <Panel title="Open work" icon={<GitBranch size={14} />} help="threads.work">
          <div className="space-y-0.5 p-3">
            {!mine || mine.items.length === 0 ? <div className="text-[12px] text-ink-faint">Nothing open.</div> : mine.items.map((v) => <Item key={v.id} v={v} />)}
          </div>
        </Panel>
        <Panel title="Recent turns" icon={<MessagesSquare size={14} />} help="threads.turns">
          {t.session_id ? <Turns session={t.session_id} /> : <Empty>The first message opens its session.</Empty>}
        </Panel>
      </div>
    </div>
  )
}
