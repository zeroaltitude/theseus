// The Timeline lens's layout (theseus-jid7t.5): the week strip v2's three layers per day, beside the Agenda's own
// (`lib/agenda.ts`'s window and marks, which it reuses): the calendar's events as background blocks in side-by-side
// lanes with an all-day band above the hours, the "ran" bars of each execution in a left gutter, and Theseus's marks
// in the right gutter. Then the night fold, the clashes, the Day zoom, the keys' order, and the table's rows. Pure:
// `npm test` runs it (test/ops-timeline.test.ts).
//
// Every position is a wall-clock hour down a 24-row day, as the Agenda's marks are (`wallHour`), so a 23- or 25-hour
// day keeps its rows: an event at 09:00 sits at 09:00 whatever the clock did that night.
import type { CalendarEvent, CalendarWindow, RunRow, WorkView } from '@protocol'
import { DAY_MS, midnight, outcomeOf, verbsFor, wallHour, type Mark, type Tone, type Verb, type Window } from '../lib/agenda.ts'
import { drawnEvents, eventFacts } from './calendar.ts'
import { taskLine, type TaskBlock } from './taskblocks.ts'

/** One calendar's event on the strip, with its calendar. */
export interface CalEvent { ev: CalendarEvent; cal: CalendarWindow }

/** Every event the strip may draw, from every calendar whose state lets it. */
export function calEvents(cals: readonly CalendarWindow[]): CalEvent[] {
  return cals.flatMap((cal) => drawnEvents(cal).map((ev) => ({ ev, cal })))
}

// ---------------------------------------------------------------- the window: week, Day zoom, and what is read

/** The Day zoom's window: the one day `day` of `w`, as a window of its own (its index 0). */
export function zoomed(w: Window, day: number | null): Window {
  if (day == null || day < 0 || day >= w.days.length) return w
  const d = w.days[day]
  return { from_ms: d.start_ms, to_ms: d.end_ms, days: [{ ...d, index: 0 }] }
}

/** The spans `calendar.window` is asked for: the shown window (a Day zoom read a day either side), and today and
 *  tomorrow for Coming up, one span when they meet. Local midnights, so a span moves only when the day does; each is at
 *  most the daemon's 62 days. Outside the Timeline (`shown` null) with Coming up not on screen (`coming` false) there is
 *  nothing to read. */
export function readSpans(shown: { from_ms: number; to_ms: number } | null, now: number, zoom = false, coming = true): Array<{ from_ms: number; to_ms: number }> {
  if (!shown && !coming) return [] // no lens shows the calendar and Coming up is folded: ask for nothing
  const t = new Date(now)
  const today = { from_ms: midnight(now), to_ms: new Date(t.getFullYear(), t.getMonth(), t.getDate() + 2).getTime() }
  if (!shown) return [today]
  // Only the two bounds: these are `calendar.window`'s params as sent.
  const s = zoom ? { from_ms: shift(shown.from_ms, -1), to_ms: shift(shown.to_ms, 1) } : { from_ms: shown.from_ms, to_ms: shown.to_ms }
  if (s.from_ms <= today.to_ms && today.from_ms <= s.to_ms && Math.max(s.to_ms, today.to_ms) - Math.min(s.from_ms, today.from_ms) <= 62 * DAY_MS) {
    return [{ from_ms: Math.min(s.from_ms, today.from_ms), to_ms: Math.max(s.to_ms, today.to_ms) }]
  }
  return [s, today]
}
const shift = (at: number, days: number) => { const d = new Date(at); return new Date(d.getFullYear(), d.getMonth(), d.getDate() + days).getTime() }

// ---------------------------------------------------------------- layer 1: the events

/** A timed event's block on a day: from `top` to `bottom` (0 to 1 of the day's 24 rows), its lane among the events it
 *  overlaps. `continues`: the event began on an earlier day, so `top` 0 is the day's edge, not a start (an event that
 *  starts exactly at 00:00 does not continue). */
export interface Block { key: string; ev: CalendarEvent; cal: CalendarWindow; day: number; top: number; bottom: number; lane: number; lanes: number; continues: boolean }

/** An all-day event on a day of the band above the hours, its row among that day's all-day events. */
export interface AllDay { key: string; ev: CalendarEvent; cal: CalendarWindow; day: number; row: number }

/** The smallest block drawn, as a share of the day (15 minutes), so a five-minute meeting is still a target. */
export const MIN_BLOCK = 0.25 / 24

/** Places the events on `w`: a timed one on each day it crosses, clipped to it, by wall-clock hour; overlapping ones in
 *  side-by-side lanes (each cluster of overlaps shares one lane count); an all-day one in the band of each day it
 *  covers (its end is the exclusive midnight Google gives). */
export function eventLayout(events: readonly CalEvent[], w: Window): { blocks: Block[]; allDay: AllDay[] } {
  const blocks: Block[] = []
  const allDay: AllDay[] = []
  const perDay: Block[][] = w.days.map(() => [])
  for (const { ev, cal } of events) {
    for (const d of w.days) {
      if (ev.start_ms >= d.end_ms || ev.end_ms <= d.start_ms) continue
      if (ev.all_day) { allDay.push({ key: ev.key, ev, cal, day: d.index, row: 0 }); continue }
      const top = ev.start_ms <= d.start_ms ? 0 : wallHour(ev.start_ms) / 24
      const bottom = ev.end_ms >= d.end_ms ? 1 : Math.max(wallHour(ev.end_ms) / 24, top)
      perDay[d.index].push({ key: ev.key, ev, cal, day: d.index, top, bottom: Math.min(1, Math.max(bottom, top + MIN_BLOCK)), lane: 0, lanes: 1, continues: ev.start_ms < d.start_ms })
    }
  }
  for (const day of perDay) {
    day.sort((a, b) => a.top - b.top || b.bottom - a.bottom || (a.key < b.key ? -1 : 1))
    // Greedy lanes over each cluster of overlapping blocks: a block takes the first lane that has ended.
    let cluster: Block[] = []
    let ends: number[] = []
    let reach = -1
    const close = () => { for (const b of cluster) b.lanes = ends.length; cluster = []; ends = [] }
    for (const b of day) {
      if (b.top >= reach) close()
      let lane = ends.findIndex((e) => e <= b.top)
      if (lane < 0) { lane = ends.length; ends.push(b.bottom) } else ends[lane] = b.bottom
      b.lane = lane
      cluster.push(b)
      reach = Math.max(reach, b.bottom)
      blocks.push(b)
    }
    close()
  }
  const rows = new Map<number, number>()
  allDay.sort((a, b) => a.ev.start_ms - b.ev.start_ms || (a.key < b.key ? -1 : 1))
  for (const a of allDay) { a.row = rows.get(a.day) ?? 0; rows.set(a.day, a.row + 1) }
  return { blocks, allDay }
}

// ---------------------------------------------------------------- layer 2: the ran bars

/** One execution's run, clipped to a day: its tone by how it ended. */
export interface Ran { key: string; day: number; top: number; bottom: number; tone: Tone; line: string }

const RAN_TONE: Readonly<Record<string, Tone>> = { ok: 'ok', failed: 'fault', skipped: 'idle', running: 'live' }

/** The ran bars: every routine run (`routine.runs`) and every task or job on the board that started, from its start to
 *  its end (now while it runs), on each day it crosses. */
export function ranBars(views: readonly WorkView[], runs: Readonly<Record<string, readonly RunRow[]>>, w: Window, now: number): Ran[] {
  const spans: Array<{ key: string; from: number; to: number; tone: Tone; line: string }> = []
  for (const [id, rs] of Object.entries(runs)) {
    for (const r of rs) {
      const o = outcomeOf(r)
      spans.push({ key: `ran:${id}:${r.task}`, from: r.started_ms, to: o === 'running' ? now : r.started_ms + r.duration_ms, tone: RAN_TONE[o], line: `run ${r.run} · ${o}` })
    }
  }
  for (const v of views) {
    if ((v.kind !== 'task' && v.kind !== 'job') || v.state === 'queued' || v.next_at_ms != null) continue
    const live = v.ended_at_ms == null
    const tone: Tone = v.state === 'failed' ? 'fault' : v.state === 'cancelled' || v.state === 'expired' ? 'idle' : live ? 'live' : 'ok'
    spans.push({ key: `ran:${v.id}`, from: v.started_at_ms, to: v.ended_at_ms ?? now, tone, line: `${v.title || v.id} · ${v.state}` })
  }
  const out: Ran[] = []
  for (const s of spans) {
    for (const d of w.days) {
      if (s.from >= d.end_ms || s.to < d.start_ms) continue
      const top = s.from <= d.start_ms ? 0 : wallHour(s.from) / 24
      const bottom = s.to >= d.end_ms ? 1 : Math.max(wallHour(s.to) / 24, top)
      out.push({ key: `${s.key}:${d.index}`, day: d.index, top, bottom, tone: s.tone, line: s.line })
    }
  }
  return out
}

// ---------------------------------------------------------------- the night fold

/** The fold's hours by default: 22:00 to 07:00. */
export const NIGHT = { from: 22, to: 7 } as const

/** A series' tick (a routine run due or past) is counted inside the fold; it does not hold the night open. */
const opens = (m: Mark) => m.kind !== 'run' && m.kind !== 'past' && m.kind !== 'watch'

/** The night fold over `w`: the early hours `[0, early)` and the late ones `[late, 24)` fold, each shrinking to leave
 *  open any hour that holds an event or a one-off mark (a reminder, a check-in, a deferred start, a milestone) on any
 *  day shown. A mark exactly on the fold's edge (07:00, 22:00) sits on the edge line and opens nothing. The series'
 *  ticks inside it are counted, never hidden silently. */
export function nightFold(w: Window, marks: readonly Mark[], blocks: readonly Pick<Block, 'top' | 'bottom' | 'day'>[]): { early: number; late: number; count: number } {
  let early: number = NIGHT.to
  let late: number = NIGHT.from
  const keep = (from: number, to: number) => {
    if (from < early) early = Math.max(0, Math.floor(from))
    if (to > late) late = Math.min(24, Math.ceil(to))
  }
  for (const b of blocks) keep(b.top * 24, b.bottom * 24)
  const inWin = marks.filter((m) => m.at_ms >= w.from_ms && m.at_ms < w.to_ms && m.kind !== 'watch')
  for (const m of inWin) if (opens(m)) { const h = wallHour(m.at_ms); keep(h, h) }
  if (late < early) late = early
  let count = 0
  for (const m of inWin) { const h = wallHour(m.at_ms); if (h < early || h > late) count++ }
  return { early, late, count }
}

/** The pixel a wall-clock hour sits at, with the folded hours drawn as a band of `foldPx` each side. */
export function yOf(hour: number, fold: { early: number; late: number } | null, hourPx: number, foldPx: number): number {
  if (!fold) return hour * hourPx
  const { early, late } = fold
  const top = early > 0 ? foldPx : 0
  if (hour < early) return early > 0 ? (hour / early) * foldPx : 0
  if (hour <= late) return top + (hour - early) * hourPx
  return top + (late - early) * hourPx + (late < 24 ? ((hour - late) / (24 - late)) * foldPx : 0)
}

/** The strip's height in pixels under the fold. */
export const heightOf = (fold: { early: number; late: number } | null, hourPx: number, foldPx: number): number => yOf(24, fold, hourPx, foldPx)

/** A pixel back to a wall-clock hour: the inverse of `yOf`, for a drop. */
export function hourAt(y: number, fold: { early: number; late: number } | null, hourPx: number, foldPx: number): number {
  if (!fold) return Math.max(0, Math.min(24, y / hourPx))
  const { early, late } = fold
  const top = early > 0 ? foldPx : 0
  if (y < top) return (y / foldPx) * early
  const open = top + (late - early) * hourPx
  if (y <= open) return early + (y - top) / hourPx
  return late < 24 ? Math.min(24, late + ((y - open) / foldPx) * (24 - late)) : 24
}

// ---------------------------------------------------------------- the clashes

/** A meeting, for the clashes: a timed event the owner has not declined. Busy blocks count; all-day events do not. */
const meeting = (ev: CalendarEvent) => !ev.all_day && ev.self_response !== 'declined' && ev.end_ms > ev.start_ms

/** The marks that can clash: a point in time Theseus will act at (a reminder, a check-in, a routine run due, a deferred
 *  start). */
const pointed = (m: Mark) => m.kind === 'reminder' || m.kind === 'check' || m.kind === 'run' || m.kind === 'start'

export interface Clashes { keys: Set<string>; count: number; why: Map<string, string> }

/** The clashes: a mark inside a meeting, and two meetings that overlap. Each pair counts once; a key in `keys` gets a
 *  ◆, and `why` says what it clashes with. Sorted sweeps, so a busy week is quick. */
export function clashes(marks: readonly Mark[], events: readonly CalEvent[]): Clashes {
  const keys = new Set<string>()
  const why = new Map<string, string>()
  let count = 0
  const evs = events.filter((e) => meeting(e.ev)).sort((a, b) => a.ev.start_ms - b.ev.start_ms)
  const say = (k: string, w: string) => { keys.add(k); if (!why.has(k)) why.set(k, w) }
  // Meeting with meeting: each against those that start before it ends.
  for (let i = 0; i < evs.length; i++) {
    for (let j = i + 1; j < evs.length && evs[j].ev.start_ms < evs[i].ev.end_ms; j++) {
      count++
      say(evs[i].ev.key, `overlaps another ${evs[j].cal.label} meeting`)
      say(evs[j].ev.key, `overlaps another ${evs[i].cal.label} meeting`)
    }
  }
  // Mark in a meeting: each mark against the meetings that began by its time and have not ended.
  const pts = marks.filter(pointed).sort((a, b) => a.at_ms - b.at_ms)
  let next = 0
  let open: CalEvent[] = []
  for (const m of pts) {
    while (next < evs.length && evs[next].ev.start_ms <= m.at_ms) open.push(evs[next++])
    open = open.filter((e) => e.ev.end_ms > m.at_ms)
    for (const e of open) { count++; say(m.key, `falls inside a ${e.cal.label} meeting`); say(e.ev.key, 'a mark of Theseus falls inside it') }
  }
  return { keys, count, why }
}

/** The command bar's "meetings this week": the window's meetings (timed, not declined). */
export function meetingCount(events: readonly CalEvent[], w: Pick<Window, 'from_ms' | 'to_ms'>): number {
  return events.filter((e) => meeting(e.ev) && e.ev.start_ms < w.to_ms && e.ev.end_ms > w.from_ms).length
}

// ---------------------------------------------------------------- the keys

/** The order `j` and `k` step through: the marks and the timed events of the window, by time. */
export function timelineOrder(marks: readonly Mark[], events: readonly CalEvent[], w: Pick<Window, 'from_ms' | 'to_ms'>, tasks: readonly TaskBlock[] = []): string[] {
  // A task block that derives from a mark has its key already; one a mark does not stand for (a task that is going or
  // went) is stepped to as well.
  const marked = new Set(marks.map((m) => m.key))
  const at: Array<[number, string]> = [
    ...marks.filter((m) => m.at_ms >= w.from_ms && m.at_ms < w.to_ms).map((m) => [m.at_ms, m.key] as [number, string]),
    ...events.filter((e) => e.ev.start_ms < w.to_ms && e.ev.end_ms > w.from_ms).map((e) => [e.ev.start_ms, `ev:${e.ev.key}`] as [number, string]),
    ...tasks.filter((t) => !marked.has(t.key) && t.from_ms >= w.from_ms && t.from_ms < w.to_ms).map((t) => [t.from_ms, t.key] as [number, string]),
  ]
  return at.sort((a, b) => a[0] - b[0] || (a[1] < b[1] ? -1 : 1)).map((x) => x[1])
}

/** The next key along `order` from `at` (`dir` 1 or -1): the first or last when nothing is focused. */
export function stepOrder(order: readonly string[], at: string | null, dir: 1 | -1): string | null {
  if (!order.length) return null
  const i = at ? order.indexOf(at) : -1
  if (i < 0) return dir > 0 ? order[0] : order[order.length - 1]
  return order[Math.max(0, Math.min(order.length - 1, i + dir))]
}

/** The Horizon key's verb on a mark, as the TUI's: `z` snoozes ten minutes, `p` pauses or resumes, `r` runs a routine
 *  now, `x` cancels. Null when the mark carries no such verb. */
export function horizonVerb(m: Mark, key: 'z' | 'p' | 'r' | 'x'): Verb | null {
  const vs = verbsFor(m)
  const by = (k: string) => vs.find((v) => v.key === k) ?? null
  if (key === 'z') return by('snooze:10m')
  if (key === 'p') return m.paused ? by('resume') : by('pause')
  if (key === 'r') return by('run_now')
  return by('cancel')
}

/** The day `d` zooms to: the focused thing's day, else today's, else the first. */
export function zoomDay(w: Window, focusAt: number | null, now: number): number {
  const at = focusAt ?? now
  const d = w.days.find((x) => at >= x.start_ms && at < x.end_ms)
  return d ? d.index : 0
}

// ---------------------------------------------------------------- the table

/** The table toggle's rows: the window's marks and events, by time, events redacted as the page is. */
export interface TableRow { key: string; at_ms: number; what: string; calendar: string; line: string; clash: boolean }

export function tableRows(marks: readonly Mark[], events: readonly CalEvent[], w: Pick<Window, 'from_ms' | 'to_ms'>, redact: boolean, c: Clashes, markLine: (m: Mark) => string, tasks: readonly TaskBlock[] = [], now = 0): TableRow[] {
  const rows: TableRow[] = []
  // A task a mark does not stand for (one that is going or went) is a row of its own; the others are their mark's.
  const marked = new Set(marks.map((m) => m.key))
  for (const t of tasks) {
    if (marked.has(t.key) || t.to_ms < w.from_ms || t.from_ms >= w.to_ms) continue
    rows.push({ key: t.key, at_ms: t.from_ms, what: `task ${t.kind}`, calendar: 'Theseus', line: taskLine(t, now), clash: false })
  }
  for (const m of marks) {
    if (m.at_ms < w.from_ms || m.at_ms >= w.to_ms) continue
    rows.push({ key: m.key, at_ms: m.at_ms, what: m.kind, calendar: 'Theseus', line: markLine(m).split('\n')[0], clash: c.keys.has(m.key) })
  }
  for (const { ev, cal } of events) {
    if (ev.start_ms >= w.to_ms || ev.end_ms <= w.from_ms) continue
    const f = eventFacts(ev, cal, redact)
    rows.push({ key: `ev:${ev.key}`, at_ms: ev.start_ms, what: ev.all_day ? 'all-day event' : ev.busy ? 'busy' : 'event', calendar: f.row[2], line: `${f.row[1]} · ${f.row[3]}${f.row[4] ? ` · ${f.row[4]}` : ''}`, clash: c.keys.has(ev.key) })
  }
  return rows.sort((a, b) => a.at_ms - b.at_ms || (a.key < b.key ? -1 : 1))
}

// ---------------------------------------------------------------- where the hours open

/** The hour the strip opens at when nothing else decides. */
export const DEFAULT_HOUR = 8

/** The wall-clock hours (fractional) at which a meeting starts or a one-off mark sits on any day shown: what the first
 *  view of an unfolded day looks for. A series' tick or a watch's window does not count (as in `nightFold`). */
export function openHours(w: Window, marks: readonly Mark[], blocks: readonly (Pick<Block, 'top'> & { continues?: boolean })[]): number[] {
  const out = blocks.filter((b) => !b.continues).map((b) => b.top * 24)
  for (const m of marks) if (m.at_ms >= w.from_ms && m.at_ms < w.to_ms && m.kind !== 'watch' && opens(m)) out.push(wallHour(m.at_ms))
  return out
}

/** The hour the first view of a week or a day centres on: now when today is shown (`nowHour`), else the earliest of
 *  `hours`, else 08:00. */
export function centerHour(nowHour: number | null, hours: readonly number[]): number {
  if (nowHour != null) return nowHour
  return hours.length ? Math.min(...hours) : DEFAULT_HOUR
}

/** The `scrollTop` that puts `hour` in the middle of a box `viewPx` tall over `contentPx` of hours, kept inside it. */
export function scrollFor(y: number, viewPx: number, contentPx: number): number {
  return Math.max(0, Math.min(Math.round(y - viewPx / 2), Math.max(0, contentPx - viewPx)))
}
