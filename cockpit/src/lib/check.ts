// A check task's basis in words (M5 28a): the same line as `TaskCheck::line` in theseus-protocol, so the cockpit's task
// view, `theseus tasks`, and the report's post on Discord say one thing.
import type { TaskCheck } from '@protocol'

/** `ses_…a1b2c3`: a session by the end of its id. */
function shortSession(id: string): string {
  const tail = [...id].slice(-6).join('')
  const i = id.indexOf('_')
  return i >= 0 ? `${id.slice(0, i)}_…${tail}` : `…${tail}`
}

/** `🔍 check of task a1b2c3 · independent (excluded ses_…a1b2c3, glm-4.6)`, and `· overlap: N spans` when flagged. */
export function checkLine(c: TaskCheck): string {
  const excluded = c.excluded_sessions.map(shortSession).join(', ')
  const n = c.overlaps?.length ?? 0  // absent when none: the wire leaves an empty list out
  const overlap = n === 0 ? '' : ` · overlap: ${n} span${n === 1 ? '' : 's'}`
  return `🔍 check of task ${c.checked_short} · independent (excluded ${excluded}, ${c.model})${overlap}`
}
