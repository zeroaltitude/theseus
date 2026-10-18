// Tasks (theseus-ypnxt.6): what is working, what needs the owner, and what is done, each task with the one progress
// line; a New task box; Pause and resume; ended tasks with their report line and a search. The work board's live
// views (`work.watch`, shared with the Threads views) say what each task is doing now; `task.list` (read again on the
// push) holds the records, and the ended ones.
import { useEffect, useMemo, useRef, useState } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { CircleCheck, ListChecks, OctagonX, Pause, Play, Plus, Search, Send } from 'lucide-react'
import type { ConfirmRequest, SessionInfo, TaskListResult, TaskStartParams, TaskStartResult } from '@protocol'
import { useQueries } from '@tanstack/react-query'
import { call, useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { useThreadBoard } from '@/lib/threadwatch'
import { progressLine } from '@/lib/progressLine'
import { reportLineOf, rowsOf, split, type Section, type TaskRow } from '@/lib/taskboard'
import { ago, cn, short, usd } from '@/lib/format'
import { Btn, Empty, Panel, Pill, StatePill } from '@/components/ui'
import { ConfirmCard, useAct } from '@/components/ConfirmCard'

const PAGE = 50

export default function Tasks() {
  const { board } = useThreadBoard()
  const { data: tl } = useRpc<TaskListResult>('task.list', {}, 3000)
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 1500)
  const [query, setQuery] = useState('')
  const [more, setMore] = useState(PAGE)
  const [params] = useSearchParams()
  const [fresh, setFresh] = useState<string | null>(null)
  const focus = fresh ?? params.get('sel')
  const views = useMemo(() => Object.values(board.views), [board.views])
  // An ended task the board no longer holds has its report in its session: read the first fifty once each, the page
  // being opened for them (the daemon's `query` over the report line comes with task.list's own filters).
  const ended = useMemo(() => rowsOf(views, tl?.tasks ?? []).filter((r) => r.section === 'done' && !r.view?.report).slice(0, PAGE), [views, tl])
  const read = useQueries({ queries: ended.map((r) => ({ queryKey: ['task.report', r.id], staleTime: Infinity, queryFn: async () => reportLineOf((await call<{ nodes: { kind: string; text?: string }[] }>('session.history', { session_id: r.id, n: 3 })).nodes) ?? null })) })
  const lines = useMemo(() => Object.fromEntries(ended.flatMap((r, i) => (read[i]?.data ? [[r.id, read[i].data as string]] : []))), [ended, read])
  const rows = useMemo(() => rowsOf(views, tl?.tasks ?? [], lines), [views, tl, lines])
  const sections = useMemo(() => split(rows, query), [rows, query])
  const quiet = !rows.length
  return (
    <div className="flex flex-col gap-3" data-testid="tasks-page">
      <NewTask onStarted={setFresh} />
      <label className="flex items-center gap-2 rounded-lg px-3 py-1.5 text-[12.5px] ring-1 ring-line focus-within:ring-live/60">
        <Search size={13} className="text-ink-faint" />
        <input value={query} onChange={(e) => { setQuery(e.target.value); setMore(PAGE) }} placeholder="Search tasks by title or report…" aria-label="Search tasks"
          className="min-w-0 flex-1 bg-transparent text-ink outline-none placeholder:text-ink-faint" />
        {query && <span className="num text-[11px] text-ink-faint">{sections.needs.length + sections.working.length + sections.done.length} of {rows.length}</span>}
      </label>
      <Block section="needs" rows={sections.needs} confirms={cl?.confirms ?? []} focus={focus} empty="nothing waits for you" />
      <Block section="working" rows={sections.working} confirms={[]} focus={focus} empty={quiet ? 'no tasks yet: start one above' : 'nothing is working'} />
      <Block section="done" rows={sections.done.slice(0, more)} total={sections.done.length} onMore={() => setMore((n) => n + PAGE)} confirms={[]} focus={focus} empty="no task has ended" />
    </div>
  )
}

const TITLES: Record<Section, string> = { needs: 'Needs you', working: 'Working', done: 'Done' }

function Block({ section, rows, total, onMore, confirms, focus, empty }: { section: Section; rows: TaskRow[]; total?: number; onMore?: () => void; confirms: ConfirmRequest[]; focus: string | null; empty: string }) {
  const count = total ?? rows.length
  return (
    <Panel help="tasks.board" title={<>{TITLES[section]} · {count}</>} icon={section === 'needs' ? <CircleCheck size={13} /> : <ListChecks size={13} />} bodyClassName="p-1.5">
      {!rows.length && <Empty>{empty}</Empty>}
      {rows.map((r) => <TaskCard key={r.id} r={r} confirms={confirms.filter((c) => c.session_id === r.id)} focused={focus != null && (focus === r.id || focus === r.view?.id || focus === r.short)} />)}
      {onMore && count > rows.length && <button onClick={onMore} className="mx-2 my-1.5 text-[12px] text-live hover:underline">more ({count - rows.length} older)</button>}
    </Panel>
  )
}

function TaskCard({ r, confirms, focused }: { r: TaskRow; confirms: ConfirmRequest[]; focused: boolean }) {
  const ref = useRef<HTMLDivElement>(null)
  const now = useTick(15_000)
  const { busy, run } = useAct()
  useEffect(() => { if (focused) ref.current?.scrollIntoView({ block: 'center', behavior: 'smooth' }) }, [focused])
  const alive = r.section !== 'done'
  const line = r.view ? progressLine(r.view, now) : undefined
  const state = r.view?.state ?? r.info?.state
  const q = r.view?.question
  const cost = r.view?.cost ?? (r.info ? { spent_usd: r.info.spent_usd, limit_usd: r.info.limit_usd } : undefined)
  return (
    <div ref={ref} data-task={r.id} data-short={r.short} className={cn('rounded-lg border-l-2 px-3 py-2.5 transition-shadow', r.section === 'needs' ? 'border-wait bg-wait/[0.04]' : r.section === 'working' ? 'border-live' : 'border-line', focused && 'ring-1 ring-live shadow-[0_0_14px_var(--color-live)]/30')}>
      <div className="flex flex-wrap items-center gap-2">
        {r.paused ? <Pill tone="wait" title="held by the owner: resume lets it go on">⏸ paused</Pill> : <StatePill state={state} />}
        <Link to={`/session/${r.id}`} className="min-w-0 flex-1 truncate text-[13px] font-medium text-ink hover:text-live" title={r.title}>{r.title}</Link>
        {r.place && <Pill tone="idle" title={r.target ?? r.place}>{r.place}</Pill>}
        <span className="num text-[10.5px] text-ink-faint">{r.short}</span>
      </div>
      {alive && (line || r.view?.doing) && (
        <div className="num mt-1 flex flex-wrap gap-x-3 text-[11.5px]">
          {line && <span className="text-ink" data-testid="progress-line">{line}</span>}
          {r.view?.doing && r.section !== 'needs' && <span className="truncate text-ink-dim">{r.view.doing.label}</span>}
        </div>
      )}
      {r.section === 'done' && (
        <div className="mt-1 text-[12px] text-ink-dim">
          {r.view?.report?.outcome && r.view.report.outcome !== 'done' && <span className="mr-1.5 text-fault">{r.view.report.outcome}</span>}
          {r.reportLine ?? <span className="text-ink-faint">no report line</span>}
        </div>
      )}
      <div className="num mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-[10.5px] text-ink-faint">
        {r.section === 'done' && cost && <span className="text-money">{usd(cost.spent_usd)}</span>}
        <span title="when it last changed">{ago(r.activityMs, now)}</span>
        {r.info && r.info.waiting_on && alive && <span>waits on {r.info.waiting_on}</span>}
        <span className="ml-auto flex items-center gap-2">
          <Link to={`/session/${r.id}`} className="text-live hover:underline" title="the task's page: its transcript, and a composer that messages it (Enter sends)">{alive ? 'open · join' : 'open'}</Link>
          {alive && r.section === 'working' && (
            <Btn tone="wait" busy={busy === r.id} onClick={() => run(r.id, r.paused ? 'task.resume' : 'task.pause', { task: r.id })} title={r.paused ? 'Let it go on' : 'Hold it: it takes no new step until resumed'}>
              {r.paused ? <><Play size={12} /> Resume</> : <><Pause size={12} /> Pause</>}
            </Btn>
          )}
          {alive && <Btn tone="fault" busy={busy === `${r.id}:stop`} onClick={() => run(`${r.id}:stop`, 'task.cancel', { task: r.id }, `Stop task ${r.short} (${r.title})? Its jobs stop, and it reports that it was cancelled.`)}><OctagonX size={12} /> Stop</Btn>}
        </span>
      </div>
      {r.section === 'needs' && (
        <div className="mt-2 flex flex-col gap-2">
          {confirms.map((c) => <ConfirmCard key={c.correlation_id} c={c} here />)}
          {!confirms.length && q && <div className="rounded-md bg-black/20 px-3 py-2 text-[12.5px] text-ink ring-1 ring-line">{q.prompt}{q.choices?.length ? <span className="num ml-2 text-ink-faint">{q.choices.join(' · ')}</span> : null}<span className="ml-2 text-ink-faint">answer it on the task’s page</span></div>}
        </div>
      )}
    </div>
  )
}

/** The New task box: a brief, a title, a budget, and where it goes. It calls `task.start`, which opens the task with
 *  no model turn; the row it makes is scrolled to, and a refusal shows the daemon's own words. */
function NewTask({ onStarted }: { onStarted: (taskId: string) => void }) {
  const nav = useNavigate()
  const [brief, setBrief] = useState('')
  const [title, setTitle] = useState('')
  const [budget, setBudget] = useState('')
  const [home, setHome] = useState('')
  const [busy, setBusy] = useState(false)
  const [said, setSaid] = useState<{ ok: boolean; text: string } | null>(null)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const open = (sl?.sessions ?? []).filter((s) => s.kind === 'conversation' && !s.retired)
  const dollars = budget.trim() === '' ? undefined : Number(budget.trim().replace(/^\$/, ''))
  const bad = dollars !== undefined && !(dollars > 0)
  const start = async () => {
    const p: TaskStartParams = { brief: brief.trim(), ...(title.trim() && { title: title.trim() }), ...(home && { home }), ...(dollars !== undefined && { budget_usd: dollars }) }
    setBusy(true)
    try {
      const r = await call<TaskStartResult>('task.start', p)
      setSaid({ ok: true, text: `started ${r.short}${p.title ? ` · ${p.title}` : ''} · ${usd(r.limit_usd)}` })
      setBrief(''); setTitle(''); setBudget('')
      onStarted(r.task_id)
      nav(`/tasks?sel=${r.task_id}`, { replace: true })
    } catch (e: any) {
      setSaid({ ok: false, text: e?.message ?? String(e) })
    } finally { setBusy(false) }
  }
  const field = 'rounded-md bg-black/20 px-2 py-1.5 text-[12.5px] text-ink outline-none ring-1 ring-line placeholder:text-ink-faint focus:ring-live/60'
  return (
    <Panel help="tasks.new" title="New task" icon={<Plus size={13} />} bodyClassName="flex flex-col gap-2 p-3">
      <textarea value={brief} onChange={(e) => setBrief(e.target.value)} rows={3} placeholder="What should it do? A task works on its own and reports back." aria-label="Brief"
        onKeyDown={(e) => { if (e.key === 'Enter' && (e.metaKey || e.ctrlKey) && brief.trim() && !bad && !busy) void start() }} className={cn(field, 'resize-y')} />
      <div className="flex flex-wrap items-center gap-2">
        <input value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Title (the brief’s first line)" aria-label="Title" className={cn(field, 'min-w-48 flex-1')} />
        <input value={budget} onChange={(e) => setBudget(e.target.value)} placeholder="Budget $ (a quarter of what’s left)" aria-label="Budget in dollars" inputMode="decimal" className={cn(field, 'w-52', bad && 'ring-fault/60')} />
        <select value={home} onChange={(e) => setHome(e.target.value)} aria-label="Where it goes" className={cn(field, 'max-w-64')}>
          <option value="">Home conversation</option>
          {open.map((s) => <option key={s.session_id} value={s.session_id}>{s.title || s.label || short(s.session_id)}</option>)}
        </select>
        <Btn tone="live" busy={busy} onClick={() => { if (brief.trim() && !bad) void start() }} title="Start it now, with no model turn (Ctrl+Enter)"><Send size={12} /> Start task</Btn>
      </div>
      {said && <div role="status" className={cn('num text-[11.5px]', said.ok ? 'text-ok' : 'text-fault')}>{said.text}</div>}
    </Panel>
  )
}
