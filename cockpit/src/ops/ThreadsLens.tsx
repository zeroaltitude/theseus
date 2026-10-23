// The Threads lens (theseus-5i8i; theseus-jid7t.3): the Threads page's list, moved whole. Each live thread as a group
// (a lock when sealed, its title, state, warm or cold, what needs you, its details, a seal waiting as a red line, and
// its open items), drawn from `threadFacts`; the title "N live · N need you on the board"; the conversations in no
// thread; and, when no thread is live, the empty state's words and a New thread button. A click selects the thread,
// whose panel the inspector shows. Under the time machine the list is the threads as the ledger's rows left them.
import { useMemo } from 'react'
import { Link } from 'react-router'
import { ExternalLink, GitBranch, Lock, Plus, Snowflake, Sun } from 'lucide-react'
import type { WorkView } from '@protocol'
import { useHistoryRows } from '@/lib/history'
import { foldThreads, groupThreads, totalNeeds, type GroupedThread } from '@/lib/threadboard'
import { cn } from '@/lib/format'
import { Empty, Panel, Pill } from '@/components/ui'
import { threadFacts } from './facts'
import { useOpsAct } from './act'
import { ActBtn } from './ActBtn'
import { Item } from './ThreadPanel'

export function ThreadsLens({ views, groups, ready, error, now, past, focus, onFocus, matches }: {
  views: readonly WorkView[]; groups: Parameters<typeof groupThreads>[1]; ready: boolean; error: string | null; now: number; past: number | null
  focus: string | null; onFocus: (slug: string) => void; matches: (t: GroupedThread) => boolean
}) {
  const history = useHistoryRows(past === null)
  const grouped = useMemo(() => groupThreads([...views].sort((a, b) => a.state_since_ms - b.state_since_ms), groups), [views, groups])
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
  const shown = grouped.threads.filter(matches)
  return (
    <Panel title="Threads" icon={<GitBranch size={14} />} help="threads.list" className="h-full"
      bodyClassName="overflow-y-auto"
      actions={<span className="num text-[11px] text-ink-faint">{grouped.threads.length} live · {totalNeeds(grouped)} need you on the board{shown.length !== grouped.threads.length ? ` · ${shown.length} of ${grouped.threads.length} shown` : ''}</span>}>
      <div className="space-y-2 p-3" data-testid="ops-threads">
        {error && <p className="text-[12px] text-fault">The board could not be read: {error}</p>}
        {!ready ? <Empty>Reading the board…</Empty> : grouped.threads.length === 0 ? <NoThreads /> : shown.map((t) => (
          <Group key={t.group.slug} t={t} now={now} focused={focus === t.group.slug} onFocus={onFocus} />
        ))}
        {ready && grouped.loose.length > 0 && (
          <p className="px-1 text-[12px] text-ink-faint">{grouped.loose.filter((v) => v.parent == null).length} conversations are in no thread: <Link to="/fleet" className="text-live">Fleet</Link>.</p>
        )}
      </div>
    </Panel>
  )
}

/** No thread is live: the Threads page's words, and the button that makes one (`thread.new`). */
function NoThreads() {
  const { busy, act, on } = useOpsAct()
  const make = () => {
    const title = window.prompt('A new thread: its title')
    if (!title?.trim()) return
    const slug = window.prompt('Its slug (letters, digits and dashes)', title.trim().toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, ''))
    if (!slug?.trim()) return
    void act('thread_new', 'thread.new', 'thread.new', { slug: slug.trim(), title: title.trim() })
  }
  return (
    <div className="flex flex-col items-center gap-3 py-8">
      <Empty>No thread is live. <code className="mx-1">theseus thread new</code> makes one.</Empty>
      <ActBtn verb="thread_new" on={on} busy={busy === 'thread.new'} onClick={make} title="thread.new: a title and a slug"><Plus size={12} /> New thread</ActBtn>
    </div>
  )
}

/** A thread's group: the header, its seal waiting, and its open work. */
function Group({ t, now, focused, onFocus }: { t: GroupedThread; now: number; focused: boolean; onFocus: (slug: string) => void }) {
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
        {t.items.length === 0 ? <div className="px-1 text-[12px] text-ink-faint">Nothing open.</div> : t.items.map((v) => <Item key={v.id} v={v} />)}
      </div>
    </div>
  )
}
