// The inspector (theseus-jid7t.3; this row's part): the selection's facts and verbs, from components that exist. A
// task's or conversation's card whole, with its questions answered in place (`ConfirmCard`) and its verbs; a queue
// item's full card; a thread's panel with its verbs. The live stream, transcript, Record and Standing sections are a
// later row's (ops-act).
import { Link } from 'react-router'
import { CircleStop, ExternalLink, MousePointerClick, OctagonX, Pause, Play } from 'lucide-react'
import type { CalendarEvent, CalendarWindow, ConfirmRequest } from '@protocol'
import { ConfirmCard } from '@/components/ConfirmCard'
import { Pill, StatePill } from '@/components/ui'
import { usd } from '@/lib/format'
import type { OpsRow } from './board'
import { cardFacts } from './facts'
import type { QueueItem } from './queue'
import { Answers } from './Queue'
import { ThreadBody } from './ThreadPanel'
import { useOpsAct } from './act'
import { ActBtn } from './ActBtn'
import { EventDetail } from './EventDetail'

export type Selection =
  | { kind: 'row'; row: OpsRow }
  | { kind: 'item'; item: QueueItem }
  | { kind: 'thread'; slug: string }
  | { kind: 'mark'; key: string }
  // ops-timeline (theseus-jid7t.5): a calendar event, drawn redacted when the page is.
  | { kind: 'event'; ev: CalendarEvent; cal: CalendarWindow; redact: boolean }
  | { kind: 'none' }

export function Inspector({ sel, confirms, past, now }: { sel: Selection; confirms: readonly ConfirmRequest[]; past: number | null; now: number }) {
  if (sel.kind === 'thread') return <ThreadBody slug={sel.slug} live wide={false} />
  if (sel.kind === 'item') return <ItemWhole i={sel.item} past={past} />
  if (sel.kind === 'row') return <RowWhole r={sel.row} confirms={confirms.filter((c) => c.session_id === sel.row.sessionId)} past={past} now={now} />
  if (sel.kind === 'event') return <EventDetail ev={sel.ev} cal={sel.cal} redact={sel.redact} />
  if (sel.kind === 'mark') return <Hint>The selected mark’s panel, with its verbs, is under the week strip.</Hint>
  return <Hint><MousePointerClick size={16} className="text-gold" /> Select a card, a thread or a queue item: its facts and verbs show here. <span className="kbd">?</span> lists the keys.</Hint>
}

function Hint({ children }: { children: React.ReactNode }) {
  return <div className="flex flex-col items-center gap-2 px-4 py-10 text-center text-[12px] text-ink-faint">{children}</div>
}

function ItemWhole({ i, past }: { i: QueueItem; past: number | null }) {
  return (
    <div className="p-2.5">
      <div className="mb-1 font-display text-[10.5px] font-bold uppercase tracking-[0.08em] text-gold">Needs you · {i.owner}</div>
      <div className="mb-2 text-[13px] text-ink">{i.title}</div>
      <Answers i={i} whole note={null} onNote={() => {}} past={past} />
    </div>
  )
}

/** A board row whole: the card's every field, its questions answered in place, and its verbs. */
function RowWhole({ r, confirms, past, now }: { r: OpsRow; confirms: readonly ConfirmRequest[]; past: number | null; now: number }) {
  const { busy, act, on } = useOpsAct()
  const f = cardFacts(r, past ?? now)
  const info = r.task?.info
  const running = r.view?.state === 'running' || r.view?.state === 'waiting' || r.session?.execution_state === 'running'
  // `execution.stop` is a conversation's; a task is stopped by `task.cancel`.
  const exe = r.kind === 'conversation' ? r.view?.id ?? r.session?.execution_id : undefined
  return (
    <div className="flex flex-col gap-2 p-2.5" data-testid="inspector-row">
      <div className="flex items-center gap-1.5">
        {f.paused ? <Pill tone="wait">{f.pill}</Pill> : <StatePill state={f.pill} />}
        <span className="num text-[10.5px] text-ink-faint">{r.kind} · {f.short}</span>
        {f.href && <Link to={f.href} className="ml-auto flex items-center gap-1 text-[11.5px] text-live hover:underline" title="o: its deck">{f.open} <ExternalLink size={11} /></Link>}
      </div>
      <div className={r.kind === 'conversation' ? 'font-display text-[15px] font-bold tracking-[0.02em] text-ink' : 'text-[14px] font-semibold text-ink'}>{f.title}</div>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[11.5px]">
        {f.model && <><dt className="text-ink-faint">model</dt><dd className="num text-model">◇ {f.model}{r.view?.moved_to ? ' (moved by a checkpoint)' : ''}</dd></>}
        {f.place && <><dt className="text-ink-faint">place</dt><dd className="num text-ink" title={f.placeTip}>{f.place}</dd></>}
        {r.thread && <><dt className="text-ink-faint">thread</dt><dd className="num"><Link className="text-live" to={`/threads/${r.thread}`}>{r.thread}</Link></dd></>}
        {f.progress && <><dt className="text-ink-faint">progress</dt><dd className="num text-ink">{f.progress}</dd></>}
        {f.doing && <><dt className="text-ink-faint">doing</dt><dd className="text-ink-dim">{f.doing}</dd></>}
        {f.due && <><dt className="text-ink-faint">due</dt><dd className="num text-think">{f.due}</dd></>}
        {f.waitsOn && <><dt className="text-ink-faint">waits</dt><dd className="text-ink-dim">{f.waitsOn}</dd></>}
        {f.outcome && <><dt className="text-ink-faint">outcome</dt><dd className="text-fault">{f.outcome}</dd></>}
        {f.report && <><dt className="text-ink-faint">report</dt><dd className="text-ink-dim">{f.report}</dd></>}
        {info && <><dt className="text-ink-faint">spend</dt><dd className="num text-money">{usd(info.spent_usd)} of {usd(info.limit_usd)}</dd></>}
        <dt className="text-ink-faint">changed</dt><dd className="num text-ink-dim">{f.activity}</dd>
      </dl>
      {f.question && <div className="rounded-md bg-black/25 px-2 py-1.5 text-[12px] text-ink ring-1 ring-wait/30">? {f.question}</div>}
      <div className="flex flex-wrap gap-1.5">
        {f.pause && <ActBtn verb="pause" on={on} tone="wait" busy={busy === r.id} onClick={() => act('pause', r.id, r.paused ? 'task.resume' : 'task.pause', { task: r.id })}>{r.paused ? <><Play size={11} /> P · Resume</> : <><Pause size={11} /> P · Pause</>}</ActBtn>}
        {running && exe && <ActBtn verb="stop" on={on} tone="wait" busy={busy === `${exe}:stop`} onClick={() => act('stop', `${exe}:stop`, 'execution.stop', { execution_id: exe }, 'Stop its running turn?')}><CircleStop size={11} /> s s · Stop the turn</ActBtn>}
        {running && exe && <ActBtn verb="stop_all" on={on} tone="fault" busy={busy === `${exe}:all`} onClick={() => act('stop_all', `${exe}:all`, 'execution.stop', { execution_id: exe, tasks: true }, 'Stop its turn, and cancel every task under it?')}><OctagonX size={11} /> S · Stop all</ActBtn>}
        {f.stop && <ActBtn verb="cancel" on={on} tone="fault" busy={busy === `${r.id}:cancel`} onClick={() => act('cancel', `${r.id}:cancel`, 'task.cancel', { task: r.id }, `Stop task ${f.short} (${f.title})? Its jobs stop, and it reports that it was cancelled.`)}><OctagonX size={11} /> c c · Cancel</ActBtn>}
        {r.kind === 'wake' && <ActBtn verb="cancel_wake" on={on} tone="fault" busy={busy === `${r.id}:x`} onClick={() => act('cancel_wake', `${r.id}:x`, 'wake.cancel', { wake: r.id }, `Cancel the wake “${f.title}”?`)}><OctagonX size={11} /> x · Cancel</ActBtn>}
      </div>
      {confirms.map((c) => <ConfirmCard key={c.correlation_id} c={c} here past={past ?? undefined} />)}
    </div>
  )
}
