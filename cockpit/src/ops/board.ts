// The Operations page's Board (theseus-jid7t.3): every live piece of work in five columns, Needs you, Working,
// Paused·queued, Scheduled and Done (today first, then older, paged). Tasks are the Tasks page's rows
// (`lib/taskboard.ts`'s `rowsOf`, unchanged); conversations, jobs, and the wakes and routine runs due today join them,
// as the TUI's board shows every session. Pure: `npm test` runs it (test/ops-board.test.ts), and it imports only the
// protocol's types and other pure modules.
import type { SessionInfo, TaskInfo, TaskRecord, WorkView } from '@protocol'
import { rowsOf, type TaskRow } from '../lib/taskboard.ts'
import { progressLine } from '../lib/progressLine.ts'
import { midnight } from '../lib/agenda.ts'
import type { Bucket, Filters } from './address.ts'

export type Column = 'needs' | 'working' | 'paused' | 'scheduled' | 'done'
export const COLUMNS: readonly Column[] = ['needs', 'working', 'paused', 'scheduled', 'done']
export const COLUMN_TITLES: Readonly<Record<Column, string>> = {
  needs: 'Needs you', working: 'Working', paused: 'Paused·queued', scheduled: 'Scheduled', done: 'Done',
}

export type RowKind = 'task' | 'conversation' | 'job' | 'wake' | 'routine'

export interface OpsRow {
  /** What the address selects it by: a task's or conversation's session (`ses_`), else its view's id (`act_`, `wak_`). */
  id: string
  kind: RowKind
  column: Column
  title: string
  short: string
  /** Its work state (`WorkState`), or a task record's state when the board holds no view of it. */
  state?: string
  paused: boolean
  view?: WorkView
  task?: TaskRow
  session?: SessionInfo
  /** The profile it runs on: `moved_to` when a checkpoint moved it, else its session's profile or model. */
  model?: string
  place?: string
  thread?: string
  /** The session that owns it: a job's, a wake's. */
  sessionId?: string
  /** When a scheduled row is due. */
  dueMs?: number
  /** When it ended, for a row in Done. */
  endedMs?: number
  activityMs: number
}

const ENDED = ['done', 'failed', 'cancelled', 'expired']
// The kernel's terminal states, as the Tasks page reads them.
const ENDED_INFO = ['complete', 'cancelled', 'failed', 'budget_exhausted']

/** The local midnight ending the day that holds `now`. */
const endOfDay = (now: number): number => { const d = new Date(midnight(now)); return new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1).getTime() }

/** The profile a row runs on: a checkpoint's move first (the progress line's `on sonnet`), then its session's. */
export function modelOf(view: WorkView | undefined, session: SessionInfo | undefined): string | undefined {
  return view?.moved_to ?? session?.profile ?? session?.model ?? undefined
}

/** Where a view stands on the board, by the attention every surface reads, then its state. */
function columnOfView(v: WorkView): Column {
  if (v.attention.level === 'needs_you') return 'needs'
  if (ENDED.includes(v.state)) return 'done'
  if (v.paused || v.state === 'queued') return 'paused'
  if (v.state === 'running' || v.state === 'waiting') return 'working'
  // A conversation at rest: its turn is done.
  return 'done'
}

function columnOfTask(r: TaskRow, now: number): Column {
  // A failure not yet dismissed still asks for someone: it needs you, where the Tasks page put it under Done.
  if (r.section === 'needs' || r.view?.attention.level === 'needs_you') return 'needs'
  if (r.section === 'done') return 'done'
  const starts = r.info?.starts_at_ms ?? (r.view?.state === 'queued' ? r.view.next_at_ms : undefined)
  if (starts != null && starts > now) return starts < endOfDay(now) ? 'scheduled' : 'paused'
  if (r.paused || r.view?.state === 'queued' || r.info?.state === 'queued') return 'paused'
  return 'working'
}

export interface BoardInput {
  views: readonly WorkView[]
  tasks: readonly TaskInfo[]
  sessions: readonly SessionInfo[]
  /** Report lines read from ended tasks' sessions (`reportLineOf`). */
  lines?: Readonly<Record<string, string>>
  now: number
}

/** Every row of the board, once. Under the time machine the board holds no views: the rows are the tasks and
 *  sessions as they stood. */
export function boardRows({ views, tasks, sessions, lines = {}, now }: BoardInput): OpsRow[] {
  const bySession = new Map(sessions.map((s) => [s.session_id, s]))
  const out: OpsRow[] = []
  for (const r of rowsOf([...views], [...tasks], lines)) {
    const s = bySession.get(r.id)
    const column = columnOfTask(r, now)
    out.push({
      id: r.id, kind: 'task', column, title: r.title, short: r.short, state: r.view?.state ?? r.info?.state, paused: r.paused,
      view: r.view, task: r, session: s, model: modelOf(r.view, s), place: r.place, thread: r.view?.thread, sessionId: r.id,
      dueMs: column === 'scheduled' ? r.info?.starts_at_ms ?? r.view?.next_at_ms : undefined,
      endedMs: column === 'done' ? r.view?.ended_at_ms ?? r.info?.updated_at_ms : undefined,
      activityMs: r.activityMs,
    })
  }
  const seen = new Set(out.map((r) => r.id))
  for (const v of views) {
    if (v.kind === 'conversation' || v.kind === 'job') {
      const id = v.kind === 'conversation' ? v.session_id ?? v.id : v.id
      if (seen.has(id)) continue
      seen.add(id)
      const s = v.session_id ? bySession.get(v.session_id) : undefined
      const column = columnOfView(v)
      out.push({
        id, kind: v.kind, column, title: v.title || s?.title || s?.label || v.attention.label, short: id.slice(-6), state: v.state, paused: v.paused === true,
        view: v, session: s, model: modelOf(v, s), place: v.place, thread: v.thread, sessionId: v.session_id,
        endedMs: column === 'done' ? v.ended_at_ms ?? v.state_since_ms : undefined, activityMs: v.at_ms,
      })
    } else if ((v.kind === 'wake' || v.kind === 'routine') && v.next_at_ms != null && v.next_at_ms >= now && v.next_at_ms < endOfDay(now)) {
      out.push({
        id: v.id, kind: v.kind, column: 'scheduled', title: v.title || v.attention.label, short: v.id.slice(-6), state: v.state, paused: v.paused === true,
        view: v, thread: v.thread, place: v.place, sessionId: v.session_id, dueMs: v.next_at_ms, activityMs: v.at_ms,
      })
    }
  }
  // The time machine's board: no views, so the conversations are the sessions as they stood, retired ones left out.
  if (views.length === 0) {
    for (const s of sessions) {
      if (s.kind !== 'conversation' || s.retired || seen.has(s.session_id)) continue
      const st = s.execution_state ?? 'idle'
      const column: Column = s.attention?.level === 'needs_you' ? 'needs' : st === 'running' || st === 'waiting' ? 'working' : st === 'queued' ? 'paused' : 'done'
      out.push({
        id: s.session_id, kind: 'conversation', column, title: s.title || s.label || s.session_id.slice(-6), short: s.session_id.slice(-6), state: st,
        paused: false, session: s, model: modelOf(undefined, s), sessionId: s.session_id,
        endedMs: column === 'done' ? s.last_active_ms : undefined, activityMs: s.last_active_ms,
      })
    }
  }
  return out
}

/** A row's time of need: within Needs you the longest waiting comes first, as in the queue. */
const needSince = (r: OpsRow): number => r.view?.attention.since_ms ?? r.task?.info?.attention?.since_ms ?? r.activityMs

export interface BoardColumns {
  needs: OpsRow[]
  working: OpsRow[]
  paused: OpsRow[]
  scheduled: OpsRow[]
  /** Ended since local midnight, newest first. */
  done: OpsRow[]
  /** Ended before it, newest first: paged. */
  older: OpsRow[]
}

/** The rows into columns, each in its order: needs longest waiting first, scheduled soonest first, the others newest
 *  first; Done splits at local midnight. */
export function columnsOf(rows: readonly OpsRow[], now: number): BoardColumns {
  const out: BoardColumns = { needs: [], working: [], paused: [], scheduled: [], done: [], older: [] }
  const today = midnight(now)
  for (const r of rows) {
    if (r.column === 'done') ((r.endedMs ?? r.activityMs) >= today ? out.done : out.older).push(r)
    else out[r.column].push(r)
  }
  const tie = (a: OpsRow, b: OpsRow) => (a.id < b.id ? -1 : a.id > b.id ? 1 : 0)
  const newest = (a: OpsRow, b: OpsRow) => b.activityMs - a.activityMs || tie(a, b)
  out.needs.sort((a, b) => needSince(a) - needSince(b) || tie(a, b))
  out.scheduled.sort((a, b) => (a.dueMs ?? 0) - (b.dueMs ?? 0) || tie(a, b))
  out.working.sort(newest)
  out.paused.sort(newest)
  const ended = (a: OpsRow, b: OpsRow) => (b.endedMs ?? b.activityMs) - (a.endedMs ?? a.activityMs) || tie(a, b)
  out.done.sort(ended)
  out.older.sort(ended)
  return out
}

/** A span the `ended` chip takes: `30m`, `1h`, `1d`, `7d`. */
export function spanMs(s: string): number | null {
  const m = /^(\d+)([mhd])$/.exec(s)
  if (!m) return null
  return Number(m[1]) * ({ m: 60_000, h: 3_600_000, d: 86_400_000 } as const)[m[2] as 'm' | 'h' | 'd']
}

/** A task's project, by its record's parents: the nearest record whose kind is `project`. */
export function projectOf(sessionId: string | undefined, records: readonly TaskRecord[]): string | undefined {
  if (!sessionId) return undefined
  const byId = new Map(records.map((t) => [t.id, t]))
  let t = records.find((x) => x.session === sessionId)
  for (let i = 0; t && i < 64; i++) {
    if (t.kind === 'project') return t.id
    t = t.parent ? byId.get(t.parent) : undefined
  }
  return undefined
}

/** Does a counter's filter hold `r`? `spentToday` is the sessions that spent since local midnight. */
export function inBucket(r: OpsRow, b: Bucket, now: number, spentToday: ReadonlySet<string>): boolean {
  switch (b) {
    case 'needs': return r.column === 'needs'
    case 'working': return r.column === 'working'
    case 'paused': return r.column === 'paused'
    case 'still': return r.column === 'scheduled'
    case 'done': return r.column === 'done' && (r.endedMs ?? r.activityMs) >= midnight(now)
    case 'spent': return r.sessionId != null && spentToday.has(r.sessionId)
  }
}

export interface FilterContext { now: number; spentToday: ReadonlySet<string>; records: readonly TaskRecord[] }

/** Does every chip of `f` hold `r`? */
export function matchesFilters(r: OpsRow, f: Filters, cx: FilterContext): boolean {
  if (f.bucket && !inBucket(r, f.bucket as Bucket, cx.now, cx.spentToday)) return false
  if (f.thread && r.thread !== f.thread) return false
  if (f.model && r.model !== f.model) return false
  if (f.place && r.place !== f.place) return false
  if (f.kind && r.kind !== f.kind) return false
  if (f.state && r.state !== f.state) return false
  if (f.project && projectOf(r.sessionId, cx.records) !== f.project) return false
  if (f.ended) {
    const span = spanMs(f.ended)
    if (span == null || r.column !== 'done' || cx.now - (r.endedMs ?? r.activityMs) > span) return false
  }
  return true
}

/** What a search reads of a row: its title, its brief, its report line and its progress line. */
export function searchText(r: OpsRow, now: number, records: readonly TaskRecord[] = []): string {
  const brief = records.find((t) => t.session === r.id)?.objective ?? r.task?.info?.title ?? ''
  const line = r.view ? progressLine(r.view, now) ?? '' : ''
  return `${r.title}\n${brief}\n${r.task?.reportLine ?? r.view?.report?.first_line ?? ''}\n${line}\n${r.short}`.toLowerCase()
}

/** Every word of the search, found in any case. */
export function matchesQuery(r: OpsRow, q: string, now: number, records: readonly TaskRecord[] = []): boolean {
  const words = q.toLowerCase().split(/\s+/).filter(Boolean)
  if (!words.length) return true
  const hay = searchText(r, now, records)
  return words.every((w) => hay.includes(w))
}

/** The rows a filter and a search keep, and how many there were: the bar's "n of m". */
export function shown(rows: readonly OpsRow[], f: Filters, q: string, cx: FilterContext): { rows: OpsRow[]; of: number } {
  return { rows: rows.filter((r) => matchesFilters(r, f, cx) && matchesQuery(r, q, cx.now, cx.records)), of: rows.length }
}

/** Does `sel` name this row? A task by its session, view, execution or short id; anything else by its id or view. */
export function selects(r: OpsRow, sel: string | null): boolean {
  if (!sel) return false
  return sel === r.id || sel === r.view?.id || sel === r.short || sel === r.task?.info?.execution_id || (r.kind === 'task' && sel === r.task?.short)
}

/** The counters the bar shows, but needs you (the queue's length). `spent` is today's dollars. */
export interface Counters { working: number; paused: number; still: number; done: number; spent: number }

export function countersOf(rows: readonly OpsRow[], now: number, spentUsd: number): Counters {
  const today = midnight(now)
  const n = (c: Column) => rows.filter((r) => r.column === c).length
  return { working: n('working'), paused: n('paused'), still: n('scheduled'), done: rows.filter((r) => r.column === 'done' && (r.endedMs ?? r.activityMs) >= today).length, spent: spentUsd }
}

/** Today's spend from the ledger's model calls since local midnight, in all and by session: Money's meaning. */
export function spendToday(rows: ReadonlyArray<{ kind: string; at_unix_ms: number; session_id?: string | null; data?: unknown }>, now: number): { usd: number; sessions: Set<string> } {
  const from = midnight(now)
  let usd = 0
  const sessions = new Set<string>()
  for (let i = rows.length - 1; i >= 0; i--) {
    const r = rows[i]
    if (r.at_unix_ms < from) break
    if (r.kind !== 'provider.call' || r.at_unix_ms > now) continue
    const cost = Number((r.data as { cost_usd?: unknown } | null)?.cost_usd ?? 0) || 0
    usd += cost
    if (r.session_id && cost > 0) sessions.add(r.session_id)
  }
  return { usd, sessions }
}

/** The chips' choices, from the rows on the board: each distinct value, in order. */
export function chipChoices(rows: readonly OpsRow[]): { thread: string[]; model: string[]; place: string[]; kind: string[]; state: string[] } {
  const of = (f: (r: OpsRow) => string | undefined) => [...new Set(rows.map(f).filter((x): x is string => !!x))].sort()
  return { thread: of((r) => r.thread), model: of((r) => r.model), place: of((r) => r.place), kind: of((r) => r.kind), state: of((r) => r.state) }
}

/** Is a row's task alive (not ended)? Its acts (pause, stop) are only for the living. */
export const alive = (r: OpsRow): boolean => r.column !== 'done' && !(r.task?.info && ENDED_INFO.includes(r.task.info.state) && !r.view)
