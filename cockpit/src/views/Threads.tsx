// The Threads on the work board (theseus-5i8i): each live thread as a group (its title, a lock when sealed, state,
// persona, last activity, context size, today's dollars, warm or cold), its open tasks, questions and wakes under it,
// and a seal waiting on the owner; and a thread's own view (`/threads/:slug`): its card, working set and history links,
// its recent turns, and its tasks. The groups come from one `work.watch` (`lib/threadwatch.ts`), the record from
// `thread.show`; the grouping is `lib/threadboard.ts`, the same rules as the CLI's and the TUI's. Under the time machine
// the list is the threads as the ledger's `thread.*` rows left them: the present's work is not folded, and it says so.
import { useMemo } from 'react'
import { Link, useParams } from 'react-router'
import { GitBranch, Lock, Snowflake, Sun } from 'lucide-react'
import { useTick } from '@/lib/hooks'
import { useAsOf } from '@/lib/timemachine'
import { useHistoryRows } from '@/lib/history'
import { useThreadBoard } from '@/lib/threadwatch'
import { boardNeeds, details, foldThreads, groupThreads, isWarm, totalNeeds, type GroupedThread } from '@/lib/threadboard'
import { Empty, Panel, Pill } from '@/components/ui'
import { Item, ThreadBody } from '@/ops/ThreadPanel'

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

/** One thread: its record, its work, and its recent turns (`ops/ThreadPanel.tsx`, which the Operations page's Threads
 *  lens shows too). */
export function ThreadView() {
  const { slug = '' } = useParams()
  return <div className="p-4"><ThreadBody slug={slug} /></div>
}
