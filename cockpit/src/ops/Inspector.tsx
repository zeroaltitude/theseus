// The inspector (theseus-jid7t.3, .6): the selection's facts and verbs. A task's or conversation's card whole, with its
// questions answered in place (`ConfirmCard`), its verbs, its numbers, Live (its narrative, the model's words, its
// running job's output), its last turns with the composer, its Record and what stands for it; a job's command, eta and
// followed output; a queue item's full card; a thread's panel with its verbs.
import { Link } from 'react-router'
import { Box, CircleStop, ExternalLink, Lock, MousePointerClick, OctagonX, Pause, Play } from 'lucide-react'
import type { ConfirmRequest, SessionInfo, SessionModeResult, TaskRecord, ThreadRow } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useState } from 'react'
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
import { Facts, RecordBlock, Section, StandingBlock } from './InspectorSections'
import { JoinPane } from './JoinPane'
import { Live } from './Live'
import { jobFacts, recordFacts, runningJob, taskFacts, type FactLine } from './sections'
import { StickyChip } from './ModelMenu'
import { RehomeSheet, RescheduleSheet } from './TaskSheets'
import { targetOf, verbEntries } from './palette'
import { useRunEntry } from './useRunEntry'
import { verbById } from './verbs'

/** What the inspector needs of the page beyond the selection. */
export interface InspectorCx {
  records: readonly TaskRecord[]
  rows: readonly OpsRow[]
  onMenu: (m: 'model' | 'mode') => void
  /** The parent's row, when the selection reports to one (`p`). */
  onParent: (sessionId: string) => void
  /** Join a session by its id or a thread by its slug (the Join pane). */
  onJoin: (id: string) => void
  /** The id in the Join pane: its session's composer is there, so the inspector keeps no second one. */
  joined: string | null
}

export type Selection =
  | { kind: 'row'; row: OpsRow }
  | { kind: 'item'; item: QueueItem }
  | { kind: 'thread'; slug: string }
  | { kind: 'mark'; key: string }
  | { kind: 'none' }

export function Inspector({ sel, confirms, past, now, cx }: { sel: Selection; confirms: readonly ConfirmRequest[]; past: number | null; now: number; cx: InspectorCx }) {
  if (sel.kind === 'thread') return <ThreadJoin slug={sel.slug} cx={cx} />
  if (sel.kind === 'item') return <ItemWhole i={sel.item} past={past} />
  if (sel.kind === 'row' && sel.row.kind === 'job') return <JobWhole r={sel.row} now={now} />
  if (sel.kind === 'row') return <RowWhole r={sel.row} confirms={confirms.filter((c) => c.session_id === sel.row.sessionId)} past={past} now={now} cx={cx} />
  if (sel.kind === 'mark') return <Hint>The selected mark’s panel, with its verbs, is under the week strip.</Hint>
  return <Hint><MousePointerClick size={16} className="text-gold" /> Select a card, a thread or a queue item: its facts and verbs show here. <span className="kbd">?</span> lists the keys.</Hint>
}

function ThreadJoin({ slug, cx }: { slug: string; cx: InspectorCx }) {
  const { on } = useOpsAct()
  // Beside the Join pane: the thread's mode and data mark, its session's, then the thread page (card, working set).
  const { data: row } = useRpc<ThreadRow>('thread.show', { slug }, 0)
  const sid = row?.thread.session_id ?? undefined
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const { data: mode } = useRpc<SessionModeResult>('session.mode', { session_id: sid }, 0, { enabled: !!sid })
  const mark = sl?.sessions.find((x) => x.session_id === sid)?.data ?? 'open'
  const lines: FactLine[] = sid ? [{ label: 'mode', value: mode?.mode ?? '…' }, { label: 'data', value: mark, tone: mark === 'open' ? undefined : 'wait' }] : [{ label: 'session', value: 'opens with the first message' }]
  return (
    <div className="flex flex-col gap-2">
      <div className="flex flex-wrap items-center gap-2 px-2.5 pt-2.5">
        <ActBtn verb="join" on={on} tone="live" onClick={() => cx.onJoin(slug)} title="i: talk to the thread; its first message opens its session">i · Join and chat</ActBtn>
        {mark !== 'open' && <Pill tone="wait" title="the floor keeps this to models of US origin"><Lock size={10} /> {mark}</Pill>}
      </div>
      <div className="px-2.5"><Facts lines={lines} /></div>
      <ThreadBody slug={slug} live wide={false} />
    </div>
  )
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
function RowWhole({ r, confirms, past, now, cx }: { r: OpsRow; confirms: readonly ConfirmRequest[]; past: number | null; now: number; cx: InspectorCx }) {
  const { busy, act, on } = useOpsAct()
  const runEntry = useRunEntry()
  const [sheet, setSheet] = useState<'rehome' | 'reschedule' | null>(null)
  const f = cardFacts(r, past ?? now)
  const lifeEntries = verbEntries([targetOf(r)], { profiles: [], prices: new Map() }).filter((e) => ['recompile:transcript', 'recompile:fresh', 'retire', 'reopen'].includes(e.id) && !e.grey)
  const info = r.task?.info
  const running = r.view?.state === 'running' || r.view?.state === 'waiting' || r.session?.execution_state === 'running'
  // `execution.stop` is a conversation's; a task is stopped by `task.cancel`.
  const exe = r.kind === 'conversation' ? r.view?.id ?? r.session?.execution_id : undefined
  return (
    <div className="flex flex-col gap-2 p-2.5" data-testid="inspector-row">
      <div className="flex items-center gap-1.5">
        {f.paused ? <Pill tone="wait">{f.pill}</Pill> : <StatePill state={f.pill} />}
        <span className="num text-[10.5px] text-ink-faint">{r.kind} · {f.short}</span>
        {r.data && r.data !== 'open' && <Pill tone="wait" title="the floor keeps this to models of US origin"><Lock size={10} /> {r.data}</Pill>}
        {f.href && <Link to={f.href} className="ml-auto flex items-center gap-1 text-[11.5px] text-live hover:underline" title="o: its deck">{f.open} <ExternalLink size={11} /></Link>}
      </div>
      <div className={r.kind === 'conversation' ? 'font-display text-[15px] font-bold tracking-[0.02em] text-ink' : 'text-[14px] font-semibold text-ink'}>{f.title}</div>
      <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-0.5 text-[11.5px]">
        {f.model && <><dt className="text-ink-faint">model</dt><dd className="num text-model">◇ {f.model}{r.view?.moved_to ? ' (moved by a checkpoint)' : ''}{r.sessionId && <span className="ml-1.5"><StickyChip sessionId={r.sessionId} /></span>}</dd></>}
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
        {r.sessionId && <ActBtn verb="join" on={on} tone="live" onClick={() => cx.onJoin(r.sessionId!)} title="i: talk to it in the Join pane">i · Join</ActBtn>}
        {r.sessionId && <ActBtn verb="model_menu" on={on} tone="model" onClick={() => cx.onMenu('model')} title="m: the model for this session"><Box size={11} /> m · Model</ActBtn>}
        {r.kind === 'conversation' && <ActBtn verb="mode_menu" on={on} tone="idle" onClick={() => cx.onMenu('mode')} title="M: Ask, Edits, Plan, Auto">M · Mode</ActBtn>}
        {r.task?.info?.parent_session_id && <ActBtn verb="inspect" on={on} tone="idle" onClick={() => cx.onParent(r.task!.info!.parent_session_id)} title="p: its parent">p · Parent</ActBtn>}
        {r.kind === 'wake' && <ActBtn verb="cancel_wake" on={on} tone="fault" busy={busy === `${r.id}:x`} onClick={() => act('cancel_wake', `${r.id}:x`, 'wake.cancel', { wake: r.id }, `Cancel the wake “${f.title}”?`)}><OctagonX size={11} /> x · Cancel</ActBtn>}
        {lifeEntries.map((e) => <ActBtn key={e.id} verb="session_life" on={on} tone="idle" onClick={() => void runEntry(e)}>{e.label}</ActBtn>)}
        {r.kind === 'task' && r.task?.info && f.stop && <ActBtn verb="rehome" on={on} tone="think" onClick={() => setSheet('rehome')} title="move where its report goes (asks)">Re-home…</ActBtn>}
        {r.kind === 'task' && r.column === 'scheduled' && <ActBtn verb="reschedule" on={on} tone="think" onClick={() => setSheet('reschedule')} title="change when it starts">Reschedule…</ActBtn>}
      </div>
      {confirms.map((c) => <ConfirmCard key={c.correlation_id} c={c} here past={past ?? undefined} />)}
      {sheet === 'rehome' && <RehomeSheet task={r.id} title={f.title} home={r.task?.info?.parent_session_id} onClose={() => setSheet(null)} />}
      {sheet === 'reschedule' && <RescheduleSheet task={r.id} title={f.title} startsAt={r.dueMs} onClose={() => setSheet(null)} />}
      {(r.kind === 'task' || r.kind === 'conversation') && <Sections r={r} cx={cx} now={now} />}
    </div>
  )
}

/** A task's or conversation's sections: facts, Live, the last turns with the composer, Record, Standing. */
function Sections({ r, cx, now }: { r: OpsRow; cx: InspectorCx; now: number }) {
  const facts = taskFacts(r, now).filter((l) => l.label !== 'spend')
  const rec = recordFacts(cx.records.find((t) => t.session === r.sessionId), now)
  const job = runningJob(cx.rows, r.sessionId)
  return (
    <>
      <Section title="Live" hint={job ? `following ${job.title}` : undefined}><Live sessionId={r.sessionId} jobId={job?.id ?? null} /></Section>
      {facts.length > 0 && <Section title="Facts"><Facts lines={facts} /></Section>}
      {r.sessionId && cx.joined !== r.sessionId && <Section title="Transcript" hint={<Link className="text-live" to={`/session/${r.sessionId}`}>open the deck for more</Link>}><JoinPane sessionId={r.sessionId} session={r.session} compact /></Section>}
      {rec && <Section title="Record"><RecordBlock f={rec} /></Section>}
      {r.sessionId && <Section title="Standing"><StandingBlock sessionId={r.sessionId} /></Section>}
    </>
  )
}

/** A job: its command, its time against its usual, its output followed live, and Stop. */
function JobWhole({ r, now }: { r: OpsRow; now: number }) {
  const { on } = useOpsAct()
  const stop = verbById('job_stop')
  return (
    <div className="flex flex-col gap-2 p-2.5" data-testid="inspector-job">
      <div className="flex items-center gap-1.5"><StatePill state={r.state ?? 'running'} /><span className="num text-[10.5px] text-ink-faint">job · {r.id.slice(-6)}</span></div>
      <Facts lines={jobFacts(r, now)} />
      <div className="flex flex-wrap gap-1.5"><ActBtn verb="stop" on={on} tone="wait" onClick={() => {}} title={stop?.missing}><CircleStop size={11} /> Stop</ActBtn></div>
      {stop?.missing && <div className="text-[11px] text-ink-faint">{stop.missing}</div>}
      <Section title="Output"><Live sessionId={r.sessionId} jobId={r.id} /></Section>
      {r.sessionId && <Link to={`/session/${r.sessionId}`} className="text-[11.5px] text-live hover:underline">its session’s deck</Link>}
    </div>
  )
}
