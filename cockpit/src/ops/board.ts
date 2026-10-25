// The Operations page's Board (theseus-jid7t.3): every live piece of work in five columns, Needs you, Working,
// Paused·queued, Scheduled and Done (today first, then older, paged). Tasks are the Tasks page's rows
// (`lib/taskboard.ts`'s `rowsOf`, unchanged); conversations, jobs, and the wakes and routine runs due today join them,
// as the TUI's board shows every session. Pure: `npm test` runs it (test/ops-board.test.ts), and it imports only the
// protocol's types and other pure modules.
import type { DataMark, SessionInfo, TaskInfo, TaskRecord, WorkView } from '@protocol'
import { rowsOf, type TaskRow } from '../lib/taskboard.ts'
import { progressLine } from '../lib/progressLine.ts'
import { midnight } from '../lib/agenda.ts'
import type { Bucket, Filters } from './address.ts'
import { bucketOfCard, bucketOfSession, countedCard, endOfDay, OWNER_CALLS, type OwnerCalls, type StateBucket } from './state.ts'
import { askKeysOf, type QueueItem } from './queue.ts'

export type Column = 'needs' | 'working' | 'paused' | 'scheduled' | 'done'
export const COLUMNS: readonly Column[] = ['needs', 'working', 'paused', 'scheduled', 'done']
export const COLUMN_TITLES: Readonly<Record<Column, string>> = {
  needs: 'Needs you', working: 'Working', paused: 'Paused·queued', scheduled: 'Scheduled', done: 'Done',
}

export type RowKind = 'task' | 'conversation' | 'job' | 'wake' | 'routine' | 'watch' | 'step' | 'ask'

export interface OpsRow {
  /** What the address selects it by: a task's or conversation's session (`ses_`), else its view's id (`act_`, `wak_`),
   *  else a plan's or step's record (`tsk_`), else an ask's queue key. */
  id: string
  kind: RowKind
  /** Its one bucket (`ops/state.ts`'s `bucketOfCard`): the Board, the Graph and the counters all read this. */
  bucket: StateBucket
  /** Its Board column, from its bucket; null when the bucket has none (a to-do step, a later wake, a resting
   *  conversation of the time machine): the Board skips it and no counter has it. */
  column: Column | null
  /** Whether a counter counts it: on the Board's columns, and not a second count of work its agent already counts
   *  (`OWNER_CALLS.agentsOnly`). */
  counted: boolean
  title: string
  short: string
  /** Its work state (`WorkState`), or a task record's state when the board holds no view of it. */
  state?: string
  paused: boolean
  /** A conversation at rest, from the time machine's sessions: a Graph anchor and a Threads header, never a Board row
   *  nor a count. The builder gives it no column; nothing else tests this flag for a column or a count. */
  rest?: boolean
  view?: WorkView
  task?: TaskRow
  session?: SessionInfo
  /** The task record it is, or carries: a task's own record keeps the row's `ses_` id and adds this one. */
  recordId?: string
  record?: TaskRecord
  /** A plan: a record some record names as `parent`, or a project. Derived, never stored. */
  plan?: boolean
  /** A step with no task of its own: who works it (the row id of its claim's holder, else the nearest task above it,
   *  else the conversation it came from). */
  carrier?: string
  /** The row (by id, view id, record id or session) it hangs from in the Graph. */
  parent?: string
  /** The asks on it (`queueOf`'s items mapped to this card, `askKeysOf`): an ask row is its own one. */
  asks: number
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
  /** What the floor reads of its data (`open` when the daemon says nothing). */
  data?: DataMark
  /** The ask itself, on an ask row. */
  item?: QueueItem
  activityMs: number
}

// The kernel's terminal states, as the Tasks page reads them.
const ENDED_INFO = ['complete', 'cancelled', 'failed', 'budget_exhausted']

/** The profile a row runs on: a checkpoint's move first (the progress line's `on sonnet`), then its session's. */
export function modelOf(view: WorkView | undefined, session: SessionInfo | undefined): string | undefined {
  return view?.moved_to ?? session?.profile ?? session?.model ?? undefined
}

/** The column of a state bucket (`ops/state.ts`, the one model). A conversation at rest keeps the column it had, Done,
 *  where a view of one is held; answering zo72s is the one line `rest: null`, and no other code tests `rest` for a
 *  column or a count. A to-do step has no column. */
export const COLUMN_OF: Readonly<Record<StateBucket, Column | null>> = { needs: 'needs', working: 'working', scheduled: 'scheduled', paused: 'paused', todo: null, rest: 'done', ended: 'done' }

export interface BoardInput {
  views: readonly WorkView[]
  tasks: readonly TaskInfo[]
  sessions: readonly SessionInfo[]
  /** Report lines read from ended tasks' sessions (`reportLineOf`). */
  lines?: Readonly<Record<string, string>>
  /** The task records: each is a plan or a step (a task's own keeps its row), and a suspended one keeps its task open. */
  records?: readonly TaskRecord[]
  /** What waits on the owner (`queueOf`): each item is an ask on a card, or a row of its own. */
  queue?: readonly QueueItem[]
  now: number
  /** The time machine shows the past (`world != null`): the board holds no views then, and the sessions stand in for
   *  the conversations. Absent, it is guessed from the views, as before the page passed it. */
  past?: boolean
  /** The owner's open calls (`OWNER_CALLS`). */
  calls?: OwnerCalls
}

/** Every card of the work, once: the one work set the counters, the Board, the Graph and the selection all read.
 *  Tasks, conversations, jobs, the wakes and routine runs, the plans and steps of the records, and the asks no card
 *  owns. Under the time machine the board holds no views: the rows are the tasks and sessions as they stood. In the
 *  present, a conversation at rest is a row (Done, as the Board always had it). */
export function boardRows({ views, tasks, sessions, lines = {}, records = [], queue = [], now, past = views.length === 0, calls = OWNER_CALLS }: BoardInput): OpsRow[] {
  const bySession = new Map(sessions.map((s) => [s.session_id, s]))
  const out: OpsRow[] = []
  const draft = (r: Omit<OpsRow, 'bucket' | 'column' | 'counted' | 'asks'>): OpsRow => ({ ...r, bucket: 'rest', column: null, counted: false, asks: 0 })
  for (const r of rowsOf([...views], [...tasks], lines)) {
    const s = bySession.get(r.id)
    out.push(draft({
      id: r.id, kind: 'task', title: r.title, short: r.short, state: r.view?.state ?? r.info?.state, paused: r.paused,
      view: r.view, task: r, session: s, model: modelOf(r.view, s), place: r.place, thread: r.view?.thread, sessionId: r.id, parent: r.view?.parent,
      activityMs: r.activityMs,
    }))
  }
  const seen = new Set(out.map((r) => r.id))
  const today = endOfDay(now)
  for (const v of views) {
    if (v.kind === 'conversation' || v.kind === 'job') {
      const id = v.kind === 'conversation' ? v.session_id ?? v.id : v.id
      if (seen.has(id)) continue
      seen.add(id)
      const s = v.session_id ? bySession.get(v.session_id) : undefined
      out.push(draft({
        id, kind: v.kind, title: v.title || s?.title || s?.label || v.attention.label, short: id.slice(-6), state: v.state, paused: v.paused === true,
        view: v, session: s, model: modelOf(v, s), place: v.place, thread: v.thread, sessionId: v.session_id, parent: v.parent, activityMs: v.at_ms,
      }))
    } else if (v.kind === 'wake' || v.kind === 'routine' || v.kind === 'watch') {
      // Due today: a Board card. Later, or a watch: a row for the Graph's "+ future", with no column and no count.
      if (seen.has(v.id)) continue
      seen.add(v.id)
      out.push(draft({
        id: v.id, kind: v.kind, title: v.title || v.attention.label, short: v.id.slice(-6), state: v.state, paused: v.paused === true,
        view: v, thread: v.thread, place: v.place, sessionId: v.session_id, parent: v.parent, dueMs: v.next_at_ms, activityMs: v.at_ms,
      }))
    }
  }
  // The time machine's board: no views, so the conversations are the sessions as they stood, retired ones left out.
  // `ready` is at rest, never working: its row is flagged `rest` and given no column, so the Graph keeps its anchor and
  // no count has it.
  const settled = new Set<OpsRow>()
  if (past) {
    for (const s of sessions) {
      if (s.kind !== 'conversation' || s.retired || seen.has(s.session_id)) continue
      const bucket = bucketOfSession(s)
      const column = bucket === 'rest' ? null : COLUMN_OF[bucket]
      const row: OpsRow = {
        ...(bucket === 'rest' ? { rest: true } : {}),
        id: s.session_id, kind: 'conversation', bucket, column, counted: column != null, asks: 0, title: s.title || s.label || s.session_id.slice(-6), short: s.session_id.slice(-6), state: s.execution_state ?? 'idle',
        paused: false, session: s, model: modelOf(undefined, s), sessionId: s.session_id,
        endedMs: column === 'done' ? s.last_active_ms : undefined, activityMs: s.last_active_ms,
      }
      settled.add(row)
      out.push(row)
    }
  }

  // The records: a task's own record rides on its row; every other record, and each step view no record names, is a
  // row of its own, a plan when a record names it as parent (a project always is).
  const byId = new Map(out.map((r) => [r.id, r]))
  const parents = new Set(records.flatMap((t) => (t.parent ? [t.parent] : [])))
  // A record shares its id tail with its task's session (`tsk_X` and `ses_X`); `session` names it as well.
  const carried = (t: TaskRecord): OpsRow | undefined => {
    const r = (t.session ? byId.get(t.session) : undefined) ?? byId.get(`ses_${t.id.slice(4)}`)
    return r && (r.kind === 'task' || r.kind === 'conversation') ? r : undefined
  }
  const steps: OpsRow[] = []
  for (const t of records) {
    const own = carried(t)
    const plan = t.kind === 'project' || parents.has(t.id)
    if (own) {
      own.recordId = t.id
      own.record = t
      own.plan = plan
      continue
    }
    if (seen.has(t.id)) continue
    seen.add(t.id)
    const ended = t.state === 'done' || t.state === 'failed' || t.state === 'abandoned'
    const row = draft({
      id: t.id, kind: 'step', title: t.title, short: t.id.slice(-6), state: t.state, paused: false, recordId: t.id, record: t, plan,
      parent: t.kind === 'project' ? undefined : t.parent ?? t.origin.session, thread: undefined,
      endedMs: ended ? t.updated_at_ms : undefined, activityMs: t.updated_at_ms,
    })
    steps.push(row)
    out.push(row)
  }
  for (const v of views) {
    if ((v.kind !== 'step' && v.kind !== 'project') || seen.has(v.id)) continue
    seen.add(v.id)
    const row = draft({
      id: v.id, kind: 'step', title: v.title || v.attention.label, short: v.id.slice(-6), state: v.state, paused: v.paused === true, view: v, thread: v.thread, place: v.place,
      sessionId: v.session_id, parent: v.parent, plan: v.kind === 'project', activityMs: v.at_ms,
    })
    steps.push(row)
    out.push(row)
  }
  for (const r of out) byId.set(r.id, r)

  // The asks: each item is an ask on the first card its keys name; an item no card is named by is a row of its own.
  const named = new Map<string, OpsRow>()
  for (const r of out) for (const k of [r.id, r.view?.id, r.recordId, r.task?.info?.execution_id]) if (k && !named.has(k)) named.set(k, r)
  const loose: QueueItem[] = []
  for (const i of queue) {
    const card = askKeysOf(i).map((k) => named.get(k)).find(Boolean)
    if (card) card.asks += 1
    else loose.push(i)
  }

  const settle = (r: OpsRow, carrierInFlight?: boolean) => {
    const dueToday = (r.kind === 'wake' || r.kind === 'routine') && r.dueMs != null && r.dueMs >= now && r.dueMs < today
    // A wake or a routine run due today is scheduled, whatever its view says; a later one has no card.
    r.bucket = dueToday && r.asks === 0 ? 'scheduled' : bucketOfCard({ kind: r.kind, view: r.view, task: r.task, record: r.record, asks: r.asks, carrierInFlight }, now, calls)
    const timed = r.kind === 'wake' || r.kind === 'routine' || r.kind === 'watch'
    const onBoard = !timed || dueToday || r.asks > 0
    r.column = onBoard ? COLUMN_OF[r.bucket] : null
    // `agentsOnly` uncounts only what is drawn working: a card in Needs (a step with a goal-change ask, a failed job) stays counted.
    r.counted = r.column != null && (r.bucket !== 'working' || countedCard({ kind: r.kind, carrierInFlight }, calls))
    const v = r.view
    if (r.kind === 'task') {
      r.dueMs = r.column === 'scheduled' ? r.task?.info?.starts_at_ms ?? v?.next_at_ms : undefined
      r.endedMs = r.column === 'done' ? v?.ended_at_ms ?? r.task?.info?.updated_at_ms : undefined
    } else if (r.kind === 'conversation' || r.kind === 'job') {
      r.endedMs = r.column === 'done' ? v?.ended_at_ms ?? v?.state_since_ms : undefined
    } else if (r.kind === 'step') {
      r.endedMs = r.column === 'done' ? r.endedMs ?? r.activityMs : undefined
    }
  }
  for (const r of out) if (r.kind !== 'step' && !settled.has(r)) settle(r)
  // The time machine's sessions are read by their attention; an ask on one still makes it need the owner.
  for (const r of settled) if (r.asks > 0) { r.bucket = 'needs'; r.column = 'needs'; r.counted = true; r.rest = undefined; r.endedMs = undefined }

  // A step's carrier: its claim's holder, else the nearest task above it, else the conversation it came from.
  const recordOf = new Map(records.map((t) => [t.id, t]))
  const carrierOf = (t: TaskRecord): OpsRow | undefined => {
    if (t.claim && byId.get(t.claim.session)) return byId.get(t.claim.session)
    let up: TaskRecord | undefined = t.parent ? recordOf.get(t.parent) : undefined
    for (let i = 0; up && i < 64; i++) {
      const own = carried(up)
      if (own) return own
      up = up.parent ? recordOf.get(up.parent) : undefined
    }
    return byId.get(t.origin.session)
  }
  for (const r of steps) {
    const c = r.record ? carrierOf(r.record) : undefined
    r.carrier = c?.id
    const inFlight = c?.bucket === 'working'
    settle(r, r.record?.state === 'in_progress' ? inFlight : undefined)
  }

  for (const i of loose) {
    out.push({
      id: i.key, kind: 'ask', bucket: 'needs', column: 'needs', counted: true, asks: 1, title: i.title, short: i.key.slice(-6), state: 'needs_you', paused: false,
      thread: i.view?.thread ?? i.thread?.slug, sessionId: i.sessionId, item: i, activityMs: i.since,
    })
  }
  for (const r of out) r.data = r.view?.data ?? r.session?.data
  return out
}

/** A row's time of need: within Needs you the longest waiting comes first, as in the queue. */
const needSince = (r: OpsRow): number => r.view?.attention.since_ms ?? r.task?.info?.attention?.since_ms ?? r.activityMs

/** Done shows its older rows fifty at a time: "more (N older)". */
export const DONE_PAGE = 50

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
    if (r.column == null) continue
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

/** Does a counter's filter hold `r`? `spentToday` is the sessions that spent since local midnight. A row with no column
 *  (a to-do step, a later wake) is on no counter, so no counter's filter keeps it. The filter reads the row's bucket,
 *  the one the counters count. */
export function inBucket(r: OpsRow, b: Bucket, now: number, spentToday: ReadonlySet<string>): boolean {
  if (r.column == null) return false
  switch (b) {
    case 'needs': return r.bucket === 'needs'
    case 'working': return r.bucket === 'working'
    case 'paused': return r.bucket === 'paused'
    case 'still': return r.bucket === 'scheduled'
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
  if (f.data && (r.data ?? 'open') !== f.data) return false
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

/** Does `sel` name this row? A task by its session, view, execution, record or short id; anything else by its id or view. */
export function selects(r: OpsRow, sel: string | null): boolean {
  if (!sel) return false
  return sel === r.id || sel === r.view?.id || sel === r.recordId || sel === r.short || sel === r.task?.info?.execution_id || (r.kind === 'task' && sel === r.task?.short)
}

/** The counters the bar shows, but needs you. `spent` is today's dollars. */
export interface Counters { working: number; paused: number; still: number; done: number; spent: number }

/** The bar's six counters, each a count of the work set's rows (`boardRows`), every card once. A counter counts the
 *  `counted` rows of its bucket and nothing else:
 *    needs you        cards waiting on the owner: an ask on them (approval, question, budget, failure not dismissed, a
 *                     step's goal change), or a record waiting_human or blocked; plus the asks no card owns. One per
 *                     card (`needsPerCard`), else one per ask, as the Needs-you list's length
 *    working          tasks in flight (running, or waiting on their own calls, jobs or children) and conversations with a
 *                     turn in flight; with `agentsOnly`, not a job nor a step being done by an agent already counted
 *    paused·queued    owner-paused tasks, queued executions, waits on a time after today, suspended records, and a step
 *                     doing that nobody works (`stalledIsPaused`); accepted records too unless `todoApart`
 *    scheduled today  cards due before local midnight: deferred starts, wakes and routine runs, sleeping waits
 *    done today       cards that ended since local midnight: tasks, conversations that ended, steps closed
 *    spent today      dollars, unchanged
 *  `countersOf` is these but needs you (the shape the Board's tests hold); `allCounters` is all six. */
export function countersOf(rows: readonly OpsRow[], now: number, spentUsd: number): Counters {
  const today = midnight(now)
  const n = (b: StateBucket) => rows.filter((r) => r.counted && r.column != null && r.bucket === b).length
  return { working: n('working'), paused: n('paused'), still: n('scheduled'), done: rows.filter((r) => r.counted && r.column === 'done' && (r.endedMs ?? r.activityMs) >= today).length, spent: spentUsd }
}

/** Needs you: pieces of work (`needsPerCard`), or every ask separately. */
export function needsOf(rows: readonly OpsRow[], calls: OwnerCalls = OWNER_CALLS): number {
  const cards = rows.filter((r) => r.counted && r.column != null && r.bucket === 'needs')
  return calls.needsPerCard ? cards.length : cards.reduce((n, r) => n + r.asks, 0)
}

export interface AllCounters extends Counters { needs: number }

export function allCounters(rows: readonly OpsRow[], now: number, spentUsd: number, calls: OwnerCalls = OWNER_CALLS): AllCounters {
  return { needs: needsOf(rows, calls), ...countersOf(rows, now, spentUsd) }
}

/** A counter's words: Q8 renames "still today" to the Board's column. */
export function counterLabel(b: Bucket, calls: OwnerCalls = OWNER_CALLS): string {
  switch (b) {
    case 'needs': return 'needs you'
    case 'working': return 'working'
    case 'paused': return 'paused·queued'
    case 'still': return calls.scheduledLabel ? 'scheduled today' : 'still today'
    case 'done': return 'done today'
    case 'spent': return 'spent today'
  }
}

/** The filter chip on the canvas: the counter's words and its number, `paused·queued · 4`, and its × clears it. */
export const chipWords = (b: Bucket, n: number, calls: OwnerCalls = OWNER_CALLS): string => `${counterLabel(b, calls)} · ${n}`

/** What a counter's click keeps as faint context: each kept card's ancestors up to the thread's head, and the jobs and
 *  the carried steps of kept cards. Never a kept card itself. */
export function contextOf(rows: readonly OpsRow[], kept: ReadonlySet<string>): Set<string> {
  const named = new Map<string, OpsRow>()
  for (const r of rows) for (const k of [r.id, r.view?.id, r.recordId, r.sessionId && r.kind !== 'job' ? r.sessionId : undefined, r.task?.info?.execution_id]) if (k && !named.has(k)) named.set(k, r)
  const out = new Set<string>()
  const add = (r: OpsRow | undefined) => { if (r && !kept.has(r.id)) out.add(r.id) }
  for (const r of rows) {
    if (!kept.has(r.id)) continue
    let up = r.parent ? named.get(r.parent) : undefined
    for (let i = 0; up && i < 64; i++) { add(up); up = up.parent ? named.get(up.parent) : undefined }
    if (r.carrier) add(named.get(r.carrier))
  }
  for (const r of rows) {
    if (kept.has(r.id)) continue
    const host = r.parent ? named.get(r.parent) : undefined
    if (r.kind === 'job' && host && kept.has(host.id)) out.add(r.id)
    if (r.kind === 'step' && r.carrier && kept.has(r.carrier)) out.add(r.id)
  }
  return out
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

/** The data marks a chip offers: always all three, since a lock is the point. */
export const DATA_MARKS: readonly DataMark[] = ['open', 'private', 'sensitive']

/** Is it a lock: private or sensitive data? */
export const locked = (r: { data?: DataMark }): boolean => r.data === 'private' || r.data === 'sensitive'

/** The chips' choices, from the rows on the board: each distinct value, in order. */
export function chipChoices(rows: readonly OpsRow[]): { thread: string[]; model: string[]; place: string[]; kind: string[]; state: string[] } {
  const of = (f: (r: OpsRow) => string | undefined) => [...new Set(rows.map(f).filter((x): x is string => !!x))].sort()
  return { thread: of((r) => r.thread), model: of((r) => r.model), place: of((r) => r.place), kind: of((r) => r.kind), state: of((r) => r.state) }
}

/** Is a row's task alive (not ended)? Its acts (pause, stop) are only for the living. */
export const alive = (r: OpsRow): boolean => r.column !== 'done' && !(r.task?.info && ENDED_INFO.includes(r.task.info.state) && !r.view)

/** The columns as the page moves through them (`h`/`l`), Done holding today and the older page shown. */
export function laneRows(c: BoardColumns, older: number): OpsRow[][] {
  return [c.needs, c.working, c.paused, c.scheduled, [...c.done, ...c.older.slice(0, older)]]
}
