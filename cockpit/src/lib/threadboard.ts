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
import { openView } from '../ops/state.ts'

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

/** A span in its largest unit: `40s`, `3m`, `2h`, `5d`. */
export function span(ms: number): string {
  const s = Math.floor(ms / 1000)
  if (s < 60) return `${s}s`
  if (s < 3600) return `${Math.floor(s / 60)}m`
  if (s < 86_400) return `${Math.floor(s / 3600)}h`
  return `${Math.floor(s / 86_400)}d`
}

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
