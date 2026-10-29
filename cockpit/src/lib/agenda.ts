// The Agenda page's pure half (theseus-p17fd; horizon HZ9): the week's window, every kind of mark placed on it, the verbs
// each mark carries, the Routines table's rows, and the moved Wakes panel's fields. Pure: `npm test` runs it
// (test/agenda.test.ts), and it imports only the protocol's types. Nothing here reads the daemon: the page passes in
// `work.agenda`'s lines, the work board's views, `routine.runs` and `wake.list`, and draws what comes back.
//
// - The strip is seven local days from a day's midnight (`weekWindow`): a column per day, 24 wall-clock hours down it.
//   A day is cut by `new Date(y, m, d + i)`, never by adding 86,400,000 ms, so a day with a DST change is 23 or 25
//   hours long and every mark still lands at the wall-clock hour a person reads on the clock.
// - A mark is one thing at one time: a reminder or check-in (dot), a routine run (tick: coming, or past and coloured
//   by how it ended), a deferred task's start (▷), a project's milestone (flag), or a watch's check (a bar across its
//   window, from the time it was set to the time it gives up).
// - Past runs are `routine.runs`' rows; the future is `work.agenda`'s lines. Neither is copied into a ledger of its own.
import type { AgendaLine, AgendaResult, RoutineInfo, RunRow, WakeInfo, WorkView } from '@protocol'

export const DAY_MS = 86_400_000
export const HOUR_MS = 3_600_000
/** The cap a run list is read at: the last 30 runs (test H5). */
export const RUNS_SHOWN = 30

export type Tone = 'live' | 'ok' | 'wait' | 'fault' | 'idle' | 'money' | 'think'

/** A day of the strip. */
export interface Day { index: number; start_ms: number; end_ms: number; label: string; today: boolean; hours: number }

/** The strip's window: seven local days, from `from_ms` to `to_ms` (what `work.agenda` is asked for). */
export interface Window { from_ms: number; to_ms: number; days: Day[] }

const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'] as const

/** The local midnight that starts the day holding `at`. */
export function midnight(at: number): number {
  const d = new Date(at)
  return new Date(d.getFullYear(), d.getMonth(), d.getDate()).getTime()
}

/** Seven local days starting `weeks` weeks from today's midnight (negative: earlier ones). */
export function weekWindow(now: number, weeks = 0): Window {
  const t = new Date(now)
  const bounds: number[] = []
  for (let i = 0; i <= 7; i++) bounds.push(new Date(t.getFullYear(), t.getMonth(), t.getDate() + weeks * 7 + i).getTime())
  const today = midnight(now)
  const days: Day[] = []
  for (let i = 0; i < 7; i++) {
    const start = bounds[i]
    const d = new Date(start)
    days.push({
      index: i, start_ms: start, end_ms: bounds[i + 1],
      label: `${WEEKDAYS[d.getDay()]} ${d.getMonth() + 1}/${d.getDate()}`,
      today: start === today,
      hours: Math.round((bounds[i + 1] - start) / HOUR_MS),
    })
  }
  return { from_ms: bounds[0], to_ms: bounds[7], days }
}

/** The wall-clock hour of `at`, fractional (09:30 is 9.5): where a mark sits down its day's column. */
export function wallHour(at: number): number {
  const d = new Date(at)
  return d.getHours() + d.getMinutes() / 60 + d.getSeconds() / 3600
}

// ---------------------------------------------------------------- the marks

export type MarkKind = 'reminder' | 'check' | 'run' | 'past' | 'start' | 'milestone' | 'watch'

/** How a past run ended, in the strip's three marks (and one for a run going now). */
export type Outcome = 'ok' | 'failed' | 'skipped' | 'running'

export interface Mark {
  /** Unique on the strip: the kind, the thing's id and the time. */
  key: string
  kind: MarkKind
  at_ms: number
  /** A watch's bar: when it gives up. */
  end_ms?: number
  title: string
  /** The wake's, task's or watch's id the verbs act on. */
  id: string
  session_id?: string
  purpose?: string
  paused?: boolean
  outcome?: Outcome
  occurrence?: number
  cost_usd?: number
  duration_ms?: number
  /** A past run's session, which names the run (`RunRow.task`). */
  run_session?: string
  line?: string
}

/** A mark's glyph, as the strip draws it. */
export function glyph(m: Pick<Mark, 'kind' | 'outcome'>): string {
  switch (m.kind) {
    case 'reminder': return '●'
    case 'check': return '○'
    case 'run': return '│'
    case 'past': return m.outcome === 'ok' ? '✓' : m.outcome === 'failed' ? '✗' : m.outcome === 'running' ? '◐' : '–'
    case 'start': return '▷'
    case 'milestone': return '⚑'
    case 'watch': return '▬'
  }
}

export function toneOf(m: Pick<Mark, 'kind' | 'outcome'>): Tone {
  switch (m.kind) {
    case 'reminder': return 'live'
    case 'check': return 'think'
    case 'run': return 'money'
    case 'past': return m.outcome === 'ok' ? 'ok' : m.outcome === 'failed' ? 'fault' : m.outcome === 'running' ? 'live' : 'idle'
    case 'start': return 'wait'
    case 'milestone': return 'wait'
    case 'watch': return 'think'
  }
}

/** A run row's state as the strip's outcome: a cancelled run was skipped (it made no report); blocked ended unhappily. */
export function outcomeOf(r: Pick<RunRow, 'state' | 'blocked'>): Outcome {
  if (r.blocked || r.state === 'failed') return 'failed'
  if (r.state === 'complete') return 'ok'
  if (r.state === 'cancelled') return 'skipped'
  return 'running'
}

/** `work.agenda`'s lines as marks. A line `expires` is a watch's last day, kept for the bar's end; `run` is a routine's. */
export function futureMarks(lines: readonly AgendaLine[]): Mark[] {
  const out: Mark[] = []
  for (const l of lines) {
    const base = { key: `${l.what}:${l.id}:${l.at_ms}`, at_ms: l.at_ms, title: l.title, id: l.id, session_id: l.session_id, purpose: l.purpose, occurrence: l.occurrence }
    if (l.what === 'run') out.push({ ...base, kind: 'run' })
    else if (l.what === 'starts') out.push({ ...base, kind: 'start' })
    else if (l.what === 'milestone') out.push({ ...base, kind: 'milestone' })
    else if (l.what === 'expires') out.push({ ...base, kind: 'watch', end_ms: l.at_ms })
    else out.push({ ...base, kind: l.purpose === 'check' ? 'check' : 'reminder' })
  }
  return out
}

/** A routine's past runs as marks: one tick each, coloured by how it ended. */
export function pastMarks(routineTitle: string, routineId: string, runs: readonly RunRow[]): Mark[] {
  return runs.map((r) => ({
    key: `past:${routineId}:${r.task}`, kind: 'past' as const, at_ms: r.started_ms, title: `${routineTitle} · run ${r.run}`,
    id: routineId, session_id: routineId, outcome: outcomeOf(r), cost_usd: r.cost_usd, duration_ms: r.duration_ms,
    run_session: r.task, line: r.report ?? r.blocked?.reason, occurrence: r.run,
  }))
}

/** The open watches on the board as bars: from when each was set to when it gives up (open ended: the window's end). */
export function watchBars(views: readonly WorkView[]): Mark[] {
  return views.filter((v) => v.kind === 'watch' && !['done', 'failed', 'cancelled', 'expired'].includes(v.state)).map((v) => ({
    key: `watch:${v.id}`, kind: 'watch' as const, at_ms: v.started_at_ms, end_ms: v.watch?.expires_at_ms ?? v.next_at_ms,
    title: v.title || v.watch?.check || v.id, id: v.id, session_id: v.session_id, purpose: 'watch',
  }))
}

// ---------------------------------------------------------------- the strip's layout

/** A tick or a dot on a day: its column, how far down (0 to 1 of the day), and its lane among those in its hour. */
export interface Placed { mark: Mark; day: number; y: number; lane: number; lanes: number }

/** A bar clipped to a day. */
export interface Bar { mark: Mark; day: number; from: number; to: number }

export interface Layout { placed: Placed[]; bars: Bar[]; hidden: number }

/** Places `marks` on `w`. Points go to their day by time and down it by wall-clock hour; the ones that share an hour sit
 *  in lanes side by side, so a busy hour stays readable. A mark outside the window is counted in `hidden`, never drawn.
 *  Linear in the marks: each is binned once, by day and hour. */
export function layout(marks: readonly Mark[], w: Window): Layout {
  const placed: Placed[] = []
  const bars: Bar[] = []
  let hidden = 0
  const buckets = new Map<number, Placed[]>()
  for (const m of marks) {
    if (m.kind === 'watch') {
      const end = m.end_ms ?? w.to_ms
      let any = false
      for (const d of w.days) {
        const from = Math.max(m.at_ms, d.start_ms)
        const to = Math.min(end, d.end_ms)
        if (to <= from && !(from === to && m.at_ms >= d.start_ms && m.at_ms < d.end_ms)) continue
        any = true
        bars.push({ mark: m, day: d.index, from: (from - d.start_ms) / (d.end_ms - d.start_ms), to: Math.max(to - d.start_ms, 0) / (d.end_ms - d.start_ms) })
      }
      if (!any) hidden++
      continue
    }
    const day = dayOf(m.at_ms, w)
    if (day < 0) { hidden++; continue }
    const d = w.days[day]
    const p: Placed = { mark: m, day, y: Math.min(wallHour(m.at_ms) / 24, 0.9999), lane: 0, lanes: 1 }
    placed.push(p)
    const slot = day * 24 + Math.min(23, Math.floor(wallHour(m.at_ms)))
    const b = buckets.get(slot)
    if (b) b.push(p); else buckets.set(slot, [p])
    void d
  }
  for (const b of buckets.values()) {
    b.sort((x, y) => x.mark.at_ms - y.mark.at_ms || (x.mark.key < y.mark.key ? -1 : 1))
    b.forEach((p, i) => { p.lane = i; p.lanes = b.length })
  }
  return { placed, bars, hidden }
}

/** The day of `w` holding `at`, or -1. */
export function dayOf(at: number, w: Window): number {
  if (at < w.from_ms || at >= w.to_ms) return -1
  for (const d of w.days) if (at < d.end_ms) return d.index
  return -1
}

/** Every mark the strip shows: the future's lines, the routines' past runs and the open watches, soonest first. */
export function allMarks(lines: readonly AgendaLine[], past: readonly Mark[], views: readonly WorkView[]): Mark[] {
  const bars = watchBars(views)
  // A watch's own `expires` line is the same watch as its bar: the bar wins.
  const barred = new Set(bars.map((b) => b.id))
  const marks = [...futureMarks(lines).filter((m) => !(m.kind === 'watch' && barred.has(m.id))), ...past, ...bars]
  return marks.sort((a, b) => a.at_ms - b.at_ms || (a.key < b.key ? -1 : 1))
}

// ---------------------------------------------------------------- the verbs

/** One verb on a mark: a label, one protocol method with its params, and the question asked first (none: no question). */
export interface Verb { key: string; label: string; method: string; params: Record<string, unknown>; ask?: string; tone: 'ok' | 'wait' | 'fault' | 'live'; /** asks the person for a time first (Move) */ prompts?: string }

/** The presets Snooze offers: each is one `wake.snooze`. */
export const SNOOZES: readonly string[] = ['10m', '1h', 'tomorrow 9:00']

/** The verbs a mark carries, each one call. Reminders and check-ins snooze, move, pause and cancel; a routine runs now,
 *  pauses or resumes, skips its next run, and is cancelled; a watch is only cancelled (nothing edits one); a deferred task is cancelled;
 *  a milestone and a past run carry none (a past run opens its session instead). */
export function verbsFor(m: Mark): Verb[] {
  const wake = (label: string, key: string, method: string, extra: Record<string, unknown> = {}, tone: Verb['tone'] = 'live', ask?: string): Verb =>
    ({ key, label, method, params: { wake: m.id, ...extra }, ask, tone })
  switch (m.kind) {
    case 'reminder':
    case 'check': {
      const verbs: Verb[] = SNOOZES.map((when) => wake(`Snooze ${when}`, `snooze:${when}`, 'wake.snooze', { when }, 'wait'))
      verbs.push({ ...wake('Move…', 'move', 'wake.snooze', {}, 'wait'), prompts: 'When? (10m, tomorrow 9:00, fri 14:00, a date)' })
      verbs.push(m.paused ? wake('Resume', 'resume', 'wake.resume', {}, 'ok') : wake('Pause', 'pause', 'wake.pause', {}, 'wait'))
      verbs.push(wake('Cancel', 'cancel', 'wake.cancel', {}, 'fault', `Cancel “${m.title}”?`))
      return verbs
    }
    case 'run': {
      const id = m.session_id ?? m.id
      return [
        { key: 'run_now', label: 'Run now', method: 'routine.run_now', params: { id }, tone: 'ok', ask: `Run “${m.title}” now?` },
        { key: 'pause', label: 'Pause', method: 'routine.pause', params: { id }, tone: 'wait' },
        { key: 'resume', label: 'Resume', method: 'routine.resume', params: { id }, tone: 'ok' },
        { key: 'skip', label: 'Skip next', method: 'routine.skip', params: { id }, tone: 'wait', ask: `Skip the next run of “${m.title}”? The series goes on after it.` },
        { key: 'cancel', label: 'Cancel', method: 'routine.delete', params: { id }, tone: 'fault', ask: `Cancel the routine “${m.title}” for good? Its runs stay as a record.` },
      ]
    }
    case 'watch': return [wake('Cancel', 'cancel', 'wake.cancel', {}, 'fault', `Cancel the watch “${m.title}”?`)]
    case 'start': return [{ key: 'cancel', label: 'Cancel', method: 'task.cancel', params: { task: m.id }, tone: 'fault', ask: `Cancel the task “${m.title}” before it starts?` }]
    case 'milestone':
    case 'past': return []
  }
}

/** The routine's own verbs by state (the table's row), where a mark would carry them all: Pause when it runs, Resume when
 *  held; Skip only while it runs, since a held series has no next run to drop. */
export function routineVerbs(v: Pick<WorkView, 'title' | 'session_id' | 'paused' | 'id'>): Verb[] {
  const mark: Mark = { key: v.id, kind: 'run', at_ms: 0, title: v.title, id: v.id, session_id: v.session_id }
  return verbsFor(mark).filter((x) => (x.key === 'pause' || x.key === 'skip' ? !v.paused : x.key === 'resume' ? !!v.paused : true))
}

// ---------------------------------------------------------------- the Routines table

/** A routine's table row: everything the cells show, worked out once. */
export interface RoutineRow {
  id: string
  session_id: string
  title: string
  schedule: string
  paused: boolean
  state: string
  next_ms?: number
  last?: { outcome: string; at_ms: number; duration_ms: number; cost_usd: number; line?: string }
  record: { ok: number; failed: number; skipped: number }
  spent_usd: number
  limit_usd: number
  /** Month spend as a share of its limit, 0 to 1 (past 1: over). */
  share: number
  deliver_to: string
}

export function routineRows(views: readonly WorkView[]): RoutineRow[] {
  return views.filter((v) => v.kind === 'routine' && v.routine).map((v) => {
    const r = v.routine!
    return {
      id: v.id, session_id: v.session_id ?? v.id, title: v.title, schedule: r.schedule, paused: !!v.paused, state: v.state,
      next_ms: v.next_at_ms, last: r.last, record: r.runs_30d, spent_usd: r.month.spent_usd, limit_usd: r.month.limit_usd,
      share: r.month.limit_usd > 0 ? r.month.spent_usd / r.month.limit_usd : 0, deliver_to: r.deliver_to,
    }
  }).sort((a, b) => (a.next_ms ?? Infinity) - (b.next_ms ?? Infinity) || (a.title < b.title ? -1 : 1))
}

/** A routine's runs, oldest first, as the cost sparkline's values. */
export function costSeries(runs: readonly RunRow[]): number[] {
  return [...runs].sort((a, b) => a.started_ms - b.started_ms).map((r) => r.cost_usd)
}

/** The sparkline's polyline points for `values` in a `w` by `h` box (a flat line sits mid-height; one value is a dot). */
export function sparkPoints(values: readonly number[], w: number, h: number, pad = 2): string {
  if (!values.length) return ''
  const max = Math.max(...values)
  const min = Math.min(...values)
  const span = max - min
  const step = values.length > 1 ? (w - 2 * pad) / (values.length - 1) : 0
  return values.map((v, i) => {
    const x = values.length > 1 ? pad + i * step : w / 2
    const y = span > 0 ? h - pad - ((v - min) / span) * (h - 2 * pad) : h / 2
    return `${x.toFixed(1)},${y.toFixed(1)}`
  }).join(' ')
}

/** The last runs shown, newest first, never more than `RUNS_SHOWN`. */
export function lastRuns(runs: readonly RunRow[]): RunRow[] {
  return [...runs].sort((a, b) => b.started_ms - a.started_ms).slice(0, RUNS_SHOWN)
}

/** A run list's summary: what the 30-day record cannot say, the shown runs' own count by outcome and their cost. */
export function runTally(runs: readonly RunRow[]): { ok: number; failed: number; skipped: number; running: number; cost_usd: number } {
  const t = { ok: 0, failed: 0, skipped: 0, running: 0, cost_usd: 0 }
  for (const r of runs) { t[outcomeOf(r)]++; t.cost_usd += r.cost_usd }
  return t
}

/** The runs a routine's view changed under: re-read `routine.runs` for a routine only when this changes. */
export function runsSignature(v: Pick<WorkView, 'state' | 'routine'>): string {
  const r = v.routine
  return r ? `${v.state}|${r.last?.at_ms ?? 0}|${r.last?.outcome ?? ''}|${r.runs_30d.ok}.${r.runs_30d.failed}.${r.runs_30d.skipped}|${r.month.spent_usd}` : v.state
}

/** `routine.runs`' routine, for the row it came with (the table reads `RoutineInfo` only for what the view lacks: its grant). */
export function grantOf(info?: Pick<RoutineInfo, 'grant'>): string {
  return info?.grant?.length ? info.grant.join(', ') : 'none'
}

// ---------------------------------------------------------------- watches and projects

export interface WatchRow { id: string; title: string; check: string; until: string; last?: { value: string; at_ms: number }; checks: number; expires_at_ms?: number; failing?: string; state: string }

export function watchRows(views: readonly WorkView[]): WatchRow[] {
  return views.filter((v) => v.kind === 'watch' && v.watch).map((v) => ({
    id: v.id, title: v.title, check: v.watch!.check, until: v.watch!.until, last: v.watch!.last, checks: v.watch!.checks,
    expires_at_ms: v.watch!.expires_at_ms, failing: v.watch!.failing, state: v.state,
  })).sort((a, b) => (a.expires_at_ms ?? Infinity) - (b.expires_at_ms ?? Infinity) || (a.title < b.title ? -1 : 1))
}

export interface ProjectRow { id: string; title: string; goal: string; steps: { done: number; total: number }; milestones: { done: number; total: number }; share: number; next?: { title: string; due_at_ms?: number }; overdue: number; review?: string }

export function projectRows(views: readonly WorkView[]): ProjectRow[] {
  return views.filter((v) => v.kind === 'project' && v.project).map((v) => {
    const p = v.project!
    return {
      id: v.id, title: v.title, goal: p.goal, steps: p.steps, milestones: p.milestones, next: p.next, overdue: p.overdue, review: p.review,
      share: p.steps.total > 0 ? Math.min(1, p.steps.done / p.steps.total) : 0,
    }
  }).sort((a, b) => (a.next?.due_at_ms ?? Infinity) - (b.next?.due_at_ms ?? Infinity) || (a.title < b.title ? -1 : 1))
}

// ---------------------------------------------------------------- the push

/** Follows a capped answer to the end of its window: while it says `more`, the next read starts at its last line (a
 *  line at that very time comes again, and is dropped). At most `MAX_PAGES` reads, then the answer says `more` still. */
export async function page(first: AgendaResult, next: (from_ms: number) => Promise<AgendaResult>): Promise<AgendaResult> {
  let out = first
  for (let n = 1; out.more && n < MAX_PAGES && out.lines.length; n++) {
    const from = out.lines[out.lines.length - 1].at_ms
    const r = await next(from)
    const seen = new Set(out.lines.map(lineKey))
    const lines = [...out.lines, ...r.lines.filter((l) => !seen.has(lineKey(l)))]
    if (lines.length === out.lines.length) break
    out = { from_ms: first.from_ms, to_ms: first.to_ms, lines, more: r.more }
  }
  return out
}
const MAX_PAGES = 12
const lineKey = (l: AgendaResult['lines'][number]) => `${l.what}:${l.id}:${l.at_ms}`


/** Does this `work.changed` (its views and gone ids) touch the future: a wake, routine, watch or project, or a task
 *  with a start time? Only then does the page read `work.agenda` and `wake.list` again; nothing else moves them. */
export function touchesFuture(c: { views: readonly Pick<WorkView, 'kind' | 'next_at_ms'>[]; gone?: readonly string[] }, heldFuture: ReadonlySet<string>): boolean {
  if (c.views.some((v) => v.kind === 'wake' || v.kind === 'routine' || v.kind === 'watch' || v.kind === 'project' || v.next_at_ms != null)) return true
  return !!c.gone?.some((id) => heldFuture.has(id))
}

// ---------------------------------------------------------------- the Wakes panel, moved here

/** Every field the Wakes panel on Actions showed, as plain values: nothing it showed is lost by the move (a test holds
 *  this list against the fields of `WakeInfo` the old panel read). */
export interface WakeFacts {
  wake_id: string
  note: string
  short: string
  session_id: string
  /** The task's id when it belongs to one, else the session's title, else its short id. */
  where: string
  task?: string
  state: string
  due_local: string
  due_at_ms: number
  /** `in 4m` or `due 2m ago`. */
  relative: string
  overdue: boolean
  set_at_ms: number
  target?: string
  /** What the old tooltip said. */
  tip: string
  /** Series words, new beside the old fields. */
  repeat?: string
  paused: boolean
  purpose?: string
}

/** The wake panel's fields; `fmtMs` words a span and `fmtClock` a time (the page passes `format.ts`'s). */
export function wakeFacts(w: WakeInfo, now: number, fmtMs: (n: number) => string, fmtClock: (n: number) => string, shortId: (id: string) => string): WakeFacts {
  const over = w.due_at_ms <= now
  return {
    wake_id: w.wake_id, note: w.note, short: w.short, session_id: w.session_id,
    where: w.task ? `task ${w.task}` : (w.session_title ?? shortId(w.session_id)), task: w.task, state: w.state,
    due_local: w.due_local, due_at_ms: w.due_at_ms,
    relative: over ? `due ${fmtMs(now - w.due_at_ms)} ago` : `in ${fmtMs(w.due_at_ms - now)}`, overdue: over,
    set_at_ms: w.set_at_ms, target: w.target,
    tip: `${w.wake_id}\nset ${fmtClock(w.set_at_ms)}${w.target ? `\nits reply goes to ${w.target}` : ''}`,
    repeat: w.every ? `every ${w.every}${w.days?.length ? ` (${w.days.join(', ')})` : ''}` : undefined,
    paused: !!w.paused, purpose: w.purpose,
  }
}
