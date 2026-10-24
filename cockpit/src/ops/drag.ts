// Drag on the Timeline (theseus-jid7t.5; the design's §2.7): what a drop does, one call with its Undo, or why it is
// refused, said on the drag cursor. Pure: `npm test` runs it (test/ops-drag.test.ts).
//
//   a reminder or check-in ● ○   onto a time   moves the wake       wake.snooze { wake, when: <ISO> }       Undo: the time before
//   a deferred start ▷            onto a time   moves its start      task.reschedule { task, start_at_ms }   Undo: its `previous`
//
// Everything else is refused: ended work, a routine's runs (its schedule is edited in a form), watches, milestones,
// calendar events (the page never writes to Google), and anything under the time machine. A drop snaps to 5 minutes of
// wall-clock time; `when` is the instant in UTC (RFC 3339), so the local time the owner dropped on is the time it moves
// to, DST included.
import type { Mark } from '../lib/agenda.ts'
import type { TaskRescheduleParams, TaskRescheduleResult } from '@protocol'

export const SNAP_MIN = 5

/** `at` snapped to the nearest 5 minutes of the local clock. */
export function snap(at: number): number {
  const d = new Date(at)
  const mins = Math.round((d.getHours() * 60 + d.getMinutes() + d.getSeconds() / 60) / SNAP_MIN) * SNAP_MIN
  return new Date(d.getFullYear(), d.getMonth(), d.getDate(), 0, mins).getTime()
}

/** The instant a drop names: a day's local midnight and a wall-clock hour (fractional), snapped. A wall-clock time a
 *  spring-forward day skips lands on the hour after it, as the clock does. */
export function dropAt(dayStart: number, hour: number): number {
  const d = new Date(dayStart)
  return snap(new Date(d.getFullYear(), d.getMonth(), d.getDate(), 0, Math.round(Math.max(0, Math.min(24, hour)) * 60)).getTime())
}

export interface Call { method: string; params: Record<string, unknown> }

export type Drop =
  | { method: 'wake.snooze' | 'task.reschedule'; params: Record<string, unknown>; words: string; undo: (result: unknown) => Call }
  | { refused: string }

export const PAST_REFUSAL = 'the past: acting needs the present'

/** What dropping `what` at `at` does. `past` is the time machine; `now` refuses a time already gone. */
export function dropOf(what: Mark | { event: true }, at: number, past: boolean, now: number): Drop {
  if (past) return { refused: PAST_REFUSAL }
  if ('event' in what) return { refused: 'a calendar event: the page never writes to Google Calendar' }
  const m = what
  switch (m.kind) {
    case 'run': return { refused: 'a routine’s schedule is edited in its form (routine.update), not by drag' }
    case 'past': return { refused: 'ended work stays where it ran' }
    case 'watch': return { refused: 'a watch keeps its window: nothing edits one' }
    case 'milestone': return { refused: 'a milestone moves with its project, not here' }
    case 'reminder':
    case 'check':
    case 'start': break
  }
  if (m.at_ms <= now && m.kind === 'start') return { refused: 'it has started: ended or running work is not moved' }
  if (at <= now) return { refused: 'that time has passed' }
  if (at === m.at_ms) return { refused: 'it is already then' }
  if (m.kind === 'start') {
    const params: TaskRescheduleParams = { task: m.id, start_at_ms: at }
    return {
      method: 'task.reschedule', params, words: `starts ${words(at)}`,
      undo: (r) => ({ method: 'task.reschedule', params: { task: m.id, start_at_ms: (r as Partial<TaskRescheduleResult> | undefined)?.previous ?? m.at_ms } }),
    }
  }
  const before = m.at_ms
  return {
    method: 'wake.snooze', params: { wake: m.id, when: new Date(at).toISOString() }, words: `moved to ${words(at)}`,
    undo: () => ({ method: 'wake.snooze', params: { wake: m.id, when: new Date(before).toISOString() } }),
  }
}

const pad = (n: number) => String(n).padStart(2, '0')
const WEEKDAYS = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'] as const
function words(at: number): string {
  const d = new Date(at)
  return `${WEEKDAYS[d.getDay()]} ${pad(d.getHours())}:${pad(d.getMinutes())}`
}
