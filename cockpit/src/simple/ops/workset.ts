// The simple Ops page's work set, as one pure function (GitHub issue 17): the Operations page's own steps (`queueOf`,
// `boardRows`, `columnsOf`, `allCounters`, `spendToday`) over what the page has read, and nothing of its own but the
// lists' order and the conversations at rest. `test/simple-ops-parity.test.ts` runs it and the Ship's pipeline over the
// same fixtures and holds that every number here is the Ship's. Pure: `npm test` runs it.
import type { ConfirmRequest, SessionInfo, TaskInfo, TaskRecord, ThreadGroup, WorkView } from '../../protocol.gen/index.ts'
import { allCounters, boardRows, columnsOf, spendToday, unaskedCards, type AllCounters, type BoardColumns, type OpsRow } from '../../ops/board.ts'
import { queueOf, type QueueItem } from '../../ops/queue.ts'
import { rowFor } from '../../ops/sections.ts'
import { stateOf } from '../../lib/sessionState.ts'

export interface WorkSetInput {
  views: readonly WorkView[]
  tasks: readonly TaskInfo[]
  sessions: readonly SessionInfo[]
  records: readonly TaskRecord[]
  confirms: readonly ConfirmRequest[]
  groups: readonly ThreadGroup[]
  /** The ledger's rows, for today's spend. */
  history: ReadonlyArray<{ kind: string; at_unix_ms: number; session_id?: string | null; data?: unknown }>
  now: number
}

export interface WorkSet {
  queue: QueueItem[]
  rows: OpsRow[]
  cols: BoardColumns
  counts: AllCounters
  spendUsd: number
  /** Cards that wait on the owner with no ask to answer, listed after the asks. */
  waiting: OpsRow[]
  /** In flight: the counted working cards, so the list is the counter. A job, or a step an agent already counted, is
   *  working too, and is one click inside the card that carries it. */
  flight: OpsRow[]
  /** Working rows the counter leaves out (a job, a step an agent already counts) whose carrying card is not itself in
   *  flight, such as a background job under a conversation at rest: listed after `flight`, so nothing the Ship's Board
   *  shows under Working is out of reach of this page. */
  loose: OpsRow[]
  /** Planned and not started: the steps no one has begun, those under running work first. */
  next: OpsRow[]
  /** The conversations at rest, the most recently active first. */
  resting: SessionInfo[]
  /** Planned: the steps not begun, what is on the clock, what is held. */
  planned: number
  /** What the lists show, top to bottom: the cursor `j` and `k` walk. A queue item's key, a row's id, a session's id. */
  order: string[]
}

export function workSetOf(i: WorkSetInput): WorkSet {
  const titleOf = (sid: string) => { const s = i.sessions.find((x) => x.session_id === sid); return s?.title || s?.label || sid.slice(-6) }
  const queue = queueOf({ confirms: [...i.confirms], views: [...i.views], records: [...i.records], groups: [...i.groups], past: false, titleOf })
  const rows = boardRows({ views: [...i.views], tasks: [...i.tasks], sessions: [...i.sessions], records: [...i.records], queue, now: i.now, past: false })
  const cols = columnsOf(rows, i.now)
  const spendUsd = spendToday(i.history, i.now).usd
  const counts = allCounters(rows, i.now, spendUsd)
  const waiting = unaskedCards(rows)
  const flight = cols.working.filter((r) => r.counted)
  // A working row inside a card that is in flight is one click inside that card; one whose card is not in flight (a job
  // started by a conversation whose turn has ended) has no card to be inside, so it is listed on its own.
  const carried = new Set(flight.map((r) => r.id))
  const hostOf = (r: OpsRow): OpsRow | undefined => (r.carrier && rowFor(rows, r.carrier)) || (r.parent && rowFor(rows, r.parent)) || undefined
  const loose = cols.working.filter((r) => !r.counted && !carried.has(hostOf(r)?.id ?? ''))
  // Planned and not yet started: the steps no one has begun, those whose plan is in flight first (the nodes that
  // come after the running ones in the graph), then the rest, newest first.
  const inFlight = new Set(cols.working.map((r) => r.id))
  const after = (r: OpsRow) => (r.carrier && inFlight.has(r.carrier)) || (r.parent && inFlight.has(rowFor(rows, r.parent)?.id ?? '')) ? 0 : 1
  const next = rows.filter((r) => r.bucket === 'todo').sort((a, b) => after(a) - after(b) || b.activityMs - a.activityMs || (a.id < b.id ? -1 : 1))
  // The conversations at rest: nothing runs in them, and any can be talked to. The board holds only work, so these are
  // `session.list`'s, less the ones a list above already shows.
  const listed = new Set(rows.filter((r) => r.column != null && r.column !== 'done').map((r) => r.sessionId ?? r.id))
  const resting = i.sessions.filter((x) => x.kind === 'conversation' && stateOf(x) !== 'retired' && !listed.has(x.session_id))
    .sort((a, b) => b.last_active_ms - a.last_active_ms || (a.session_id < b.session_id ? -1 : 1))
  const planned = next.length + cols.scheduled.length + cols.paused.length
  const order = [...queue.map((q) => q.key), ...waiting.map((r) => r.id), ...flight.map((r) => r.id), ...loose.map((r) => r.id), ...next.map((r) => r.id), ...cols.scheduled.map((r) => r.id), ...cols.paused.map((r) => r.id),
    ...resting.map((s) => s.session_id), ...cols.done.map((r) => r.id)]
  return { queue, rows, cols, counts, spendUsd, waiting, flight, loose, next, resting, planned, order }
}

/** What an address's `sel` names: a queue item, else a row of the work set, else a conversation at rest. */
export function selectedOf(w: WorkSet, sessions: readonly SessionInfo[], sel: string | null): { item?: QueueItem; row?: OpsRow; session?: SessionInfo } {
  const item = sel ? w.queue.find((q) => q.key === sel) : undefined
  if (item) return { item }
  const row = rowFor(w.rows, sel)
  if (row) return { row }
  return { session: sel ? sessions.find((x) => x.session_id === sel) : undefined }
}

/** What is open when the address names nothing: the first that waits, else the newest work in flight, else the
 *  conversation last spoken in. */
export const defaultSel = (w: WorkSet): string | null => w.queue[0]?.key ?? w.waiting[0]?.id ?? w.flight[0]?.id ?? w.loose[0]?.id ?? w.resting[0]?.session_id ?? null

/** The entry `by` places on from `at` in the order the lists show (`j` is 1, `k` is -1), stopping at either end; from
 *  nothing, the first or the last. */
export function stepBy(order: readonly string[], at: string | null, by: 1 | -1): string | null {
  if (order.length === 0) return null
  const i = at == null ? -1 : order.indexOf(at)
  if (i < 0) return order[by > 0 ? 0 : order.length - 1]
  return order[Math.max(0, Math.min(order.length - 1, i + by))]
}
