// The Tasks page's rules (theseus-ypnxt.6). Pure: `npm test` runs it (test/taskboard.test.ts). A task is a row made of
// its work view (the live board's: state, paused, progress, report) and its record (`task.list`'s: the session, the
// place it reports to, its spend); either may be missing. The page shows three sections, the newest activity first:
// what needs the owner, what is working (running, waiting on its own command, paused), and what is done.
import type { TaskInfo, WorkView } from '@protocol'

export type Section = 'needs' | 'working' | 'done'

export interface TaskRow {
  /** The task's session id (`ses_…`): what `task.pause`, `task.cancel` and `/session/<id>` take. */
  id: string
  short: string
  title: string
  section: Section
  paused: boolean
  /** The place it lives or reports to, by name (`discord dm`), and the raw target for a tooltip. */
  place?: string
  target?: string
  view?: WorkView
  info?: TaskInfo
  /** The report's first line, or why it ended. */
  reportLine?: string
  activityMs: number
}

const ENDED_VIEW = ['done', 'failed', 'cancelled', 'expired']
// The kernel's terminal states (ExecState::is_terminal), as Actions reads them.
const ENDED_INFO = ['complete', 'cancelled', 'failed', 'budget_exhausted']
const WORKING_VIEW = ['running', 'waiting', 'queued']

/** The place rule's name for a report target: `discord:dm:123456` → `discord dm`; the id is the tooltip's. */
export function placeName(target: string | undefined): string | undefined {
  if (!target) return undefined
  const parts = target.split(':')
  return (parts.length >= 3 ? parts.slice(0, -1) : parts).join(' ')
}

const sectionOf = (view: WorkView | undefined, info: TaskInfo | undefined): Section => {
  if (view) return ENDED_VIEW.includes(view.state) ? 'done' : view.attention.level === 'needs_you' ? 'needs' : 'working'
  if (!info) return 'working'
  return ENDED_INFO.includes(info.state) ? 'done' : info.pending_confirms > 0 ? 'needs' : 'working'
}

/** Every task, once: the board's task views joined to `task.list`'s records by session, newest activity first. `lines`
 *  are report lines read from ended tasks' sessions (`reportLineOf`), for those the board no longer holds. */
export function rowsOf(views: WorkView[], tasks: TaskInfo[], lines: Readonly<Record<string, string>> = {}): TaskRow[] {
  const bySession = new Map(views.filter((v) => v.kind === 'task' && v.session_id).map((v) => [v.session_id as string, v]))
  const rows: TaskRow[] = []
  const row = (id: string, view?: WorkView, info?: TaskInfo): TaskRow => ({
    id,
    short: info?.short ?? id.slice(-6),
    title: view?.title || info?.title || `task ${info?.short ?? id.slice(-6)}`,
    section: sectionOf(view, info),
    paused: view?.paused === true,
    place: view?.place ?? placeName(info?.target),
    target: info?.target,
    view,
    info,
    reportLine: view?.report?.first_line ?? view?.report?.reason ?? lines[id] ?? info?.ended_reason,
    activityMs: Math.max(view?.at_ms ?? 0, info?.updated_at_ms ?? 0),
  })
  for (const t of tasks) {
    rows.push(row(t.task_id, bySession.get(t.task_id), t))
    bySession.delete(t.task_id)
  }
  for (const [id, v] of bySession) rows.push(row(id, v))
  return rows.sort((a, b) => b.activityMs - a.activityMs || (a.id < b.id ? -1 : 1))
}

/** The first line of a task's report, from the tail of its session: its last assistant message, which is the report it
 *  ended with. For an ended task the live board no longer holds (a page opened after it ended). */
export function reportLineOf(nodes: ReadonlyArray<{ kind: string; text?: string | null }> | undefined): string | undefined {
  const last = [...(nodes ?? [])].reverse().find((n) => n.kind === 'assistant_message' && n.text)
  return last?.text?.split('\n').map((l) => l.trim()).find(Boolean)
}

/** A search over title, report line and short id: every word must be found, in any case. */
export function matches(r: TaskRow, query: string): boolean {
  const words = query.toLowerCase().split(/\s+/).filter(Boolean)
  const hay = `${r.title}\n${r.reportLine ?? ''}\n${r.short}`.toLowerCase()
  return words.every((w) => hay.includes(w))
}

export function split(rows: TaskRow[], query = ''): Record<Section, TaskRow[]> {
  const out: Record<Section, TaskRow[]> = { needs: [], working: [], done: [] }
  for (const r of rows) if (matches(r, query)) out[r.section].push(r)
  return out
}

/** How many tasks are working, for the header: running, waiting on their own command, or paused. A task that waits
 *  for the owner is counted in "need you", and an ended one not at all. */
export function workingCount(views: WorkView[]): number {
  return views.filter((v) => v.kind === 'task' && !ENDED_VIEW.includes(v.state) && (v.paused === true || WORKING_VIEW.includes(v.state))).length
}

/** Where a desktop notice opens: a task's row on the Tasks page, anything else's session. */
export function noticeTarget(work: { id: string; kind: string; session_id?: string }): string | undefined {
  if (work.kind === 'task') return `/tasks?sel=${work.session_id ?? work.id}`
  return work.session_id ? `/session/${work.session_id}` : undefined
}
