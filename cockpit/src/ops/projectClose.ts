// What closing a project says (issue 37): the counts the sheet shows before it asks, and the words of the answer. Pure, so
// `npm test` runs it; the terminal's `close_counts`, `close_card` and `project_closed` say the same things.
import type { ProjectCloseResult, TaskRecord } from '@protocol'

const many = (n: number, one: string, more: string) => `${n} ${n === 1 ? one : more}`

/** What a close would end, counted from the project's records: its open items, open milestones, and how many of those
 *  milestones have a date (so a reminder). The terminal's `close_counts` counts the same way. */
export function closeCounts(records: readonly TaskRecord[]): { items: number; milestones: number; dated: number } {
  const open = (kind: string) => records.filter((t) => (t.kind ?? 'item') === kind && !['done', 'failed', 'abandoned'].includes(t.state))
  return { items: open('item').length, milestones: open('milestone').length, dated: open('milestone').filter((t) => t.due_at_ms != null).length }
}

/** What the sheet says will end and stop. */
export function closeWords(c: { items: number; milestones: number; dated: number }, review: boolean): string {
  const stops = [review && 'its weekly review', c.dated > 0 && `the reminders of ${many(c.dated, 'dated milestone', 'dated milestones')}`].filter(Boolean)
  return `Ends ${many(c.items, 'open item', 'open items')} and ${many(c.milestones, 'open milestone', 'open milestones')}${stops.length ? ` and stops ${stops.join(' and ')}` : ''}. It does not stop a task that carries part of the project.`
}

/** What a close says it did, as the terminal does (`project_closed`). */
export function closedWords(r: ProjectCloseResult): string {
  const base = `${many(r.items_ended, 'item', 'items')} and ${many(r.milestones_ended, 'milestone', 'milestones')} ended`
  const stopped = [r.review_stopped ? 'its weekly review' : '', r.reminders_cancelled > 0 ? many(r.reminders_cancelled, 'reminder', 'reminders') : ''].filter(Boolean)
  return `Project ${r.how === 'done' ? 'marked done' : 'dropped'}: ${base}${stopped.length ? `; ${stopped.join(' and ')} stopped` : ''}.`
}

