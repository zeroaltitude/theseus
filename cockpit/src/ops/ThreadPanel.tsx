// A thread, whole (theseus-5i8i; theseus-jid7t.3): its record, its work and its recent turns, drawn from
// `threadPanelFacts`. The thread's own page (`/threads/:slug`) shows it, and the Threads lens's inspector shows it with
// the thread's verbs (pause, resume, done, reopen, seal, unseal, edit), each one existing call.
import { useEffect, useMemo } from 'react'
import { Link } from 'react-router'
import { useQueryClient } from '@tanstack/react-query'
import { GitBranch, MessagesSquare } from 'lucide-react'
import type { NodeInfo, SessionHistoryResult, ThreadRow, WorkView } from '@protocol'
import { client, useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { useThreadBoard } from '@/lib/threadwatch'
import { groupThreads } from '@/lib/threadboard'
import { ago } from '@/lib/format'
import { AttentionPill, Empty, Panel, Pill } from '@/components/ui'
import { threadPanelFacts } from './facts'
import { useOpsAct } from './act'
import { ActBtn } from './ActBtn'

/** One open task, question or wake: its attention, its title, and the session it lives in. */
export function Item({ v }: { v: WorkView }) {
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

/** The last turns of a thread's session: what was said, newest last. `interval` 0 reads once; the panel reads again
 *  when the thread's last activity moves. */
function Turns({ session, interval, stamp }: { session: string; interval: number; stamp: number }) {
  const { data, refetch } = useRpc<SessionHistoryResult>('session.history', { session_id: session, n: 12 }, interval)
  useEffect(() => { if (interval === 0 && stamp) void refetch() }, [stamp, interval, refetch])
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

/** A thread's record, work and turns. `live`: the lens's panel, which reads on the push instead of every 5 s and
 *  carries the thread's verbs; the thread's own page keeps its reads as they were. */
export function ThreadBody({ slug, live = false, wide = true }: { slug: string; live?: boolean; wide?: boolean }) {
  const now = useTick(15_000)
  const qc = useQueryClient()
  const { board } = useThreadBoard()
  const { data: row, error } = useRpc<ThreadRow>('thread.show', { slug }, live ? 0 : 5000)
  // The lens reads the record again when the daemon says a thread changed: no timer.
  useEffect(() => (live ? client.onNotify((m) => { if (m === 'thread.changed') void qc.invalidateQueries({ queryKey: ['thread.show'] }) }) : undefined), [live, qc])
  const grouped = useMemo(() => groupThreads(Object.values(board.views), Object.values(board.groups)), [board])
  const mine = grouped.threads.find((t) => t.group.slug === slug)
  if (error) return <Panel title="Thread" help={false}><Empty>{String((error as { message?: string }).message ?? error)}</Empty></Panel>
  if (!row) return <Empty>Reading the thread…</Empty>
  const f = threadPanelFacts(row, mine, now)
  return (
    <div className={wide ? 'grid gap-3 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.2fr)]' : 'flex flex-col gap-3'}>
      <div className="space-y-3">
        <Panel title={f.title} icon={<GitBranch size={14} />} help="threads.thread"
          actions={<Pill tone={f.state === 'active' ? 'live' : 'idle'}>{f.state}</Pill>}>
          <div className="space-y-2 p-3 text-[12px]">
            {f.goal && <p className="text-ink">{f.goal}</p>}
            {f.details && <p className="num text-ink-faint">{f.details}</p>}
            <p className="num text-ink-faint">{f.context ? `${f.context} · ` : ''}{f.today}</p>
            {f.sealPending && <p className="text-fault">{f.sealPending}</p>}
            {f.sealedBy && <p className="text-ink-faint">{f.sealedBy}</p>}
            {f.topics && <p className="text-ink-faint">{f.topics}</p>}
            {f.links.length > 0 && (
              <p className="text-ink-faint">{f.links.map((l, i) => (
                <span key={`${l.label}:${l.slug}`}>{i > 0 && ' · '}{l.label} <Link to={`/threads/${l.slug}`} className="text-live">{l.slug}</Link></span>
              ))}</p>
            )}
            {live && <ThreadVerbs row={row} />}
          </div>
        </Panel>
        <Panel title="Card" help="threads.card">
          {f.card.length === 0 ? <Empty>No card yet.</Empty> : (
            <ul className="list-disc space-y-0.5 p-3 pl-6 text-[12px] text-ink">{f.card.map((l, i) => <li key={i}>{l}</li>)}</ul>
          )}
        </Panel>
        <Panel title="Working set" help="threads.workingset">
          {f.workingSet.length === 0 ? <Empty>Nothing yet.</Empty> : <ul className="space-y-0.5 p-3 text-[12px] text-ink">{f.workingSet.map((l) => <li key={l}>{l}</li>)}</ul>}
        </Panel>
      </div>
      <div className="space-y-3">
        <Panel title="Open work" icon={<GitBranch size={14} />} help="threads.work">
          <div className="space-y-0.5 p-3">
            {!mine || f.openWork.length === 0 ? <div className="text-[12px] text-ink-faint">Nothing open.</div> : f.openWork.map((w, i) => <Item key={w.id} v={mine.items[i]} />)}
          </div>
        </Panel>
        <Panel title="Recent turns" icon={<MessagesSquare size={14} />} help="threads.turns">
          {f.turnsOf ? <Turns session={f.turnsOf} interval={live ? 0 : 5000} stamp={mine?.lastActiveMs ?? 0} /> : <Empty>The first message opens its session.</Empty>}
        </Panel>
      </div>
    </div>
  )
}

/** The thread's verbs, each one call: pause or resume, done or reopen, seal or unseal, and edit its title and goal. */
function ThreadVerbs({ row }: { row: ThreadRow }) {
  const { busy, act, on } = useOpsAct()
  const t = row.thread
  const slug = t.slug
  const go = (verb: Parameters<typeof act>[0], method: string, params: Record<string, unknown>, ask?: string) => void act(verb, `${slug}:${method}`, method, params, ask)
  const edit = () => {
    const title = window.prompt('Title', t.title)
    if (title == null) return
    const goal = window.prompt('Goal', t.goal ?? '')
    if (goal == null) return
    go('thread_edit', 'thread.edit', { slug, ...(title !== t.title && { title }), ...(goal !== (t.goal ?? '') && { goal }) })
  }
  const seal = () => {
    const reason = window.prompt(`Seal ${t.title}? Its notes stop reaching any other thread’s recall. Why (optional):`, t.seal_pending ?? '')
    if (reason == null) return
    go('seal', 'thread.seal', { slug, ...(reason.trim() && { reason: reason.trim() }) })
  }
  const b = (k: string) => busy === `${slug}:${k}`
  return (
    <div className="flex flex-wrap gap-1.5 pt-1" data-testid="thread-verbs">
      {t.state === 'active' && <ActBtn verb="thread_pause" on={on} tone="wait" busy={b('thread.pause')} onClick={() => go('thread_pause', 'thread.pause', { slug })}>Pause</ActBtn>}
      {t.state === 'paused' && <ActBtn verb="thread_resume" on={on} tone="ok" busy={b('thread.resume')} onClick={() => go('thread_resume', 'thread.resume', { slug })}>Resume</ActBtn>}
      {t.state !== 'done' && <ActBtn verb="thread_done" on={on} tone="idle" busy={b('thread.done')} onClick={() => go('thread_done', 'thread.done', { slug }, `Mark ${t.title} done?`)}>Done</ActBtn>}
      {t.state === 'done' && <ActBtn verb="thread_reopen" on={on} tone="live" busy={b('thread.reopen')} onClick={() => go('thread_reopen', 'thread.reopen', { slug })}>Reopen</ActBtn>}
      {!t.sealed && <ActBtn verb="seal" on={on} tone="think" busy={b('thread.seal')} onClick={seal}>Seal</ActBtn>}
      {t.sealed && <ActBtn verb="unseal" on={on} tone="think" busy={b('thread.unseal')} onClick={() => go('unseal', 'thread.unseal', { slug }, `Unseal ${t.title}? Its notes may reach other threads again.`)}>Unseal</ActBtn>}
      {t.seal_pending && <ActBtn verb="seal_dismiss" on={on} tone="fault" busy={b('thread.seal_dismiss')} onClick={() => go('seal_dismiss', 'thread.seal_dismiss', { slug })}>Not now</ActBtn>}
      <ActBtn verb="thread_edit" on={on} tone="idle" busy={b('thread.edit')} onClick={edit}>Edit…</ActBtn>
    </div>
  )
}
