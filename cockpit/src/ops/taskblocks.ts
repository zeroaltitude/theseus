// Tasks on the agenda timeline (theseus-1t4q8): a task beside the calendar's meetings as a block of its own. A running
// task is a live block from its start to now (the lens's minute tick grows it); a finished one keeps its real span; a
// deferred one is a future block at its planned time; a routine's occurrences are recurring blocks, and its runs are
// recurring ones that happened. Nothing is invented here: the span of a plan is the forecast the daemon serves for the
// task (`WorkView.forecast`) or the routine's last run (`RoutineView.last`); the times are the daemon's own
// (`work.agenda`, `routine.runs`, the board's views). A block that derives from a mark carries the mark's key, so the
// strip's selection, the keys and the verbs reach it through the mark; a task with no mark has a `task:<id>` key of its
// own. Pure: `npm test` runs it (test/ops-taskblocks.test.ts).
import type { RunRow, WorkView } from '@protocol'
import { outcomeOf, wallHour, type Mark, type Tone, type Verb, type Window } from '../lib/agenda.ts'
import { lineUsd, progressLine } from '../lib/progressLine.ts'
import { span, when } from '../lib/figures.ts'

export type TaskBlockKind = 'running' | 'finished' | 'scheduled' | 'recurring'

/** One task on the strip. */
export interface TaskBlock {
  key: string
  kind: TaskBlockKind
  /** The task's, routine's or wake's id the verbs act on. */
  id: string
  session_id?: string
  title: string
  from_ms: number
  to_ms: number
  /** Going now: its end is the clock. */
  live: boolean
  /** Its end is a plan (a forecast, a routine's last run), not a fact. */
  planned: boolean
  /** An occurrence of a routine, coming or happened. */
  recurring: boolean
  occurrence?: number
  /** The board's state, or how the run ended. */
  state: string
  tone: Tone
  paused?: boolean
  cost_usd?: number
  limit_usd?: number
  /** The one progress line (`lib/progressLine.ts`), for a task the board holds. */
  progress?: string
  /** A routine's schedule as it was written. */
  schedule?: string
}

/** A deferred task with no forecast is drawn this long: half an hour. */
export const DEFAULT_PLANNED_MS = 30 * 60_000
/** A routine occurrence with no run to go by is drawn this long. */
export const DEFAULT_RECURRING_MS = 15 * 60_000
/** The longest a plan is drawn: a recurring routine whose last run took a day is not a day-long block. */
const MAX_PLANNED_MS = 6 * 3_600_000
/** The smallest block drawn, as a share of the day (15 minutes), so a one-minute task is still a target. */
const MIN_SHARE = 0.25 / 24

const ENDED: readonly string[] = ['done', 'failed', 'cancelled', 'expired']

const stateTone = (state: string, paused: boolean | undefined, live: boolean): Tone =>
  paused && live ? 'wait' : state === 'failed' ? 'fault' : state === 'cancelled' || state === 'expired' ? 'idle' : live ? 'live' : 'ok'

const clampPlan = (ms: number) => Math.min(MAX_PLANNED_MS, Math.max(DEFAULT_RECURRING_MS, ms))

/** Every task block of the board, the routines' runs and the agenda's marks, by start. `views` are the work board's,
 *  `runs` are `routine.runs` by routine, `marks` are the strip's marks (`work.agenda` and the runs), `now` the clock. */
export function taskBlocks(views: readonly WorkView[], runs: Readonly<Record<string, readonly RunRow[]>>, marks: readonly Mark[], now: number): TaskBlock[] {
  const out: TaskBlock[] = []
  const byId = new Map(views.map((v) => [v.id, v]))
  const routineOf = new Map<string, WorkView>()
  for (const v of views) if (v.kind === 'routine') routineOf.set(v.session_id ?? v.id, v)
  const runTasks = new Set(Object.values(runs).flatMap((rs) => rs.map((r) => r.task)))

  // A task that has started: from its start to its end, or to now while it goes. A deferred one has not started; a
  // routine's run is its routine's block, below.
  for (const v of views) {
    if (v.kind !== 'task' || v.state === 'queued' || v.started_at_ms <= 0 || v.next_at_ms != null) continue
    if (v.session_id && runTasks.has(v.session_id)) continue
    const ended = ENDED.includes(v.state)
    const to = ended ? Math.max(v.ended_at_ms ?? v.state_since_ms, v.started_at_ms) : Math.max(now, v.started_at_ms)
    out.push({
      key: `task:${v.id}`, kind: ended ? 'finished' : 'running', id: v.id, session_id: v.session_id, title: v.title || v.id,
      from_ms: v.started_at_ms, to_ms: to, live: !ended, planned: false, recurring: false, state: v.state, tone: stateTone(v.state, v.paused, !ended), paused: v.paused,
      cost_usd: v.cost?.spent_usd, limit_usd: v.cost?.limit_usd, progress: progressLine(v, now),
    })
  }

  // A routine's runs: the one going is live, the rest keep their real span. Their keys are the past marks'.
  for (const [routineId, rs] of Object.entries(runs)) {
    const routine = routineOf.get(routineId)
    const title = routine?.title ?? routineId
    for (const r of rs) {
      const o = outcomeOf(r)
      const live = o === 'running'
      out.push({
        key: `past:${routineId}:${r.task}`, kind: live ? 'running' : 'finished', id: routineId, session_id: r.task, title: `${title} · run ${r.run}`,
        from_ms: r.started_ms, to_ms: live ? Math.max(now, r.started_ms) : r.started_ms + r.duration_ms, live, planned: false, recurring: true, occurrence: r.run,
        state: o, tone: o === 'ok' ? 'ok' : o === 'failed' ? 'fault' : o === 'skipped' ? 'idle' : 'live', cost_usd: r.cost_usd, schedule: routine?.routine?.schedule,
      })
    }
  }

  // What is coming: a deferred task's start and a routine's occurrences, as the daemon serves them.
  for (const m of marks) {
    if (m.kind === 'start') {
      const v = byId.get(m.id)
      const f = v?.forecast
      const plan = f && f.n > 0 && f.min_p50 > 0 ? f.min_p50 * 60_000 : DEFAULT_PLANNED_MS
      out.push({
        key: m.key, kind: 'scheduled', id: m.id, session_id: m.session_id ?? v?.session_id, title: m.title || v?.title || m.id,
        from_ms: m.at_ms, to_ms: m.at_ms + plan, live: false, planned: true, recurring: false, state: 'scheduled', tone: 'wait',
      })
    } else if (m.kind === 'run') {
      const v = byId.get(m.id)
      // A stopped, ended, held or over routine has no future: a read that has not caught up draws none of it.
      if (v && v.kind === 'routine' && (ENDED.includes(v.state) || v.paused || (v.series?.until_ms != null && m.at_ms > v.series.until_ms))) continue
      const last = v?.routine?.last?.duration_ms ?? 0
      out.push({
        key: m.key, kind: 'recurring', id: m.id, session_id: m.session_id ?? v?.session_id, title: m.title || v?.title || m.id,
        from_ms: m.at_ms, to_ms: m.at_ms + (last > 0 ? clampPlan(last) : DEFAULT_RECURRING_MS), live: false, planned: true, recurring: true, occurrence: m.occurrence,
        state: 'recurring', tone: 'money', schedule: v?.routine?.schedule,
      })
    }
  }
  return out.sort((a, b) => a.from_ms - b.from_ms || (a.key < b.key ? -1 : 1))
}

// ---------------------------------------------------------------- words

/** A block in a line: the tooltip, the hover, the table's cell. Every number the strip's marks and bars said is here. */
export function taskLine(b: TaskBlock, now: number): string {
  const head = b.title
  const dur = span(Math.max(0, b.to_ms - b.from_ms))
  const what = b.kind === 'running' ? `${b.paused ? 'paused' : 'running'} ${dur}`
    : b.kind === 'finished' ? `${b.state} · took ${dur}`
    : b.kind === 'scheduled' ? `starts ${when(b.from_ms, now)} · about ${dur}`
    : `recurring${b.occurrence != null ? ` · run ${b.occurrence}` : ''} · ${when(b.from_ms, now)} · about ${dur}`
  const sched = b.recurring && b.schedule ? `${b.kind === 'recurring' ? '' : 'recurring · '}${b.schedule}` : ''
  const cost = b.progress ?? (b.cost_usd != null ? lineUsd(b.cost_usd) : '')
  return [head, what, sched, cost].filter(Boolean).join(' · ')
}

/** The verbs a block carries of its own: a live task pauses or resumes and stops. A block that derives from a mark has
 *  the mark's (`verbsFor`); a finished task opens its session and nothing more. */
export function taskVerbs(b: TaskBlock): Verb[] {
  if (!b.key.startsWith('task:') || b.kind !== 'running') return []
  return [
    b.paused ? { key: 'resume', label: 'Resume', method: 'task.resume', params: { task: b.id }, tone: 'ok' } : { key: 'pause', label: 'Pause', method: 'task.pause', params: { task: b.id }, tone: 'wait' },
    { key: 'stop', label: 'Stop', method: 'task.cancel', params: { task: b.id }, tone: 'fault', ask: `Stop the task “${b.title}”?` },
  ]
}

// ---------------------------------------------------------------- layout

/** A block's piece on a day: from `top` to `bottom` (0 to 1 of the day), its lane among the blocks it overlaps. */
export interface TaskPlaced { key: string; block: TaskBlock; day: number; top: number; bottom: number; lane: number; lanes: number; continues: boolean }

/** Places the blocks on `w` as the events are (`eventLayout`): each on every day it crosses, by wall-clock hour, the
 *  overlapping ones in side-by-side lanes that each cluster shares. */
export function taskLayout(blocks: readonly TaskBlock[], w: Window): TaskPlaced[] {
  const perDay: TaskPlaced[][] = w.days.map(() => [])
  for (const block of blocks) {
    for (const d of w.days) {
      if (block.from_ms >= d.end_ms || block.to_ms < d.start_ms || (block.to_ms === d.start_ms && block.from_ms < d.start_ms)) continue
      const top = block.from_ms <= d.start_ms ? 0 : wallHour(block.from_ms) / 24
      const bottom = block.to_ms >= d.end_ms ? 1 : Math.max(wallHour(block.to_ms) / 24, top)
      perDay[d.index].push({ key: block.key, block, day: d.index, top, bottom: Math.min(1, Math.max(bottom, top + MIN_SHARE)), lane: 0, lanes: 1, continues: block.from_ms < d.start_ms })
    }
  }
  const out: TaskPlaced[] = []
  for (const day of perDay) {
    day.sort((a, b) => a.top - b.top || b.bottom - a.bottom || (a.key < b.key ? -1 : 1))
    let cluster: TaskPlaced[] = []
    let ends: number[] = []
    let reach = -1
    const close = () => { for (const p of cluster) p.lanes = ends.length; cluster = []; ends = [] }
    for (const p of day) {
      if (p.top >= reach) close()
      let lane = ends.findIndex((e) => e <= p.top)
      if (lane < 0) { lane = ends.length; ends.push(p.bottom) } else ends[lane] = p.bottom
      p.lane = lane
      cluster.push(p)
      reach = Math.max(reach, p.bottom)
      out.push(p)
    }
    close()
  }
  return out
}
