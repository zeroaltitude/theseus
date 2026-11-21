// The tasks involved in a project, as the panel lists them (issue 37): each a title and one line of state, when it started
// and ended, and how it was involved, in the page's relative style. Pure, so `npm test` runs it.
import type { ProjectTask } from '@protocol'
import { ago } from '../../lib/figures.ts'

export interface TaskRow {
  /** The task's session, which the page selects it by. */
  id: string
  title: string
  sub: string
  running: boolean
}

/** The rows of the "Tasks" section, in the order the daemon sent them (those still going first, then the newest). */
export function taskRows(tasks: readonly ProjectTask[], now: number): TaskRow[] {
  return tasks.map((t) => ({
    id: t.task_id,
    title: t.title || t.short,
    sub: [t.state, `started ${ago(t.started_at_ms, now)}`, t.ended_at_ms != null ? `ended ${ago(t.ended_at_ms, now)}` : '', t.how].filter(Boolean).join(' · '),
    running: !t.ended,
  }))
}

/** The section's hint: how many were involved and how many still go, or why there is nothing. */
export function taskHint(total: number, running: number): string {
  return total === 0 ? 'none yet' : `${total} ${total === 1 ? 'task' : 'tasks'}${running > 0 ? `, ${running} running` : ''}`
}
