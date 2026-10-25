// The Threads lens (theseus-5i8i; theseus-jid7t.3): the Threads page's list, moved whole. Each live thread as a group
// (a lock when sealed, its title, state, warm or cold, what needs you, its details, a seal waiting as a red line, and
// its open items), drawn from `threadFacts`; the title "N live · N need you on the board"; the conversations in no
// thread; and, when no thread is live, the empty state's words and a New thread button. A click selects the thread,
// whose panel the inspector shows. Under the time machine the list is the threads as the ledger's rows left them.
import { useMemo, useState } from 'react'
import { Link } from 'react-router'
import { ExternalLink, GitBranch, Lock, Plus, Snowflake, Sun } from 'lucide-react'
import type { TaskRecord, WorkView } from '@protocol'
import { useHistoryRows } from '@/lib/history'
import { foldThreads, groupThreads, threadWork, totalNeeds, type GroupedThread, type ThreadWork, type WorkLine } from '@/lib/threadboard'
import { boardRows, type OpsRow } from './board'
import { Item } from './ThreadPanel'
import { cn } from '@/lib/format'
import { Empty, Panel, Pill } from '@/components/ui'
import { threadFacts } from './facts'
import { useOpsAct } from './act'
import { ActBtn } from './ActBtn'
import { NewThreadSheet } from './ThreadSheets'

export function ThreadsLens({ views, groups, records, ready, error, now, past, focus, onFocus, matches, rows, kept, context, onSelect }: {
  views: readonly WorkView[]; groups: Parameters<typeof groupThreads>[1]; records?: Parameters<typeof groupThreads>[2]; ready: boolean; error: string | null; now: number; past: number | null
  focus: string | null; onFocus: (slug: string) => void; matches: (t: GroupedThread) => boolean
  /** The page's one work set (`boardRows`), the ids a counter's filter keeps and their context (`contextOf`). The page
   *  passes all three; without them the lens reads a work set of the views and records alone and filters nothing. */
  rows?: readonly OpsRow[]; kept?: ReadonlySet<string> | null; context?: ReadonlySet<string> | null; onSelect?: (id: string) => void
}) {
  const history = useHistoryRows(past === null)
  const grouped = useMemo(() => groupThreads([...views].sort((a, b) => a.state_since_ms - b.state_since_ms), groups, records), [views, groups, records])
  const set = useMemo(() => rows ?? boardRows({ views: [...views], tasks: [], sessions: [], records: (records ?? []) as TaskRecord[], now, past: false }), [rows, views, records, now])
  const work = useMemo(() => threadWork(set, grouped.threads.map((t) => t.group), { kept, context }), [set, grouped.threads, kept, context])
  const then = useMemo(() => (past === null ? null : foldThreads(history.rows, past)), [past, history.rows])
  if (then) {
    return (
      <Panel title="Threads then" icon={<GitBranch size={14} />} help="threads.list">
        <div className="space-y-1 p-3">
          <p className="text-[12px] text-ink-faint">As the ledger’s thread rows left them at this moment. The work under each thread is the present’s and is not folded.</p>
          {then.length === 0 ? <Empty>No thread then.</Empty> : then.map((t) => (
            <button key={t.slug} onClick={() => onFocus(t.slug)} className={cn('flex w-full items-center gap-2 rounded px-1 text-left text-[13px] hover:bg-white/5', focus === t.slug && 'ring-1 ring-gold')}>
              <span className="text-ink">{t.title}</span>
              <Pill tone={t.state === 'active' ? 'live' : 'idle'}>{t.state}</Pill>
            </button>
          ))}
        </div>
      </Panel>
    )
  }
  const bySlug = new Map(grouped.threads.map((t) => [t.group.slug, t]))
  const shown = work.filter((w) => (w.group ? bySlug.has(w.group.slug) && matches(bySlug.get(w.group.slug)!) : true))
  return (
    <Panel title="Threads" icon={<GitBranch size={14} />} help="threads.list" className="h-full"
      bodyClassName="overflow-y-auto"
      actions={<span className="num text-[11px] text-ink-faint">{grouped.threads.length} live · {totalNeeds(grouped)} need you on the board{shown.filter((w) => w.group).length !== grouped.threads.length ? ` · ${shown.filter((w) => w.group).length} of ${grouped.threads.length} shown` : ''}</span>}>
      <div className="space-y-2 p-3" data-testid="ops-threads">
        {error && <p className="text-[12px] text-fault">The board could not be read: {error}</p>}
        {!ready ? <Empty>Reading the board…</Empty> : grouped.threads.length === 0 ? <NoThreads /> : shown.map((w) => (w.group
          ? <Group key={w.group.slug} t={bySlug.get(w.group.slug)!} w={w} now={now} focused={focus === w.group.slug} onFocus={onFocus} onSelect={onSelect} />
          : <Home key="home" w={w} onSelect={onSelect} />))}
        {ready && grouped.loose.length > 0 && (
          <p className="px-1 text-[12px] text-ink-faint">{grouped.loose.filter((v) => v.parent == null).length} conversations are in no thread: <Link to="/fleet" className="text-live">Fleet</Link>.</p>
        )}
      </div>
    </Panel>
  )
}

/** No thread is live: the Threads page's words, and the button that makes one (`thread.new`). */
function NoThreads() {
  const { busy, on } = useOpsAct()
  const [sheet, setSheet] = useState(false)
  return (
    <div className="flex flex-col items-center gap-3 py-8">
      <Empty>No thread is live. <code className="mx-1">theseus thread new</code> makes one.</Empty>
      <ActBtn verb="thread_new" on={on} busy={busy === 'thread.new'} onClick={() => setSheet(true)} title="thread.new: a title and a slug"><Plus size={12} /> New thread</ActBtn>
      {sheet && <NewThreadSheet onClose={() => setSheet(false)} />}
    </div>
  )
}

/** A thread's group: the header, its seal waiting, and its open work. */
function Group({ t, w, now, focused, onFocus, onSelect }: { t: GroupedThread; w: ThreadWork; now: number; focused: boolean; onFocus: (slug: string) => void; onSelect?: (id: string) => void }) {
  const f = threadFacts(t, now)
  return (
    <div role="button" tabIndex={-1} aria-selected={focused} onClick={() => onFocus(t.group.slug)} data-thread={t.group.slug}
      className={cn('cursor-pointer rounded-lg border border-line bg-black/20 p-3 transition-shadow hover:border-line-strong',
        t.needsYou > 0 && 'shadow-[0_0_14px_-6px_var(--color-wait)]', focused && 'border-gold shadow-[0_0_16px_-2px_rgb(214_165_72_/_0.55)]')}>
      <div className="flex flex-wrap items-center gap-2">
        {f.sealed && <Lock size={13} className="text-ink-faint" aria-label="sealed" />}
        <span className="font-display text-[14px] font-bold tracking-[0.02em] text-ink">{f.title}</span>
        <Link to={f.href} onClick={(e) => e.stopPropagation()} title="the thread’s own page" className="text-ink-faint hover:text-live"><ExternalLink size={12} /></Link>
        <Pill tone={f.state === 'active' ? 'live' : 'idle'}>{f.state}</Pill>
        {f.warmth && <Pill tone={f.warmth === 'warm' ? 'ok' : 'idle'} title={f.warmthTip}>{f.warmth === 'warm' ? <Sun size={11} /> : <Snowflake size={11} />}{f.warmth}</Pill>}
        {f.needs && <Pill tone={f.needsCounted ? 'fault' : 'idle'} title={f.pausedTip}>{f.needs}</Pill>}
        <span className="num ml-auto text-[11px] text-ink-faint">{f.details}</span>
      </div>
      {f.seal && (
        <div className="mt-2 rounded-md bg-fault/10 px-2 py-1 text-[12px] text-ink ring-1 ring-fault/30">
          {f.seal} <span className="text-ink-faint">— the owner seals it (in the queue)</span>
        </div>
      )}
      <div className="mt-2 space-y-0.5" onClick={(e) => e.stopPropagation()}>
        {/* A thread the work set lists nothing under keeps the old page's open items (ops-parity pins them to this lens). */}
        {w.lines.length === 0 && f.items.length > 0 ? f.items.map((it, i) => <Item key={it.id} v={t.items[i]} />) : <Lines w={w} onSelect={onSelect} />}
      </div>
    </div>
  )
}

/** Home: the work that belongs to no thread. */
function Home({ w, onSelect }: { w: ThreadWork; onSelect?: (id: string) => void }) {
  return (
    <div className="rounded-lg border border-line bg-black/10 p-3" data-thread="home">
      <div className="flex items-center gap-2"><span className="font-display text-[14px] font-bold text-ink">Home</span><span className="text-[11px] text-ink-faint">in no thread</span></div>
      <div className="mt-2 space-y-0.5"><Lines w={w} onSelect={onSelect} /></div>
    </div>
  )
}

/** A thread's cards: plans and steps nested, a card's task on its own line, a plan's n of m and its open children. */
function Lines({ w, onSelect }: { w: ThreadWork; onSelect?: (id: string) => void }) {
  if (w.lines.length === 0) return <div className="px-1 text-[12px] text-ink-faint">Nothing open.</div>
  return <>{w.lines.map((l) => <Line key={l.row.id} l={l} onSelect={onSelect} />)}</>
}

function Line({ l, onSelect }: { l: WorkLine; onSelect?: (id: string) => void }) {
  const r = l.row
  return (
    <div role="button" tabIndex={-1} data-card={r.id} data-bucket={r.bucket} onClick={() => onSelect?.(r.id)} style={{ paddingLeft: l.depth * 14 }}
      className={cn('flex flex-wrap items-center gap-2 rounded px-1 text-[13px] hover:bg-white/5', l.context && 'opacity-50')}>
      <span className="text-ink-faint">{r.plan ? '▾' : r.kind === 'step' ? '☐' : '·'}</span>
      <span className="text-ink">{r.title}</span>
      {l.task && <Pill tone="idle" title="the task that carries it">task</Pill>}
      {l.needs > 0 && <Pill tone="fault">{l.needs === 1 ? 'needs you' : `${l.needs} need you`}</Pill>}
      <Pill tone={r.bucket === 'working' ? 'live' : 'idle'}>{r.bucket === 'todo' ? 'to do' : r.bucket}</Pill>
      {l.progress && <span className="num text-[11px] text-ink-faint">{l.progress.done} of {l.progress.total} done · {l.progress.words}</span>}
      {l.carriedBy && <span className="text-[11px] text-ink-faint">{l.carriedBy}</span>}
    </div>
  )
}
