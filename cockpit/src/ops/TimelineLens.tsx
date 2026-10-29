// The Timeline lens (theseus-jid7t.5; the Agenda's page, theseus-p17fd, moved here and grown): every horizon on one
// week strip beside both of the owner's calendars. The week strip v2 (`WeekStripV2`: the calendar's events, the ran
// bars and Theseus's marks), its legend with each calendar's state and fix, the clashes, a Day zoom, the table of the
// same items, and under it the Routines, Watches, Projects and Wakes panels, the same components unchanged. A selected
// mark's facts and verbs show under the strip (`MarkDetail`, the Agenda's `Detail`); a selected event's in the
// inspector (`EventDetail`).
//
// It reads `work.agenda` and `wake.list` when the week shown changes or the push says the future moved, and the work
// board's views from the one `work.watch` the Threads views keep: a page left alone makes no request
// (lib/agendawatch.ts). The calendar is the page's one read (`ops/calendarwatch.ts`), passed in. The week, the zoom and
// the selection are in the address (`?w=`, `?day=`, `?sel=`); redact mode (`?redact=1`) masks a sensitive calendar on
// every surface drawn here, through `ops/calendar.ts`.
import { useEffect, useMemo, useState } from 'react'
import { CalendarClock, Eye, Flag, Hourglass, Lock, Repeat, Undo2 } from 'lucide-react'
import { useThreadBoard } from '@/lib/threadwatch'
import { useFuture, useRoutineRuns } from '@/lib/agendawatch'
import { useTick } from '@/lib/hooks'
import { useTableView } from '@/lib/chartview'
import { allMarks, pastMarks, projectRows, routineRows, verbsFor, watchRows, weekWindow, type Mark, type Verb as MarkVerb } from '@/lib/agenda'
import { markLine } from '@/lib/agendawords'
import { cn, stamp } from '@/lib/format'
import { toneHex } from '@/lib/taxonomy'
import { Empty, Panel, Segmented } from '@/components/ui'
import { RoutinesTable } from '@/components/RoutinesTable'
import { ProjectList, WatchList } from '@/components/FutureLists'
import { Wakes } from '@/components/Wakes'
import type { CalendarWindow } from '@protocol'
import { eventMatches, hatch, stateOf } from './calendar'
import type { CalendarRead } from './calendarwatch'
import { calEvents, clashes, meetingCount, tableRows, timelineOrder, zoomed } from './timeline'
import { taskBlocks, taskLine, taskVerbs, type TaskBlock } from './taskblocks'
import type { Call, Drop } from './drag'
import { WeekStripV2 } from './WeekStripV2'
import { useFullGraph } from '@/components/FullGraph'
import { useOpsAct } from './act'
import { ActBtn } from './ActBtn'
import { FIELD, Sheet } from './Sheet'

/** What the lens tells the page: its marks (for the Horizon keys), the order `j`/`k` step, the command bar's two
 *  counters, and the search's "n of m". */
export interface TimelineReport { marks: readonly Mark[]; order: readonly string[]; meetings: number; clashes: number; n: number; of: number; dayOf: (key: string | null) => number }

export function TimelineLens({ weeks, day, sel, redact, q, cal, past, onSel, onWeek, onToday, onZoom, onReport, onOpen }: {
  weeks: number; day: number | null; sel: string | null; redact: boolean; q: string; cal: CalendarRead; past: boolean
  onSel: (key: string | null) => void; onWeek: (delta: number) => void; onToday: () => void; onZoom: (day: number | null) => void
  onReport: (r: TimelineReport) => void; onOpen: (session: string) => void
}) {
  // The minute hand and "in 4m" move once a minute: a timer, not a request.
  const now = useTick(60_000)
  const fullWindow = useFullGraph('Timeline')
  const win = useMemo(() => weekWindow(now, weeks), [Math.floor(now / 3_600_000), weeks]) // eslint-disable-line
  const shown = useMemo(() => zoomed(win, day), [win, day])
  const board = useThreadBoard()
  const views = useMemo(() => Object.values(board.board.views), [board.board.views])
  const future = useMemo(() => views.filter((v) => ['wake', 'routine', 'watch', 'project'].includes(v.kind) || v.next_at_ms != null), [views])
  const held = useMemo(() => new Set(future.map((v) => v.id)), [future])
  const { agenda, wakes, error } = useFuture(win.from_ms, win.to_ms, held)
  const routines = useMemo(() => views.filter((v) => v.kind === 'routine'), [views])
  const runs = useRoutineRuns(routines)
  const rows = useMemo(() => routineRows(views), [views])
  const pastRuns = useMemo(() => routines.flatMap((v) => pastMarks(v.title, v.session_id ?? v.id, runs[v.session_id ?? v.id] ?? [])), [routines, runs])
  const marks = useMemo(() => allMarks(agenda?.lines ?? [], pastRuns, views), [agenda, pastRuns, views])
  // Tasks beside the meetings (theseus-1t4q8): the board's tasks, the routines' runs and the agenda's marks, as blocks. The
  // minute tick grows a live one; no read of its own.
  const tasks = useMemo(() => taskBlocks(views, runs, marks, now), [views, runs, marks, now])
  const taskOnly = useMemo(() => { const marked = new Set(marks.map((m) => m.key)); return tasks.filter((t) => !marked.has(t.key)) }, [tasks, marks])
  const events = useMemo(() => calEvents(cal.calendars), [cal.calendars])
  const clash = useMemo(() => clashes(marks, events), [marks, events])
  const inShown = useMemo(() => ({
    marks: marks.filter((m) => (m.end_ms ?? m.at_ms) >= shown.from_ms && m.at_ms < shown.to_ms),
    events: events.filter((e) => e.ev.start_ms < shown.to_ms && e.ev.end_ms > shown.from_ms),
    tasks: taskOnly.filter((t) => t.to_ms >= shown.from_ms && t.from_ms < shown.to_ms),
  }), [marks, events, taskOnly, shown])
  const hit = useMemo(() => {
    if (!q.trim()) return null
    const words = q.toLowerCase().split(/\s+/).filter(Boolean)
    const out = new Set<string>()
    for (const m of inShown.marks) if (words.every((w) => m.title.toLowerCase().includes(w))) out.add(m.key)
    for (const e of inShown.events) if (eventMatches(e.ev, e.cal, redact, q)) out.add(`ev:${e.ev.key}`)
    for (const t of inShown.tasks) if (words.every((w) => t.title.toLowerCase().includes(w))) out.add(t.key)
    return out
  }, [q, inShown, redact])
  const matches = (k: string) => !hit || hit.has(k)
  const total = inShown.marks.length + inShown.events.length + inShown.tasks.length
  const found = hit ? hit.size : total
  useEffect(() => {
    onReport({
      marks, order: timelineOrder(marks, events, shown, tasks), meetings: meetingCount(events, shown), clashes: clash.count, n: found, of: total,
      dayOf: (key) => { const at = key?.startsWith('ev:') ? events.find((e) => `ev:${e.ev.key}` === key)?.ev.start_ms : marks.find((m) => m.key === key)?.at_ms ?? taskOnly.find((t) => t.key === key)?.from_ms; const d = win.days.find((x) => at != null && at >= x.start_ms && at < x.end_ms); return d ? d.index : -1 },
    })
  }, [marks, events, tasks, taskOnly, shown, clash.count, found, total, win]) // eslint-disable-line react-hooks/exhaustive-deps
  const [hover, setHover] = useState<string | null>(null)
  const [table, setTable] = useTableView('ops.timeline')
  const picked = sel ? marks.find((m) => m.key === sel) ?? null : null
  const pickedTask = sel && !picked ? taskOnly.find((t) => t.key === sel) ?? null : null
  // A mark that a block stands for says the block's words beside its own (the cost and span the bar never said).
  const pickedBlock = picked ? tasks.find((t) => t.key === picked.key) ?? null : null
  const { send, settle, on } = useOpsAct()
  const [toast, setToast] = useState<{ text: string; undo?: Call; bad?: boolean } | null>(null)
  useEffect(() => { if (!toast) return; const t = window.setTimeout(() => setToast(null), toast.undo ? 10_000 : 5000); return () => window.clearTimeout(t) }, [toast])
  const drop = async (x: Drop & { method: string }) => {
    if ('refused' in x) return
    const r = await send('drag', x.method, x.params)
    void settle()
    setToast(r.ok ? { text: x.words, undo: x.undo(r.result) } : { text: r.words, bad: true })
  }
  const undo = async (c: Call) => {
    const r = await send('undo', c.method, c.params)
    void settle()
    setToast(r.ok ? { text: 'put back as it was' } : { text: r.words, bad: true })
  }
  const title = day != null && shown.days[0] ? `This week · ${total} item${total === 1 ? '' : 's'} · ${shown.days[0].label}` : `This week · ${total} item${total === 1 ? '' : 's'}`
  return (
    <div className="flex flex-col gap-3" data-testid="timeline-lens">
      {error && <div className="rounded-lg bg-fault/[0.06] px-3 py-2 text-[12px] text-fault ring-1 ring-fault/30">the agenda could not be read: {error}</div>}
      {cal.error && <div className="rounded-lg bg-fault/[0.06] px-3 py-2 text-[12px] text-fault ring-1 ring-fault/30">the calendar could not be read: {cal.error} Theseus’s own marks still draw.</div>}
      <Panel help="agenda.week" title={title} icon={<CalendarClock size={13} />} bodyClassName={cn('p-3', fullWindow.full && 'overflow-auto')} {...fullWindow.panel}
        actions={<span className="flex min-w-0 items-center gap-2">
          {fullWindow.button}
          <span className="num truncate text-[11px] text-ink-faint">{hover ?? (picked ? markLine(picked).split('\n')[0] : pickedTask ? taskLine(pickedTask, now) : `${stamp(shown.from_ms)} → ${stamp(shown.to_ms - 1)}`)}</span>
          {redact && <span className="rounded px-1.5 text-[10px] font-semibold uppercase tracking-wider text-wait ring-1 ring-wait/40" title="⇧R shows them">redacted</span>}
          <Segmented value={table ? 'table' : 'chart'} options={['chart', 'table'] as const} onChange={(v) => setTable(v === 'table')} />
        </span>}>
        <Legend cals={cal.calendars} read={cal.read} more={agenda?.more} />
        {table
          ? <TimelineTable rows={tableRows(marks, events, shown, redact, clash, markLine, taskOnly, now)} sel={sel} onSel={onSel} matches={matches} />
          : <WeekStripV2 window={shown} marks={marks} events={events} tasks={tasks} cals={cal.calendars} views={views} runs={runs} now={now} past={past} selected={sel} clash={clash}
              redact={redact} matches={matches} zoom={day != null} weeks={weeks} full={fullWindow.full} onPick={(k) => onSel(sel === k ? null : k)} onHover={setHover}
              onWeek={onWeek} onToday={onToday} onZoom={onZoom} onDrop={(x) => void drop(x)} />}
        {picked && <MarkDetail mark={picked} block={pickedBlock} now={now} onClose={() => onSel(null)} onOpen={onOpen} />}
        {pickedTask && <TaskDetail block={pickedTask} now={now} onClose={() => onSel(null)} onOpen={onOpen} />}
        {sel?.startsWith('ev:') && <p className="mt-2 text-[11px] text-ink-faint">The selected event’s facts and verbs are in the inspector (<span className="kbd">]</span> unfolds it).</p>}
      </Panel>
      <Panel help="agenda.routines" title={<>Routines · {rows.length}</>} icon={<Repeat size={13} />} bodyClassName="overflow-auto">
        <RoutinesTable rows={rows} views={routines} runs={runs} now={now} />
      </Panel>
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
        <Panel help="agenda.watches" title={<>Watches · {watchRows(views).length}</>} icon={<Eye size={13} />} bodyClassName="p-2"><WatchList rows={watchRows(views)} now={now} /></Panel>
        <Panel help="agenda.projects" title={<>Projects · {projectRows(views).length}</>} icon={<Flag size={13} />} bodyClassName="p-2"><ProjectList rows={projectRows(views)} /></Panel>
      </div>
      <Panel help="agenda.wakes" title={<>Wakes · {wakes.length}</>} icon={<Hourglass size={13} />} bodyClassName="p-2"><Wakes wakes={wakes} now={now} /></Panel>
      {toast && (
        <div role="status" className={cn('fixed bottom-24 left-1/2 z-30 flex -translate-x-1/2 items-center gap-3 rounded-lg bg-hull/95 px-3 py-1.5 text-[12px] shadow-xl ring-1', toast.bad ? 'text-fault ring-fault/50' : 'text-ink ring-line-strong')}>
          {toast.text}
          {toast.undo && <ActBtn verb="undo" on={on} onClick={() => { const c = toast.undo!; void undo(c) }} title="put it back exactly as it was"><Undo2 size={11} /> Undo</ActBtn>}
        </div>
      )}
    </div>
  )
}

/** The legend: each calendar's material and lock, its state in its tone, its one fix; then Theseus's own glyphs. */
function Legend({ cals, read, more }: { cals: readonly CalendarWindow[]; read: boolean; more?: boolean }) {
  return (
    <div className="mb-2 flex flex-col gap-1.5" data-testid="timeline-legend">
      <div className="flex flex-wrap items-start gap-x-4 gap-y-1 text-[11px]">
        {read && !cals.length && <span className="text-ink-faint">no calendar is set up: [google] calendars in the config adds one</span>}
        {cals.map((c) => {
          const s = stateOf(c)
          return (
            <span key={c.id} className="flex items-center gap-1.5" data-state={c.status}>
              <span className="inline-block h-3 w-5 rounded-[3px]" aria-hidden style={{ background: s.draws === 'lane' || s.draws === 'none' ? 'transparent' : hatch(c.color, c.sensitive, c.status === 'busy_only'), boxShadow: s.draws === 'none' ? undefined : `inset 2px 0 0 ${c.color}`, outline: s.draws === 'lane' ? `1px dashed ${toneHex.wait}` : s.draws === 'none' ? `1px solid ${c.color}55` : undefined }} />
              <span className="text-ink">{c.label}</span>{c.sensitive && <Lock size={10} className="text-ink-dim" aria-label="sensitive" />}
              <span style={{ color: toneHex[s.tone] }}>{s.tone === 'ok' ? '✓' : s.tone === 'wait' ? '◌' : '✗'} {s.words}</span>
            </span>
          )
        })}
      </div>
      {cals.filter((c) => stateOf(c).fix).map((c) => (
        <details key={c.id} className="text-[11px] text-ink-faint" data-fix={c.status}>
          <summary className="cursor-pointer select-none hover:text-ink">{c.label}: how to fix it</summary>
          <p className="mt-1 max-w-[760px] leading-snug text-ink-dim">{stateOf(c).fix}</p>
        </details>
      ))}
      <div className="flex flex-wrap gap-x-3 gap-y-0.5 text-[11px] text-ink-faint">
        <span style={{ color: toneHex.live }}>● reminder</span><span style={{ color: toneHex.think }}>○ check-in</span><span style={{ color: toneHex.money }}>│ run due</span>
        <span style={{ color: toneHex.ok }}>✓ ok</span><span style={{ color: toneHex.fault }}>✗ failed</span><span style={{ color: toneHex.idle }}>– skipped</span>
        <span style={{ color: toneHex.wait }}>▷ task starts · ⚑ milestone</span><span style={{ color: toneHex.think }}>▬ watch</span>
        <span className="text-wait">◆ clash</span><span>▏ran</span>
        <span style={{ color: toneHex.live }}>▮ task going</span><span style={{ color: toneHex.wait }}>▯ task planned</span><span style={{ color: toneHex.money }}>↻ recurring</span>
        {more && <span className="ml-auto text-wait">the daemon would say more than the strip holds: busy series are cut</span>}
      </div>
    </div>
  )
}

/** The strip's table of the same items: every mark and event of the window, by time. */
function TimelineTable({ rows, sel, onSel, matches }: { rows: ReturnType<typeof tableRows>; sel: string | null; onSel: (k: string | null) => void; matches: (k: string) => boolean }) {
  if (!rows.length) return <Empty><span className="text-[11px]">nothing in this window</span></Empty>
  return (
    <table className="num w-full text-[11px]" data-testid="timeline-table">
      <thead className="text-[10px] uppercase tracking-wider text-ink-faint"><tr><th className="px-2 text-left">when</th><th className="px-2 text-left">what</th><th className="px-2 text-left">calendar</th><th className="px-2 text-left">item</th></tr></thead>
      <tbody>
        {rows.filter((r) => matches(r.key)).map((r) => (
          <tr key={r.key} onClick={() => onSel(sel === r.key ? null : r.key)} className={cn('cursor-pointer border-t border-line/50 hover:bg-white/5', sel === r.key && 'bg-gold/10')}>
            <td className="px-2 text-ink-dim">{stamp(r.at_ms)}</td><td className="px-2 text-ink-faint">{r.clash ? '◆ ' : ''}{r.what}</td><td className="px-2 text-ink-dim">{r.calendar}</td><td className="px-2 text-ink">{r.line}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

/** A selected mark: what it is, and its verbs, each one call (the Agenda's `Detail`, through `useOpsAct`). */
function MarkDetail({ mark, block, now, onClose, onOpen }: { mark: Mark; block: TaskBlock | null; now: number; onClose: () => void; onOpen: (session: string) => void }) {
  const { busy, act, on } = useOpsAct()
  const verbs = verbsFor(mark).filter((v) => !(v.key === 'resume' && !mark.paused) && !(v.key === 'pause' && mark.paused && mark.kind === 'run'))
  const session = mark.run_session ?? mark.session_id
  // A verb that needs a time (Move…) asks in a sheet, as every act on the page that needs words does (ops-act).
  const [asking, setAsking] = useState<MarkVerb | null>(null)
  const [when, setWhen] = useState('')
  return (
    <div className="mt-3 rounded-lg px-3 py-2 ring-1 ring-gold/40" data-testid="mark-detail">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="text-[12.5px] text-ink">{mark.title}</div>
          <div className="num whitespace-pre-line text-[11px] text-ink-faint">{markLine(mark).replace(`${mark.title} · `, '')}</div>
          {block && <div className="num text-[11px] text-ink-faint" data-testid="block-line">{taskLine(block, now).replace(`${block.title} · `, '')}</div>}
        </div>
        {session && <button onClick={() => onOpen(session)} title="⏎: its session" className="rounded-md px-2 py-1 text-[11.5px] text-live ring-1 ring-live/40 hover:brightness-125">Open</button>}
        <button onClick={onClose} className="text-[11px] text-ink-faint hover:text-ink">close</button>
      </div>
      {verbs.length > 0 ? (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {verbs.map((v) => (
            <ActBtn key={v.key} verb="mark" on={on} tone={v.tone} busy={busy === `${mark.key}:${v.key}`} onClick={() => {
              if (v.prompts) { setWhen(''); setAsking(v); return }
              void act('mark', `${mark.key}:${v.key}`, v.method, v.params, v.ask)
            }}>{v.label}</ActBtn>
          ))}
        </div>
      ) : <Empty><span className="text-[11px]">nothing to do with this one</span></Empty>}
      {asking && (
        <Sheet title={`${asking.label.replace(/…$/, '')} ${mark.title}`} note={asking.prompts} verb="mark" submit={asking.label.replace(/…$/, '')} busy={busy === `${mark.key}:${asking.key}`}
          disabled={!when.trim()} onClose={() => setAsking(null)}
          onSubmit={() => void act('mark', `${mark.key}:${asking.key}`, asking.method, { ...asking.params, when: when.trim() }, asking.ask).then((ok) => { if (ok) setAsking(null) })}>
          <input autoFocus value={when} onChange={(e) => setWhen(e.target.value)} placeholder="10m, tomorrow 9:00, fri 14:00" aria-label="When" className={FIELD} />
        </Sheet>
      )}
    </div>
  )
}

/** A selected task block that no mark stands for (a task going or gone): its facts and its verbs, each one call. */
function TaskDetail({ block, now, onClose, onOpen }: { block: TaskBlock; now: number; onClose: () => void; onOpen: (session: string) => void }) {
  const { busy, act, on } = useOpsAct()
  const verbs = taskVerbs(block)
  return (
    <div className="mt-3 rounded-lg px-3 py-2 ring-1 ring-gold/40" data-testid="task-detail">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="text-[12.5px] text-ink">{block.title}</div>
          <div className="num whitespace-pre-line text-[11px] text-ink-faint">{taskLine(block, now).replace(`${block.title} · `, '')}</div>
          <div className="num text-[11px] text-ink-faint">{stamp(block.from_ms)} → {block.live ? 'now' : stamp(block.to_ms)}</div>
        </div>
        {block.session_id && <button onClick={() => onOpen(block.session_id!)} title="⏎: its session" className="rounded-md px-2 py-1 text-[11.5px] text-live ring-1 ring-live/40 hover:brightness-125">Open</button>}
        <button onClick={onClose} className="text-[11px] text-ink-faint hover:text-ink">close</button>
      </div>
      {verbs.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {verbs.map((v) => <ActBtn key={v.key} verb="mark" on={on} tone={v.tone} busy={busy === `${block.key}:${v.key}`} onClick={() => void act('mark', `${block.key}:${v.key}`, v.method, v.params, v.ask)}>{v.label}</ActBtn>)}
        </div>
      )}
    </div>
  )
}
