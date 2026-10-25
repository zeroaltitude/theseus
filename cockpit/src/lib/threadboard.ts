// Threads on the work board (theseus-5i8i). The same grouping `theseus_protocol::group` makes in Rust, for the cockpit:
// the daemon stamps each work view with the thread its tree belongs to (`WorkView.thread`) and sends the thread groups
// beside the views (`work.watch`, then `work.changed`); this file keeps both by the position rule and groups them.
// Pure: `npm test` runs it (test/threadboard.test.ts), and it imports only the protocol's types.
//
// - A thread's needs-you is its open items at the needs-you level, its conversation's own wait when none of its open
//   questions is in the list, and a seal recommended and not yet answered (one).
// - A paused thread shows its items and its count, and leaves the board's count.
// - A done thread shows only while something under it is open.
// - Warm is a last activity within five minutes, the provider cache's lifetime.
import type { TaskRecord, ThreadGroup, WorkChanged, WorkView, WorkWatchResult } from '@protocol'
import { openView, OWNER_CALLS, type OwnerCalls, type StateBucket } from '../ops/state.ts'
import type { OpsRow } from '../ops/board.ts'
import { span } from './figures.ts'

export const WARM_MS = 5 * 60 * 1000

/** What the page holds of the board. */
export interface BoardState { views: Record<string, WorkView>; groups: Record<string, ThreadGroup>; position: number }

export const EMPTY: BoardState = { views: {}, groups: {}, position: 0 }

function mergeGroup(held: ThreadGroup | undefined, g: ThreadGroup): ThreadGroup {
  if (!held) return g
  return {
    ...g,
    context_tokens: g.context_tokens ?? held.context_tokens,
    today_usd: g.today_usd ?? held.today_usd,
    last_active_ms: Math.max(g.last_active_ms, held.last_active_ms),
  }
}

function applyViews(views: Record<string, WorkView>, incoming: WorkView[]): Record<string, WorkView> {
  let next = views
  for (const v of incoming) {
    const old = next[v.id]
    if (old && v.position <= old.position) continue
    if (next === views) next = { ...views }
    next[v.id] = v
  }
  return next
}

/** `work.watch`'s snapshot: the truth for everything it names; a view it lacks that is not newer than it is gone. */
export function applySnapshot(s: BoardState, snap: WorkWatchResult): BoardState {
  const named = new Set(snap.views.map((v) => v.id))
  const kept: Record<string, WorkView> = {}
  for (const [id, v] of Object.entries(s.views)) if (named.has(id) || v.position > snap.position) kept[id] = v
  const groups: Record<string, ThreadGroup> = {}
  for (const g of snap.threads ?? []) groups[g.slug] = mergeGroup(s.groups[g.slug], g)
  for (const [slug, g] of Object.entries(s.groups)) if (!groups[slug]) groups[slug] = g
  return { views: applyViews(kept, snap.views), groups, position: Math.max(s.position, snap.position) }
}

/** One `work.changed`: views by the position rule, the ids that left, and the groups it changed. */
export function applyChanged(s: BoardState, c: WorkChanged): BoardState {
  let views = applyViews(s.views, c.views)
  if (c.gone?.some((id) => id in views)) {
    views = { ...views }
    for (const id of c.gone) delete views[id]
  }
  let groups = s.groups
  for (const g of c.threads ?? []) {
    if (groups === s.groups) groups = { ...s.groups }
    groups[g.slug] = mergeGroup(groups[g.slug], g)
  }
  return { views, groups, position: Math.max(s.position, c.position) }
}

/** An open task, question or wake (`ops/state.ts`: waiting counts as open), or one that failed and has not been looked
 *  at. A suspended record keeps its task's work open though its execution ended. */
function listed(v: WorkView, records: ReadonlyMap<string, TaskRecord>): boolean {
  return ['task', 'question', 'wake'].includes(v.kind) && openView(v, v.session_id ? records.get(v.session_id) : undefined)
}

/** How many of `views` wait on the owner; a conversation or task that asks counts through its question view when the
 *  list holds it. */
export function needsCount(views: WorkView[]): number {
  return views.filter((v) => {
    if (v.attention.level !== 'needs_you') return false
    const asking = v.question != null && (v.kind === 'conversation' || v.kind === 'task')
    return !(asking && views.some((q) => q.kind === 'question' && q.parent === v.id))
  }).length
}

export interface GroupedThread {
  group: ThreadGroup
  head: WorkView | null
  items: WorkView[]
  needsYou: number
  working: number
  lastActiveMs: number
}

export interface Grouped { threads: GroupedThread[]; loose: WorkView[] }

/** The board by thread: needs-you threads first, then the most recently active; the work in no thread stays loose. */
export function groupThreads(views: WorkView[], groups: ThreadGroup[], records: readonly TaskRecord[] = []): Grouped {
  const bySession = new Map(records.flatMap((r) => (r.session ? [[r.session, r] as const] : [])))
  const threads: GroupedThread[] = []
  for (const g of groups) {
    const mine = views.filter((v) => v.thread === g.slug)
    const head = mine.find((v) => v.kind === 'conversation' && v.parent == null) ?? null
    const rank = (v: WorkView) => ({ needs_you: 0, working: 2, ready: 3, idle: 4 })[v.attention.level]
    const items = mine.filter((v) => listed(v, bySession)).sort((a, b) => rank(a) - rank(b) || a.state_since_ms - b.state_since_ms || (a.id < b.id ? -1 : 1))
    if (g.state === 'done' && items.length === 0) continue
    const counted = head ? [...items, head] : items
    threads.push({
      group: g,
      head,
      items,
      needsYou: needsCount(counted) + (g.seal_pending != null ? 1 : 0),
      working: items.filter((v) => v.attention.level === 'working').length,
      lastActiveMs: Math.max(g.last_active_ms, head?.at_ms ?? 0),
    })
  }
  threads.sort((a, b) => Number(a.needsYou === 0) - Number(b.needsYou === 0) || b.lastActiveMs - a.lastActiveMs || (a.group.slug < b.group.slug ? -1 : 1))
  const known = new Set(groups.map((g) => g.slug))
  return { threads, loose: views.filter((v) => v.thread == null || !known.has(v.thread)) }
}

/** What a thread adds to the board's needs-you count: nothing while it is paused. */
export const boardNeeds = (t: GroupedThread): number => (t.group.state === 'paused' ? 0 : t.needsYou)

/** What waits on the owner on the whole board. */
export const totalNeeds = (g: Grouped): number => needsCount(g.loose) + g.threads.reduce((n, t) => n + boardNeeds(t), 0)

export const isWarm = (lastActiveMs: number, now: number): boolean => lastActiveMs > 0 && now - lastActiveMs <= WARM_MS

/** The thread's title, with a lock when it is sealed. */
export const titleOf = (g: ThreadGroup): string => (g.sealed ? `🔒 ${g.title}` : g.title)

/** Tokens as people say them: `900`, `41k`, `1.2M`. */
export function tokensWords(t: number): string {
  if (t < 1000) return String(t)
  if (t < 1_000_000) return `${Math.floor((t + 500) / 1000)}k`
  return `${(t / 1_000_000).toFixed(1)}M`
}

/** The details a header gives after the title, in order: state, last activity, warm or cold, context, today's dollars,
 *  persona. */
export function details(t: GroupedThread, now: number): string[] {
  const g = t.group
  const out: string[] = [g.state]
  out.push(t.lastActiveMs === 0 ? 'no messages yet' : `${span(Math.max(0, now - t.lastActiveMs))} ago`)
  if (t.lastActiveMs > 0) out.push(isWarm(t.lastActiveMs, now) ? 'warm' : 'cold')
  if (g.context_tokens != null) out.push(`${tokensWords(g.context_tokens)} ctx`)
  if (g.today_usd != null && g.today_usd > 0) out.push(`$${g.today_usd.toFixed(2)} today`)
  if (g.persona) out.push(g.persona)
  return out
}

/** The history links of a thread record: where it came from and where it went. */
export interface Links { parent?: string; merged_into?: string; split_from?: string; links?: string[] }
export function historyLinks(l: Links): Array<{ label: string; slug: string }> {
  const out: Array<{ label: string; slug: string }> = []
  if (l.parent) out.push({ label: 'Under', slug: l.parent })
  if (l.split_from) out.push({ label: 'Split from', slug: l.split_from })
  if (l.merged_into) out.push({ label: 'Merged into', slug: l.merged_into })
  for (const slug of l.links ?? []) out.push({ label: 'Linked with', slug })
  return out
}

/** A working set as lines of words (the record's, never invented). */
export function workingSetLines(ws: { roots?: string[]; repos?: string[]; branches?: string[]; refs?: string[]; files?: string[]; hosts?: string[] }): string[] {
  const parts: Array<[string, string[] | undefined]> = [
    ['Roots', ws.roots], ['Repos', ws.repos], ['Branches', ws.branches], ['Refs', ws.refs], ['Files', ws.files], ['Hosts', ws.hosts],
  ]
  return parts.filter(([, v]) => v && v.length > 0).map(([k, v]) => `${k}: ${v!.join(', ')}`)
}

/** A thread as the ledger's rows left it at a moment: its slug, title and state (the rows hold the words of an edit,
 *  never the card). The time machine reads this where the live page reads `thread.list`. */
export interface ThreadThen { slug: string; title: string; state: 'active' | 'paused' | 'done'; at: number }

/** Fold the `thread.*` rows at or before `t` (rows in the ledger's order) into the threads as they stood. A row of a
 *  kind this build does not know leaves the thread as it was. */
export function foldThreads(rows: Array<{ at_unix_ms: number; kind: string; data: unknown }>, t: number): ThreadThen[] {
  const then = new Map<string, ThreadThen>()
  for (const r of rows) {
    if (r.at_unix_ms > t || !r.kind.startsWith('thread.')) continue
    const d = (r.data ?? {}) as { slug?: unknown; title?: unknown; state?: unknown }
    if (typeof d.slug !== 'string') continue
    const state = d.state === 'active' || d.state === 'paused' || d.state === 'done' ? d.state : then.get(d.slug)?.state ?? 'active'
    then.set(d.slug, { slug: d.slug, title: typeof d.title === 'string' ? d.title : then.get(d.slug)?.title ?? d.slug, state, at: r.at_unix_ms })
  }
  return [...then.values()].sort((a, b) => b.at - a.at || (a.slug < b.slug ? -1 : 1))
}

// ---- The Threads lens over the one work set (theseus-brcs4) ----------------------------------------------------------
// `threadWork` reads the rows `boardRows` made (every card once, each with its bucket and whether it counts) and never
// derives a second set: a thread lists its plans and steps, the tasks that carry them on the same line, and under a
// counter's filter only the kept cards with their ancestors as context. The counters are `ops/board.ts`'s alone.

const RANK: Record<StateBucket, number> = { needs: 0, working: 1, scheduled: 2, paused: 3, todo: 4, rest: 5, ended: 6 }
const DONE_STATES = ['done', 'complete']
const OPEN_WORDS: ReadonlyArray<[StateBucket, string]> = [['needs', 'need you'], ['working', 'working'], ['scheduled', 'scheduled'], ['paused', 'paused'], ['todo', 'to do']]

/** A plan's progress over the leaf cards of its subtree (a step, or a task carrying a step; never a plan, a job, a wake
 *  or an ask), each counted once however deep the nesting. `todo` is its own word while `todoApart` holds (Q2: a to-do
 *  step is carried in its plan's n/m, on no counter); otherwise it reads as paused. Derived from rows, as the Graph's
 *  plan card derives its own n/m; the two should fold onto one helper if they ever differ. */
export interface PlanProgress { done: number; total: number; by: Partial<Record<StateBucket, number>>; words: string }

export interface WorkLine {
  row: OpsRow
  depth: number
  /** Drawn faintly: it is here only because a kept card sits under it (or beside it, a job). */
  context: boolean
  /** The attached task: the row itself when a task carries its record; absent on a step nobody has taken. */
  task?: OpsRow
  /** A step with no task of its own: who carries it, in words. */
  carriedBy?: string
  /** What the card asks of the owner: one a card (`needsPerCard`), else one an ask. */
  needs: number
  progress?: PlanProgress
}

export interface ThreadWork {
  /** The thread's group; null is Home, the work that belongs to no thread. */
  group: ThreadGroup | null
  lines: WorkLine[]
  /** The needs-you marks of the cards listed here that a counter counts. */
  needs: number
  /** The cards listed here that a filter keeps (every line, when none filters). */
  kept: number
}

export interface WorkOpts {
  /** The ids a counter's filter keeps (`shown()`), and the context it adds (`contextOf`); absent when nothing filters. */
  kept?: ReadonlySet<string> | null
  context?: ReadonlySet<string> | null
  calls?: OwnerCalls
}

export function threadWork(rows: readonly OpsRow[], groups: readonly ThreadGroup[], opts: WorkOpts = {}): ThreadWork[] {
  const calls = opts.calls ?? OWNER_CALLS
  const kept = opts.kept ?? null
  const ctx = opts.context ?? new Set<string>()
  const named = new Map<string, OpsRow>()
  for (const r of rows) for (const k of [r.id, r.view?.id, r.recordId, r.sessionId && r.kind !== 'job' ? r.sessionId : undefined, r.task?.info?.execution_id]) if (k && !named.has(k)) named.set(k, r)
  // A plan's step hangs from its record's parent; any other card from the row's own (a task's view parent, a job's host).
  const parentOf = (r: OpsRow): OpsRow | undefined => {
    const p = (r.record?.parent ? named.get(r.record.parent) : undefined) ?? (r.parent ? named.get(r.parent) : undefined)
    return p && p !== r ? p : undefined
  }
  const slugOf = new Map<string, string | null>()
  const known = new Set(groups.map((g) => g.slug))
  const threadOf = (r: OpsRow, depth = 0): string | null => {
    if (slugOf.has(r.id)) return slugOf.get(r.id)!
    let s: string | null = r.thread != null && known.has(r.thread) ? r.thread : null
    if (s == null && r.thread == null && depth < 64) { const p = parentOf(r); if (p) s = threadOf(p, depth + 1) }
    slugOf.set(r.id, s)
    return s
  }
  const kids = new Map<string, OpsRow[]>()
  const roots = new Map<string | null, OpsRow[]>()
  // A `parent` cycle has no root, so neither member would be emitted and both would vanish from the lens, where the
  // Graph still draws them: a row whose parent stands under itself is a root of its thread instead (theseus-brcs4 F4).
  const ancestral = (p: OpsRow, r: OpsRow): boolean => { for (let up: OpsRow | undefined = p, i = 0; up && i < 64; i++, up = parentOf(up)) if (up === r) return true; return false }
  for (const r of rows) {
    const p = parentOf(r)
    const t = threadOf(r)
    const into = p && !ancestral(p, r) && threadOf(p) === t ? (kids.get(p.id) ?? kids.set(p.id, []).get(p.id)!) : (roots.get(t) ?? roots.set(t, []).get(t)!)
    into.push(r)
  }
  const order = (a: OpsRow, b: OpsRow) => RANK[a.bucket] - RANK[b.bucket] || a.activityMs - b.activityMs || (a.id < b.id ? -1 : 1)
  for (const l of kids.values()) l.sort(order)
  for (const l of roots.values()) l.sort(order)

  const leaves = (r: OpsRow, seen: Set<string>, depth = 0): void => {
    if (depth > 64) return
    for (const k of kids.get(r.id) ?? []) {
      if (seen.has(k.id)) continue
      seen.add(k.id)
      leaves(k, seen, depth + 1)
    }
  }
  const progressOf = (r: OpsRow): PlanProgress | undefined => {
    const below = new Set<string>()
    leaves(r, below)
    const cards = [...below].map((id) => named.get(id)!).filter((c) => c.recordId != null && !c.plan)
    if (cards.length === 0) return undefined
    const by: Partial<Record<StateBucket, number>> = {}
    let done = 0
    let over = 0
    for (const c of cards) {
      const b: StateBucket = c.bucket === 'todo' && !calls.todoApart ? 'paused' : c.bucket
      by[b] = (by[b] ?? 0) + 1
      if (b === 'ended') { if (DONE_STATES.includes(c.state ?? '')) done += 1; else over += 1 }
    }
    const parts = OPEN_WORDS.filter(([b]) => (by[b] ?? 0) > 0).map(([b, w]) => `${by[b]} ${w}`)
    if (done > 0) parts.push(`${done} done`)
    if (over > 0) parts.push(`${over} ended`)
    return { done, total: cards.length, by, words: parts.join(' · ') }
  }

  const carriedBy = (r: OpsRow): string => {
    const c = r.carrier ? named.get(r.carrier) : undefined
    return c ? `carried by ${c.title}` : 'nobody carries it'
  }
  const emit = (r: OpsRow, depth: number, parentKept: boolean): WorkLine[] => {
    const isKept = kept == null || kept.has(r.id)
    const below = (kids.get(r.id) ?? []).flatMap((k) => emit(k, depth + 1, isKept))
    if (!(isKept || below.length > 0 || (ctx.has(r.id) && parentKept))) return []
    const line: WorkLine = {
      row: r, depth, context: !isKept, needs: r.bucket === 'needs' ? (calls.needsPerCard ? 1 : Math.max(r.asks, 1)) : 0,
      ...(r.kind === 'task' && r.recordId ? { task: r } : {}),
      ...(r.kind === 'step' && !r.plan ? { carriedBy: carriedBy(r) } : {}),
      ...(r.plan ? { progress: progressOf(r) } : {}),
    }
    return [line, ...below]
  }
  const make = (group: ThreadGroup | null): ThreadWork => {
    const lines = (roots.get(group?.slug ?? null) ?? []).flatMap((r) => emit(r, 0, false))
    const mine = lines.filter((l) => !l.context)
    return { group, lines, needs: mine.filter((l) => l.row.counted).reduce((n, l) => n + l.needs, 0), kept: mine.length }
  }
  const out = [...groups.map(make), make(null)]
  // A filter lists a thread only when it holds a kept card; no filter lists every thread, and Home when it has work.
  return out.filter((t) => (kept != null ? t.kept > 0 : t.group != null || t.lines.length > 0))
}
