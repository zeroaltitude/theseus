// The Agenda (theseus-p17fd; horizon HZ9; the owner's tests H4 and H5): every horizon in one page. A week strip of
// reminders, routine runs, watches, deferred starts and milestones; the Routines table with each run's cost; the
// watches and the projects; and the wakes, moved here from Actions. It reads `work.agenda` and `wake.list` when the week
// shown changes or the push says the future moved, and the work board's views from the one `work.watch` the Threads
// views keep: a page left alone makes no request (lib/agendawatch.ts; test/agenda.test.ts reads it for a timer).
// The week and the selected mark are in the address (`?w=`, `?sel=`).
import { useMemo, useState } from 'react'
import { useSearchParams } from 'react-router'
import { CalendarClock, Eye, Flag, Hourglass, Repeat } from 'lucide-react'
import { useThreadBoard } from '@/lib/threadwatch'
import { useFuture, useRoutineRuns } from '@/lib/agendawatch'
import { useTick } from '@/lib/hooks'
import { allMarks, pastMarks, projectRows, routineRows, verbsFor, watchRows, weekWindow, type Mark } from '@/lib/agenda'
import { Btn, Empty, Panel } from '@/components/ui'
import { useAct } from '@/components/ConfirmCard'
import { WeekStrip } from '@/components/WeekStrip'
import { markLine } from '@/lib/agendawords'
import { RoutinesTable } from '@/components/RoutinesTable'
import { ProjectList, WatchList } from '@/components/FutureLists'
import { Wakes } from '@/components/Wakes'
import { clock } from '@/lib/format'
import { useNavigate } from 'react-router'

export default function Agenda() {
  const nav = useNavigate()
  const [params, setParams] = useSearchParams()
  const weeks = Math.max(-52, Math.min(52, parseInt(params.get('w') ?? '0', 10) || 0))
  const sel = params.get('sel')
  // The minute hand and "in 4m" move once a minute: a timer, not a request.
  const now = useTick(60_000)
  const win = useMemo(() => weekWindow(now, weeks), [Math.floor(now / 3_600_000), weeks]) // eslint-disable-line
  const board = useThreadBoard()
  const views = useMemo(() => Object.values(board.board.views), [board.board.views])
  const future = useMemo(() => views.filter((v) => ['wake', 'routine', 'watch', 'project'].includes(v.kind) || v.next_at_ms != null), [views])
  const held = useMemo(() => new Set(future.map((v) => v.id)), [future])
  const { agenda, wakes, error } = useFuture(win.from_ms, win.to_ms, held)
  const routines = useMemo(() => views.filter((v) => v.kind === 'routine'), [views])
  const runs = useRoutineRuns(routines)
  const rows = useMemo(() => routineRows(views), [views])
  const past = useMemo(() => routines.flatMap((v) => pastMarks(v.title, v.session_id ?? v.id, runs[v.session_id ?? v.id] ?? [])), [routines, runs])
  const marks = useMemo(() => allMarks(agenda?.lines ?? [], past, views), [agenda, past, views])
  const [hover, setHover] = useState<Mark | null>(null)
  const picked = sel ? marks.find((m) => m.key === sel) ?? null : null
  const set = (k: string, v: string | null) => setParams((p) => { if (v == null || v === '0') p.delete(k); else p.set(k, v); return p }, { replace: true })
  return (
    <div className="flex flex-col gap-3">
      {error && <div className="rounded-lg bg-fault/[0.06] px-3 py-2 text-[12px] text-fault ring-1 ring-fault/30">the agenda could not be read: {error}</div>}
      <Panel help="agenda.week" title={<>This week · {marks.length} item{marks.length === 1 ? '' : 's'}</>} icon={<CalendarClock size={13} />} bodyClassName="p-3"
        actions={<span className="num truncate text-[11px] text-ink-faint">{hover ? markLine(hover).split('\n')[0] : `${clock(win.from_ms)} → ${clock(win.to_ms - 1)}`}</span>}>
        <WeekStrip marks={marks} window={win} now={now} selected={sel} weeks={weeks} hidden={agenda?.more}
          onPick={(m) => set('sel', sel === m.key ? null : m.key)} onHover={setHover} onWeek={(d) => set('w', String(weeks + d))} />
        {picked && <Detail mark={picked} onClose={() => set('sel', null)} onOpen={(id) => nav(`/session/${id}`)} />}
      </Panel>
      <Panel help="agenda.routines" title={<>Routines · {rows.length}</>} icon={<Repeat size={13} />} bodyClassName="overflow-auto">
        <RoutinesTable rows={rows} views={routines} runs={runs} now={now} />
      </Panel>
      <div className="grid grid-cols-1 gap-3 xl:grid-cols-2">
        <Panel help="agenda.watches" title={<>Watches · {watchRows(views).length}</>} icon={<Eye size={13} />} bodyClassName="p-2"><WatchList rows={watchRows(views)} now={now} /></Panel>
        <Panel help="agenda.projects" title={<>Projects · {projectRows(views).length}</>} icon={<Flag size={13} />} bodyClassName="p-2"><ProjectList rows={projectRows(views)} /></Panel>
      </div>
      <Panel help="agenda.wakes" title={<>Wakes · {wakes.length}</>} icon={<Hourglass size={13} />} bodyClassName="p-2"><Wakes wakes={wakes} now={now} /></Panel>
    </div>
  )
}

/** A selected mark: what it is, and its verbs, each one call. */
function Detail({ mark, onClose, onOpen }: { mark: Mark; onClose: () => void; onOpen: (session: string) => void }) {
  const { busy, run } = useAct()
  const verbs = verbsFor(mark).filter((v) => !(v.key === 'resume' && !mark.paused) && !(v.key === 'pause' && mark.paused && mark.kind === 'run'))
  return (
    <div className="mt-3 rounded-lg px-3 py-2 ring-1 ring-gold/40" data-testid="mark-detail">
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <div className="text-[12.5px] text-ink">{mark.title}</div>
          <div className="num whitespace-pre-line text-[11px] text-ink-faint">{markLine(mark).split('\n').slice(0).join('\n').replace(`${mark.title} · `, '')}</div>
        </div>
        {(mark.run_session ?? mark.session_id) && <Btn tone="live" onClick={() => onOpen((mark.run_session ?? mark.session_id)!)}>Open</Btn>}
        <button onClick={onClose} className="text-[11px] text-ink-faint hover:text-ink">close</button>
      </div>
      {verbs.length > 0 ? (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {verbs.map((v) => (
            <Btn key={v.key} tone={v.tone} busy={busy === `${mark.key}:${v.key}`} onClick={() => {
              let params = v.params
              if (v.prompts) { const when = window.prompt(v.prompts); if (!when) return; params = { ...params, when } }
              void run(`${mark.key}:${v.key}`, v.method, params, v.ask)
            }}>{v.label}</Btn>
          ))}
        </div>
      ) : <Empty><span className="text-[11px]">nothing to do with this one</span></Empty>}
    </div>
  )
}
