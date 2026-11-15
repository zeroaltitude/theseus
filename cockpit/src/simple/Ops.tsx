// Ops, the simple page: what waits for the owner, what is in flight, what is planned and not yet started, and what
// finished today, as four short lists, with the selected piece of work open beside them to watch and to guide.
//
// It is the Operations page's work set read plainly (`ops/board.ts`'s rows and `ops/queue.ts`'s asks, nothing of its
// own), so a number here is the number there. It reads and does not poll: the board from the one `work.watch`,
// `task.list`, `confirm.list` and `session.list` on their pushes, and today's spend from the ledger's one copy.
// What is selected is in the address (`?sel=`, and `?pane=details`), so a link reopens it. Every act is the
// Operations page's own: an answer is `work.answer`, a message is `turn.submit`, each confirmed where that page
// confirms it.
import { useCallback, useMemo, useState, type ReactNode } from 'react'
import { useSearchParams } from 'react-router'
import { CalendarClock, Circle, CircleAlert, CircleCheck, CircleDollarSign, CircleDot, CircleHelp, CirclePause, CircleX, Clock, Inbox, ListTodo, Loader, Lock, MessageCircle, MessagesSquare, PencilLine, Plus, Radio, ShieldQuestion, Split, type LucideIcon } from 'lucide-react'
import type { ConfirmRequest, SessionInfo, SessionListResult, TaskListResult } from '@protocol'
import { useConn, useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { useHistoryRows } from '@/lib/history'
import { useThreadBoard } from '@/lib/threadwatch'
import { ago, cn, short, usd } from '@/lib/format'
import { stateOf } from '@/lib/sessionState'
import { Panel, Empty } from '@/components/ui'
import { ConfirmCard } from '@/components/ConfirmCard'
import type { OpsRow } from '@/ops/board'
import type { QueueItem } from '@/ops/queue'
import { cardFacts, itemFacts, ITEM_LOOK, waitingFacts } from '@/ops/facts'
import { rowFor } from '@/ops/sections'
import { useRouterAsks } from '@/ops/asks'
import { Answers } from '@/ops/Queue'
import { Inspector } from '@/ops/Inspector'
import { JoinPane } from '@/ops/JoinPane'
import { ModeMenu, ModelMenu } from '@/ops/ModelMenu'
import { NewTask } from '@/ops/NewTask'
import { Toast } from '@/ops/Toast'
import { Page, Stat } from './kit'
import { workSetOf, defaultSel, selectedOf } from './ops/workset'

/** One empty list for every absent read, so a memo over it holds. */
const NONE: never[] = []
/** How many rows a list shows before "show all". */
const SHORT = 8

/** Each state's mark and tone: a shape always goes with the colour. */
const LOOK: Readonly<Record<OpsRow['bucket'], { Icon: LucideIcon; tone: string }>> = {
  needs: { Icon: CircleAlert, tone: 'wait' }, working: { Icon: CircleDot, tone: 'live' }, scheduled: { Icon: Clock, tone: 'think' }, paused: { Icon: CirclePause, tone: 'idle' },
  todo: { Icon: Circle, tone: 'idle' }, rest: { Icon: MessageCircle, tone: 'idle' }, ended: { Icon: CircleCheck, tone: 'ok' },
}
/** Each kind of ask's mark (its tone is `ITEM_LOOK`'s). */
const ASK_ICON: Readonly<Record<QueueItem['kind'], LucideIcon>> = { approval: ShieldQuestion, budget: CircleDollarSign, change: PencilLine, question: CircleHelp, failure: CircleX, seal: Lock, ask: Split }
type PaneTab = 'talk' | 'details'

export default function SimpleOps() {
  const [params, setParams] = useSearchParams()
  const asked = params.get('sel')
  const paneAsked: PaneTab = params.get('pane') === 'details' ? 'details' : 'talk'
  const set = useCallback((patch: { sel?: string | null; pane?: PaneTab }) => setParams(() => {
    const next = new URLSearchParams(window.location.search)
    if (patch.sel !== undefined) { if (patch.sel) next.set('sel', patch.sel); else next.delete('sel'); next.delete('pane') }
    if (patch.pane !== undefined) { if (patch.pane === 'details') next.set('pane', 'details'); else next.delete('pane') }
    return next
  }, { replace: true }), [setParams])
  const now = useTick(15_000)
  const { board, ready, error } = useThreadBoard()
  const { data: tl } = useRpc<TaskListResult>('task.list', {}, 3000)
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 1500)
  const { data: sl } = useRpc<SessionListResult>('session.list', undefined, 5000)
  const history = useHistoryRows()
  const asks = useRouterAsks(now)
  const linked = useConn((s) => s.status === 'open')

  const views = useMemo(() => Object.values(board.views), [board.views])
  const groups = useMemo(() => Object.values(board.groups), [board.groups])
  const tasks = tl?.tasks ?? NONE
  const records = tl?.records ?? NONE
  const sessions = sl?.sessions ?? NONE
  const confirms = cl?.confirms ?? NONE

  // The one work set (`./ops/workset.ts`): the Operations page's steps, so a number here is the number there.
  const work = useMemo(() => workSetOf({ views, tasks, sessions, records, confirms, groups, asks, history: history.rows, now }), [views, tasks, sessions, records, confirms, groups, asks, history.rows, now])
  const { queue, rows, cols, counts, waiting, next, resting, planned } = work
  const spend = { usd: work.spendUsd }

  // What is open beside the lists: the address's choice, else the first thing that waits, else the newest work in
  // flight, else the conversation last spoken in.
  const sel = asked ?? defaultSel(work)
  const { item, row, session } = selectedOf(work, sessions, sel)
  const pick = (id: string | null) => set({ sel: id })

  const [note, setNote] = useState<{ key: string; text: string } | null>(null)
  const [menu, setMenu] = useState<{ kind: 'model' | 'mode'; sid: string } | null>(null)
  const [newTask, setNewTask] = useState(false)
  const menuSession = menu ? sessions.find((x) => x.session_id === menu.sid) : undefined
  const onMenu = (kind: 'model' | 'mode', sid: string) => setMenu({ kind, sid })

  const jump = (id: string) => document.getElementById(id)?.scrollIntoView({ behavior: 'smooth', block: 'start' })

  return (
    <Page fill title="Ops" sub="What Theseus is doing right now, what waits for you, and what comes next."
      actions={<button type="button" onClick={() => setNewTask(true)} className="brass-button"><Plus size={14} aria-hidden /> New task</button>}>
      <Panel help="simple.ops.now" title="Right now" icon={<Radio size={14} />} className="shrink-0" bodyClassName="grid grid-cols-2 gap-1 p-2 sm:grid-cols-5">
        <Stat n={counts.needs} label={counts.needs === 1 ? 'needs you' : 'need you'} tone={counts.needs > 0 ? 'wait' : undefined} onClick={() => jump('ops-needs')} title="pieces of work that wait on you" />
        <Stat n={counts.working} label="in flight" tone={counts.working > 0 ? 'live' : undefined} onClick={() => jump('ops-flight')} title="tasks and conversations working now" />
        <Stat n={planned} label="planned" onClick={() => jump('ops-planned')} title="steps not yet started, scheduled work, and paused work" />
        <Stat n={counts.done} label="done today" onClick={() => jump('ops-done')} title="work that ended since midnight" />
        <Stat n={usd(spend.usd)} label="spent today" tone="money" title="what today's model calls cost, from the ledger" />
      </Panel>
      {!linked && <div role="status" className="shrink-0 rounded-lg bg-wait/10 px-3 py-2 text-[12.5px] text-wait ring-1 ring-wait/30">Not connected to Theseus. What is shown is what was last read; the page reconnects by itself.</div>}
      {error && <div className="shrink-0 rounded-lg bg-fault/10 px-3 py-2 text-[12.5px] text-fault ring-1 ring-fault/30">The board could not be read: {error}</div>}
      <div className="grid min-h-0 flex-1 gap-4 lg:grid-cols-[minmax(340px,460px)_minmax(0,1fr)]">
        <div className="flex min-h-0 flex-col gap-4 overflow-y-auto pb-2 pr-1">
          <div id="ops-needs" className="shrink-0 scroll-mt-2">
            <Panel help="simple.ops.needs" icon={<Inbox size={14} />} bodyClassName="p-1.5" title={counted('Needs you', counts.needs, counts.needs > 0, queue.length + waiting.length !== counts.needs ? `${queue.length + waiting.length} to answer` : undefined)}>
              <Rows empty={ready ? 'Nothing waits for you.' : 'Reading the board…'}>
                {queue.map((i) => <ItemRow key={i.key} i={i} now={now} current={sel === i.key} onPick={pick} />)}
                {waiting.map((r) => { const w = waitingFacts(r, now); return <RowButton key={r.id} current={sel === r.id} onPick={() => pick(r.id)} Icon={CircleAlert} tone="wait" title={w.title} sub={`${w.why} · ${w.age}`} /> })}
              </Rows>
            </Panel>
          </div>
          <div id="ops-flight" className="shrink-0 scroll-mt-2">
            <Panel help="simple.ops.flight" icon={<Loader size={14} />} bodyClassName="p-1.5" title={counted('In flight', work.flight.length)}>
              <Rows empty={ready ? 'Nothing is running.' : 'Reading the board…'}>
                {work.flight.map((r) => <WorkRow key={r.id} r={r} now={now} current={sel === r.id} onPick={pick} />)}
              </Rows>
            </Panel>
          </div>
          <Planned next={next} scheduled={cols.scheduled} paused={cols.paused} rows={rows} now={now} sel={sel} onPick={pick} n={planned} />
          <div id="ops-talk" className="shrink-0 scroll-mt-2">
            <Panel help="simple.ops.conversations" icon={<MessagesSquare size={14} />} bodyClassName="p-1.5" title={counted('Conversations', resting.length)}>
              <Rows empty={ready ? 'No conversation is open.' : 'Reading the board…'} short>
                {resting.map((x) => (
                  <RowButton key={x.session_id} current={sel === x.session_id} onPick={() => pick(x.session_id)} Icon={MessageCircle} tone="idle" title={x.title || x.label || short(x.session_id)}
                    sub={[stateOf(x), x.profile ?? x.model, `${x.turns} ${x.turns === 1 ? 'turn' : 'turns'}`, ago(x.last_active_ms, now)].filter(Boolean).join(' · ')} />
                ))}
              </Rows>
            </Panel>
          </div>
          <div id="ops-done" className="shrink-0 scroll-mt-2">
            <Panel help="simple.ops.done" icon={<CircleCheck size={14} />} bodyClassName="p-1.5" title={counted('Done today', counts.done)}>
              <Rows empty="Nothing has finished today." short>
                {cols.done.map((r) => <WorkRow key={r.id} r={r} now={now} current={sel === r.id} onPick={pick} />)}
              </Rows>
            </Panel>
          </div>
        </div>
        <Detail item={item} row={row} session={session} rows={rows} records={records} confirms={confirms} now={now} tab={paneAsked} onTab={(pane) => set({ pane })}
          note={note} onNote={setNote} onMenu={onMenu} onPick={pick} />
      </div>
      {newTask && <NewTask home={row?.kind === 'conversation' ? row.sessionId ?? '' : ''} onClose={() => setNewTask(false)} onStarted={(id) => { setNewTask(false); pick(id) }} />}
      {menu?.kind === 'model' && menuSession && <ModelMenu session={menuSession} onClose={() => setMenu(null)} />}
      {menu?.kind === 'mode' && menuSession && <ModeMenu session={menuSession} onClose={() => setMenu(null)} />}
      <Toast />
    </Page>
  )
}

/** A list's name with how many it holds (the counter's number); amber when that many wait on the owner. */
const counted = (title: string, n: number, glow = false, more?: string) => (
  <span className="flex items-center gap-2">{title}<span className={cn('num rounded-full px-1.5 text-[11.5px] font-medium', glow ? 'bg-wait/15 text-wait' : 'text-ink-faint')}>{n}</span>
    {more && <span className="text-[11.5px] font-normal normal-case tracking-normal text-ink-faint" title="one piece of work can hold more than one question">{more}</span>}</span>
)

/** A list's rows (the first few, then all on request), or what an empty one means. */
function Rows({ empty, short, children }: { empty: string; /** show only the first few until asked */ short?: boolean; children: ReactNode }) {
  const all = (Array.isArray(children) ? children.flat() : [children]).filter(Boolean)
  const [open, setOpen] = useState(false)
  const shown = short && !open ? all.slice(0, SHORT) : all
  return (
    <>
      {all.length === 0 ? <div className="px-3 py-4 text-[12.5px] text-ink-faint">{empty}</div> : shown}
      {short && all.length > SHORT && (
        <button type="button" onClick={() => setOpen(!open)} className="mt-0.5 w-full rounded-lg px-3 py-1.5 text-left text-[12px] text-live hover:underline">{open ? 'Show fewer' : `Show all ${all.length}`}</button>
      )}
    </>
  )
}

/** What is planned and has not started: the steps that come next, what is on the clock, and what is held. */
function Planned({ next, scheduled, paused, rows, now, sel, onPick, n }: { next: readonly OpsRow[]; scheduled: readonly OpsRow[]; paused: readonly OpsRow[]; rows: readonly OpsRow[]; now: number; sel: string | null; onPick: (id: string) => void; n: number }) {
  const [open, setOpen] = useState(false)
  const steps = open ? next : next.slice(0, SHORT)
  const after = (r: OpsRow): string => {
    const who = (r.carrier && rowFor(rows, r.carrier)) || (r.parent && rowFor(rows, r.parent)) || undefined
    const what = r.plan ? 'a plan' : 'a step'
    return who ? `${what} ${who.bucket === 'working' ? 'after' : 'under'} ${who.title}` : `${what} no one has taken yet`
  }
  return (
    <div id="ops-planned" className="shrink-0 scroll-mt-2">
      <Panel help="simple.ops.planned" icon={<ListTodo size={14} />} bodyClassName="p-1.5"
        title={counted('Planned', n)}>
        {n === 0 && <div className="px-3 py-4 text-[12.5px] text-ink-faint">Nothing is planned beyond what is running.</div>}
        {next.length > 0 && <div className="simple-eyebrow px-3 pb-1 pt-2">Next steps</div>}
        {steps.map((r) => <RowButton key={r.id} current={sel === r.id} onPick={() => onPick(r.id)} Icon={Circle} tone="idle" title={r.title} sub={after(r)} />)}
        {next.length > SHORT && <button type="button" onClick={() => setOpen(!open)} className="w-full rounded-lg px-3 py-1.5 text-left text-[12px] text-live hover:underline">{open ? 'Show fewer' : `Show all ${next.length} steps`}</button>}
        {scheduled.length > 0 && <div className="simple-eyebrow flex items-center gap-1.5 px-3 pb-1 pt-3"><CalendarClock size={11} aria-hidden /> On the clock</div>}
        {scheduled.map((r) => <WorkRow key={r.id} r={r} now={now} current={sel === r.id} onPick={onPick} />)}
        {paused.length > 0 && <div className="simple-eyebrow px-3 pb-1 pt-3">Paused</div>}
        {paused.map((r) => <WorkRow key={r.id} r={r} now={now} current={sel === r.id} onPick={onPick} />)}
      </Panel>
    </div>
  )
}

function RowButton({ current, onPick, Icon, tone, title, sub, badge }: { current: boolean; onPick: () => void; Icon: LucideIcon; tone: string; title: string; sub: string; badge?: ReactNode }) {
  return (
    <button type="button" onClick={onPick} aria-current={current ? 'true' : undefined} className="simple-row">
      <Icon aria-hidden size={15} className="mt-0.5 shrink-0" style={{ color: `var(--color-${tone})` }} />
      <span className="min-w-0 flex-1">
        <span className="simple-row-title block truncate">{title}</span>
        <span className="simple-row-sub block truncate">{sub}</span>
      </span>
      {badge}
    </button>
  )
}

/** A piece of work as one row: what it is, what it is doing now, and when it last changed. */
function WorkRow({ r, now, current, onPick }: { r: OpsRow; now: number; current: boolean; onPick: (id: string) => void }) {
  const f = cardFacts(r, now)
  const doing = f.doing ?? f.waitsOn ?? f.outcome ?? f.progress
  const sub = [r.kind, doing, f.model && `◇ ${f.model}`, f.due ? `due ${f.due}` : f.activity].filter(Boolean).join(' · ')
  return (
    <RowButton current={current} onPick={() => onPick(r.id)} Icon={LOOK[r.bucket].Icon} tone={LOOK[r.bucket].tone} title={f.title} sub={sub}
      badge={r.asks > 0 ? <span className="num mt-0.5 shrink-0 rounded-full bg-wait/15 px-1.5 text-[11px] font-medium text-wait" title={`${r.asks} ${r.asks === 1 ? 'ask waits' : 'asks wait'} on you`}>{r.asks}</span> : undefined} />
  )
}

/** Something that waits on the owner: what is asked, who asks, and for how long. */
function ItemRow({ i, now, current, onPick }: { i: QueueItem; now: number; current: boolean; onPick: (key: string) => void }) {
  const f = itemFacts(i, now)
  return <RowButton current={current} onPick={() => onPick(i.key)} Icon={ASK_ICON[i.kind]} tone={ITEM_LOOK[i.kind].tone} title={f.title} sub={`${f.kind} · ${f.owner} · waiting ${f.age}`} />
}

/** The selected piece of work, whole: an ask with its answers, or a session to talk to and its details. */
function Detail({ item, row, session, rows, records, confirms, now, tab, onTab, note, onNote, onMenu, onPick }: {
  item: QueueItem | undefined; row: OpsRow | undefined; session: SessionInfo | undefined; rows: readonly OpsRow[]; records: TaskListResult['records']; confirms: readonly ConfirmRequest[]; now: number
  tab: PaneTab; onTab: (t: PaneTab) => void; note: { key: string; text: string } | null; onNote: (n: { key: string; text: string } | null) => void
  onMenu: (kind: 'model' | 'mode', sid: string) => void; onPick: (id: string | null) => void
}) {
  const sid = item?.sessionId ?? row?.sessionId ?? session?.session_id
  const talks = !!sid && (!!item || !!session || row?.kind === 'task' || row?.kind === 'conversation')
  // An ask shows its answers over its conversation; a session shows the conversation or its details.
  const shown: PaneTab = item || session ? 'talk' : talks && tab === 'talk' ? 'talk' : 'details'
  const title = item ? item.title : row ? row.title : session ? session.title || session.label || short(session.session_id) : 'Nothing is selected'
  const mine = sid ? confirms.filter((c) => c.session_id === sid) : NONE
  const tabs = row && talks && (
    <div role="tablist" aria-label="What to show of it" className="flex items-center gap-0.5">
      <button type="button" role="tab" aria-selected={shown === 'talk'} onClick={() => onTab('talk')} className="simple-tab !px-2 !py-0.5 !text-[12px]">Talk</button>
      <button type="button" role="tab" aria-selected={shown === 'details'} onClick={() => onTab('details')} className="simple-tab !px-2 !py-0.5 !text-[12px]">Details</button>
    </div>
  )
  return (
    <Panel help="simple.ops.session" title={<span className="truncate">{title}</span>} actions={tabs} className="min-h-[420px] lg:min-h-0" bodyClassName="flex min-h-0 flex-col overflow-hidden">
      {!item && !row && !session && <Empty>Choose anything on the left to watch it and to guide it.</Empty>}
      {item && (
        <div className="shrink-0 border-b border-line p-3">
          <div className="simple-eyebrow mb-1">Needs you · {item.owner}</div>
          <Answers i={item} whole note={note?.key === item.key ? note.text : null} onNote={onNote} past={null} />
        </div>
      )}
      {(item || row || session) && shown === 'talk' && sid && (
        <>
          {!item && mine.length > 0 && <div className="flex shrink-0 flex-col gap-2 border-b border-line p-3">{mine.map((c) => <ConfirmCard key={c.correlation_id} c={c} here />)}</div>}
          <div className="min-h-0 flex-1"><JoinPane key={sid} sessionId={sid} session={row?.session ?? session} onMenu={onMenu} /></div>
        </>
      )}
      {row && shown === 'details' && (
        <div className="min-h-0 flex-1 overflow-y-auto">
          <Inspector sel={{ kind: 'row', row }} confirms={confirms} past={null} now={now}
            cx={{ records, rows, onMenu, onParent: (id) => onPick(id), onJoin: () => onTab('talk'), joined: null }} />
        </div>
      )}
      {item && !sid && <div className="p-3 text-[12.5px] text-ink-faint">It belongs to no session: answering it is all there is to do.</div>}
    </Panel>
  )
}
