// The Operations page's one state model (theseus-g00vv): every surface (the Board, the Graph, the Threads lens, the
// queue and the counters) asks this module which bucket a piece of work is in, and nothing else maps a state to one.
// The owner's rules: waiting counts as open; opacity marks the ended alone; WORKING is work in flight; an external-text
// mark that makes nothing wait is a notice (`ops/notices.ts`), not an ask.
//
//   needs     it asks for the owner: a question, an approval, a failure not dismissed, a record waiting on a person
//   working   work in flight: running, or waiting on its own calls, jobs or a child
//   scheduled it waits on a time due before local midnight
//   paused    paused, queued, or waiting on a later time; a suspended record is here, open and recoverable
//   rest      a conversation between exchanges: neither open nor ended, drawn at full opacity
//   ended     done, cancelled, expired, or failed and dismissed: the only bucket drawn faded
//
// Pure: `npm test` runs it (test/ops-state.test.ts), and it imports only the protocol's types and other pure modules.
import type { SessionInfo, TaskRecord, WorkView } from '@protocol'
import { midnight } from '../lib/agenda.ts'

export type StateBucket = 'needs' | 'working' | 'scheduled' | 'paused' | 'rest' | 'ended'

/** Open work: waiting counts. */
export const isOpen = (b: StateBucket): boolean => b === 'needs' || b === 'working' || b === 'scheduled' || b === 'paused'
export const isEnded = (b: StateBucket): boolean => b === 'ended'

/** The one faded class, and the one treatment of a node the filters leave out: a dashed outline, never opacity. */
export const FADED = 'opacity-[0.55]'
export const FILTERED_OUT = 'outline outline-1 outline-dashed outline-offset-2 outline-ink-faint/70 saturate-50'
export const look = (b: StateBucket, filteredOut: boolean): { faded: boolean; filteredOut: boolean } => ({ faded: isEnded(b), filteredOut })

/** The local midnight ending the day that holds `now`. */
export const endOfDay = (now: number): number => { const d = new Date(midnight(now)); return new Date(d.getFullYear(), d.getMonth(), d.getDate() + 1).getTime() }

/** A time the work waits on: today's is scheduled, a later one is paused. */
const byTime = (at: number | undefined, now: number): StateBucket => (at != null && at < endOfDay(now) ? 'scheduled' : 'paused')

const TIMED = ['wake', 'routine', 'watch']

/** A view's bucket. `paused` and the state come before the attention level: a paused view reads `idle` there, and idle
 *  does not mean ended. */
export function bucketOf(v: WorkView, now: number): StateBucket {
  if (v.state === 'failed') return v.attention.level === 'needs_you' && !v.dismissed ? 'needs' : 'ended'
  if (v.state === 'done' || v.state === 'cancelled' || v.state === 'expired') return 'ended'
  if (v.state === 'needs_you' || (v.attention.level === 'needs_you' && !v.dismissed)) return 'needs'
  // A conversation the daemon calls ready is at rest, whatever its kernel state (the time machine's view says waiting).
  if (v.state === 'ready' || (v.kind === 'conversation' && v.attention.level === 'ready')) return 'rest'
  if (v.paused === true) return 'paused'
  // Running is work in flight, a routine's or a watch's fire included: before the time checks.
  if (v.state === 'running') return 'working'
  if (v.state === 'queued') return byTime(v.next_at_ms, now)
  if (TIMED.includes(v.kind)) return byTime(v.next_at_ms, now)
  if (v.state === 'waiting' && v.doing?.what === 'sleeping') return byTime(v.next_at_ms, now)
  if (v.state === 'waiting') return 'working'
  return 'rest'
}

/** A task record's bucket, where the board holds no view of it. */
export function bucketOfRecord(state: string): StateBucket {
  switch (state) {
    case 'waiting_human': case 'blocked': return 'needs'
    case 'in_progress': return 'working'
    case 'done': case 'failed': case 'abandoned': return 'ended'
    default: return 'paused' // suspended, accepted, proposed: open, and recoverable
  }
}

const ENDED_EXECUTION = ['complete', 'cancelled', 'failed', 'budget_exhausted']

/** A session's bucket from its attention and its execution's state: the time machine's board, which holds no views.
 *  `ready` is at rest, never working. */
export function bucketOfSession(s: Pick<SessionInfo, 'attention' | 'execution_state'>): StateBucket {
  const st = s.execution_state ?? 'idle'
  switch (s.attention?.level) {
    case 'needs_you': return 'needs'
    case 'ready': return 'rest'
    case 'working': return st === 'queued' ? 'paused' : 'working'
    default: break
  }
  if (st === 'running') return 'working'
  if (st === 'queued') return 'paused'
  if (ENDED_EXECUTION.includes(st)) return 'ended'
  return 'rest'
}

/** A view with its task record: a suspended record whose execution ended is paused·queued, open and recoverable,
 *  never working and never faded (the spec: a suspended task can be resumed). */
export function bucketOfWork(v: WorkView, now: number, record?: Pick<TaskRecord, 'state'>): StateBucket {
  const b = bucketOf(v, now)
  return b === 'ended' && record?.state === 'suspended' ? 'paused' : b
}

/** Is the work open? Whether it is open does not depend on the clock (scheduled and paused are both open), so the
 *  Threads lens asks without one. */
export const openView = (v: WorkView, record?: Pick<TaskRecord, 'state'>): boolean => isOpen(bucketOfWork(v, 0, record))

/** A node of the Graph: its view's bucket (with its record), else its record's, else what a fold stands for. */
export function bucketOfNode(n: { kind: string; state: string; view?: WorkView; record?: Pick<TaskRecord, 'state'> }, now: number): StateBucket {
  if (n.view) return bucketOfWork(n.view, now, n.record)
  if (n.record) return bucketOfRecord(n.record.state)
  return n.kind === 'done' ? 'ended' : 'rest'
}

/** The tone a bucket sets over its state's own: open work that waits is amber, work in flight is cyan. Absent, the
 *  state's tone stands (an ended one's ✓ or ✗). A record's `suspended` or `accepted` is no longer grey. */
export function bucketTone(b: StateBucket, state: string): 'wait' | 'live' | undefined {
  if (b === 'needs' || b === 'paused' || b === 'scheduled') return 'wait'
  if (b === 'working' && (state === 'running' || state === 'in_progress')) return 'live'
  return undefined
}

/** The glyph a paused or scheduled bucket wears: ⏸ for a pause, ◷ for a wait on a time or a turn in a queue. */
export function bucketGlyph(b: StateBucket, state: string): string | undefined {
  if (b === 'scheduled') return '◷'
  if (b !== 'paused') return undefined
  return state === 'paused' || state === 'suspended' ? '⏸' : '◷'
}

/** A view's state is a turn in flight, the only thing `execution.stop` ends. `waiting` is open work (WORKING) but not a
 *  turn to stop: it sleeps, or waits on a job or a child. `executionState` is the session's own, when no view is held. */
export const isRunning = (viewState: string | undefined, executionState?: string): boolean => viewState === 'running' || executionState === 'running'

/** The view states that mean the work ended (a failure not yet dismissed still asks: that is `bucketOf`'s, not this). */
export const ENDED_VIEW_STATES: readonly string[] = ['done', 'failed', 'cancelled', 'expired']
export const isEndedState = (state: string | undefined): boolean => state !== undefined && ENDED_VIEW_STATES.includes(state)
