// A long-horizon task's plan, its next steps and what it is waiting on (GitHub issue 17, "horizon task management"): one
// reading of `project.show`'s records, drawn by both Operations pages (the Inspector holds it, and both pages draw the
// Inspector), and the two calls that change it, `project.item.add` and `project.item.close` (issue 13's per-project
// item list). A project is a task record of kind `project`; its milestones are dated records under it and its items are
// records of kind `item` under it, so nothing here is a second list. Pure: `npm test` runs it (test/simple-ops-horizon.test.ts).
import type { ProjectInfo, TaskRecord } from '../../protocol.gen/index.ts'

/** What `project.item.add`, `.close` and `.list` answer (issue 13): the project, and the whole list as it now stands. */
export interface ItemsResult { project: string; title: string; items: TaskRecord[]; changed?: TaskRecord }

/** The longest an item's one line may be: the daemon's own limit. */
export const ITEM_MAX = 200

const ENDED: ReadonlySet<string> = new Set(['done', 'failed', 'abandoned'])

/** A state in a person's words. */
const STATE_WORDS: Readonly<Record<string, string>> = {
  proposed: 'proposed', accepted: 'not started', in_progress: 'being worked', blocked: 'blocked', waiting_human: 'waiting for a person',
  suspended: 'on hold', done: 'done', failed: 'failed', abandoned: 'dropped',
}
export const stateWord = (state: string): string => STATE_WORDS[state] ?? state

export interface HorizonStep {
  id: string
  title: string
  state: string
  word: string
  /** Its date, unix ms, for a milestone. */
  due?: number
  overdue: boolean
  /** An item on the project's own list: the kind `project.item.close` ends. */
  item: boolean
}

export interface Horizon {
  id: string
  title: string
  objective: string
  acceptance: string[]
  /** Steps and milestones done of total, from the daemon's own count. */
  done: number
  total: number
  /** The milestones, soonest first, the closed last. */
  plan: HorizonStep[]
  /** Open steps and items no one has begun, in the order the plan holds them. */
  next: HorizonStep[]
  /** Open steps and items being worked now. */
  doing: HorizonStep[]
  /** What it is waiting on, each with the reason in words. */
  waiting: Array<{ id: string; title: string; why: string }>
}

const isOpen = (r: TaskRecord) => !ENDED.has(r.state)

/** A project as the owner reads it: from `project.show`'s project and the records under it (depth first). */
export function horizonOf(project: ProjectInfo, records: readonly TaskRecord[], now: number): Horizon {
  const own = records.filter((r) => r.id !== project.id)
  const byId = new Map(records.map((r) => [r.id, r]))
  const step = (r: TaskRecord): HorizonStep => ({
    id: r.id, title: r.title, state: r.state, word: stateWord(r.state), due: r.due_at_ms,
    overdue: r.kind === 'milestone' && isOpen(r) && r.due_at_ms != null && r.due_at_ms < now, item: r.kind == null || r.kind === 'item',
  })
  const milestones = own.filter((r) => r.kind === 'milestone')
  const plan = milestones.map(step).sort((a, b) => Number(ENDED.has(a.state)) - Number(ENDED.has(b.state)) || (a.due ?? Infinity) - (b.due ?? Infinity) || (a.id < b.id ? -1 : 1))
  const steps = own.filter((r) => r.kind !== 'milestone' && r.kind !== 'project' && isOpen(r))
  const begun = (r: TaskRecord) => r.state === 'in_progress'
  const waits = (r: TaskRecord) => r.state === 'blocked' || r.state === 'waiting_human'
  const next = steps.filter((r) => !begun(r) && !waits(r)).map(step)
  const doing = steps.filter(begun).map(step)
  const waiting: Horizon['waiting'] = []
  for (const r of [...milestones.filter(isOpen), ...steps]) {
    const why: string[] = []
    if (r.state === 'waiting_human') why.push('it waits for a person')
    else if (r.state === 'blocked') why.push('it is blocked')
    const pending = r.deps.map((d) => byId.get(d)).filter((d): d is TaskRecord => !!d && isOpen(d))
    if (pending.length) why.push(`it waits on ${pending.map((d) => d.title).join(', ')}`)
    if (r.proposal) why.push('a change to it waits for a yes')
    if (r.kind === 'milestone' && r.due_at_ms != null && r.due_at_ms < now) why.push('its date has passed')
    if (why.length) waiting.push({ id: r.id, title: r.title, why: why.join('; ') })
  }
  if (project.state !== 'done' && records.find((r) => r.id === project.id)?.proposal) waiting.unshift({ id: project.id, title: project.title, why: 'a change to the project’s goal waits for a yes' })
  const p = project.progress
  const useMilestones = p.milestones_total > 0
  return {
    id: project.id, title: project.title, objective: project.objective, acceptance: project.acceptance,
    done: useMilestones ? p.milestones_done : p.steps_done, total: useMilestones ? p.milestones_total : p.steps_total,
    plan, next, doing, waiting,
  }
}

/** Why an item's line is refused before a call is made, or null when it can be added. */
export function itemRefusal(title: string): string | null {
  const t = title.trim()
  if (!t) return 'Write the step first.'
  if (t.length > ITEM_MAX) return `A step is one line of at most ${ITEM_MAX} characters; this one is ${t.length}.`
  return null
}

/** The two changes, as the calls that make them. */
export const addItemCall = (project: string, title: string) => ({ method: 'project.item.add', params: { project, title: title.trim() } })
export const closeItemCall = (item: string) => ({ method: 'project.item.close', params: { item } })

/** A project's record in a work set's rows, or in the records: the one a row stands for. */
export const projectIdOf = (r: { record?: Pick<TaskRecord, 'id' | 'kind'>; recordId?: string }): string | null => (r.record?.kind === 'project' ? r.record.id : null)
