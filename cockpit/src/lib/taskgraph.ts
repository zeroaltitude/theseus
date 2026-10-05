// The task graph (M7 39b): the records as a tree and as a graph's layout, a claim and a waiting change in words. Pure,
// so `npm test` runs it; the panel and the graph are `src/components/TaskGraph.tsx`.
import type { ConfirmRequest, TaskChange, TaskRecord } from '@protocol'

/** A task as people name it: the last six characters of its id. */
export function taskShort(id: string): string {
  return [...id].slice(-6).join('')
}

/** One row of the tree: the task, how deep it sits, and its parent's id when the parent is in the list. */
export interface TreeRow { t: TaskRecord; depth: number }

/** The records as a tree: roots oldest first, each child under its parent in the order it was made. A task whose
 *  parent is not in the list is a root. */
export function taskTree(records: TaskRecord[]): TreeRow[] {
  const byId = new Map(records.map((t) => [t.id, t]))
  const kids = new Map<string, TaskRecord[]>()
  for (const t of records) {
    if (t.parent && byId.has(t.parent)) {
      const k = kids.get(t.parent) ?? []
      k.push(t)
      kids.set(t.parent, k)
    }
  }
  const order = (a: TaskRecord, b: TaskRecord) => a.created_at_ms - b.created_at_ms || a.id.localeCompare(b.id)
  const out: TreeRow[] = []
  const seen = new Set<string>()
  const walk = (t: TaskRecord, depth: number) => {
    if (seen.has(t.id)) return
    seen.add(t.id)
    out.push({ t, depth })
    for (const c of (kids.get(t.id) ?? []).sort(order)) walk(c, depth + 1)
  }
  for (const r of records.filter((t) => !t.parent || !byId.has(t.parent)).sort(order)) walk(r, 0)
  return out
}

/** `claimed by session d4e5f6 · 29m left`, while the claim holds at `now`; null once it lapsed or when none. */
export function claimWords(t: TaskRecord, now: number): string | null {
  const c = t.claim
  if (!c || c.until_ms <= now) return null
  const left = Math.max(0, Math.floor((c.until_ms - now) / 60000))
  return `claimed by session ${taskShort(c.session)} · ${left < 1 ? 'under a minute' : `${left}m`} left`
}

const CLOSED = new Set(['done', 'failed', 'abandoned'])

/** Whether a task is closed: done, failed, or abandoned. */
export function isClosed(t: TaskRecord): boolean {
  return CLOSED.has(t.state)
}

/** The question a waiting change asks, as the layer-1 card and `theseus confirm` word it. */
export function changeWords(t: TaskRecord): string | null {
  const p = t.proposal
  if (!p) return null
  if (p.abandon) return `Abandon ${t.id} (${t.title})?`
  const what = p.objective != null && p.acceptance != null ? 'objective and acceptance' : p.objective != null ? 'objective' : 'acceptance'
  return `Change the ${what} of ${t.id} (${t.title})?`
}

/** A layer-1 question's change in the card's words, as `TaskChange::question` says it in theseus-protocol:
 *  `Change the acceptance of tsk_… (title)? Before: … After: …`. */
export function changeQuestion(c: TaskChange): string {
  const side = (s: string) => (s.trim() ? s : '(none)')
  const [before, after] = [side(c.before), side(c.after)]
  return c.field === 'abandon'
    ? `Abandon ${c.task} (${c.title})? Before: ${before} After: ${after}`
    : `Change the ${c.field} of ${c.task} (${c.title})? Before: ${before} After: ${after}`
}

/** The card a waiting change is asked by, among the questions that wait. */
export function cardOf(t: TaskRecord, confirms: ConfirmRequest[]): ConfirmRequest | undefined {
  const card = t.proposal?.card
  return card ? confirms.find((c) => c.correlation_id === card) : undefined
}

/** A graph's layout: each task a node at its depth's column, rows in tree order; an edge from each parent to its
 *  children, and (dashed) from each task to what it waits on. */
export interface Laid { id: string; x: number; y: number; t: TaskRecord }
export interface Link { id: string; source: string; target: string; dep: boolean }

export function layout(records: TaskRecord[], w = 260, h = 84): { nodes: Laid[]; links: Link[] } {
  const rows = taskTree(records)
  const ids = new Set(records.map((t) => t.id))
  const nodes = rows.map((r, i) => ({ id: r.t.id, x: r.depth * w, y: i * h, t: r.t }))
  const links: Link[] = []
  for (const { t } of rows) {
    if (t.parent && ids.has(t.parent)) links.push({ id: `${t.parent}>${t.id}`, source: t.parent, target: t.id, dep: false })
    for (const d of t.deps) if (ids.has(d)) links.push({ id: `${t.id}~${d}`, source: t.id, target: d, dep: true })
  }
  return { nodes, links }
}
