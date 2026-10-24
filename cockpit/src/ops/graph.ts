// The Operations page's Graph lens, laid out (theseus-jid7t.4): every session, task, step and job as one graph in
// lanes, one lane per live thread and a home lane for the rest. Pure: `npm test` runs it (test/ops-graph.test.ts), and
// it imports only the protocol's types and other pure modules; the lens that draws it is `ops/GraphLens.tsx`.
//
// The rules, each held by a test:
// - Within a lane, columns by depth (a conversation is its lane's anchor, at the left) and rows by start time.
// - **Positions are sticky by id.** Given the last layout (`prev`), every node it placed keeps its slot: a new node
//   takes the next free row of its column, a node that left leaves a hole, and a lane's band keeps a spare row, so one
//   new node moves nothing else. The lanes keep their order; a new lane goes below. `prev = null` lays it out afresh,
//   needs-you lanes first (the lens's "tidy").
// - Ended work older than an hour folds into one "✓ N done" node per lane, which opens on a click (`expanded`).
// - Past `cap` nodes (400), subtrees fold, the deepest and oldest first, each into one "+N folded" node; a subtree that
//   holds something that needs the owner never folds.
// - Edges: a view's parent (solid; it flows while the child runs), a record's deps (dashed, with an arrow: "waits
//   on"), and `retry_of` (dotted).
import type { DataMark, TaskInfo, TaskRecord, WorkView } from '@protocol'
import type { OpsRow } from './board.ts'
import { bucketOfWork } from './state.ts'

export const COL_W = 288
export const ROW_H = 140
export const GUTTER = 212
export const LANE_HEAD = 18
export const LANE_GAP = 22
export const NODE_W = 248
export const NODE_CAP = 400
export const DONE_FOLD_MS = 3_600_000
export const HOME = 'home'

export type GKind = 'conversation' | 'task' | 'step' | 'job' | 'wake' | 'routine' | 'watch' | 'project' | 'done' | 'folded'

export interface GNode {
  /** What the address and the selection name it by: a conversation's or task's session (`ses_`), else its own id. */
  id: string
  kind: GKind
  lane: string
  col: number
  row: number
  x: number
  y: number
  title: string
  state: string
  view?: WorkView
  record?: TaskRecord
  /** The node it hangs from (its parent, by node id). */
  parent?: string
  /** It needs the owner: it glows amber, with its keys. */
  needs: boolean
  /** Questions it asks, as a badge on it (a question is never a node). */
  asks: number
  data?: DataMark
  startedMs: number
  /** A fold's nodes: "✓ N done" or "+N folded". */
  members?: string[]
  /** New since the last layout: it fades in. */
  fresh: boolean
}

export type EdgeKind = 'parent' | 'deps' | 'retry'

export interface GEdge { id: string; source: string; target: string; kind: EdgeKind; running: boolean }

/** A lane's words, from the page (the Threads lens's `threadFacts`): the layout reads `key` and `needs` alone. */
export interface LaneMeta { key: string; title: string; needs: number; words?: string; sealed?: boolean; data?: DataMark }

export interface GLane extends LaneMeta { y: number; height: number; rows: number }

export interface Graph {
  nodes: GNode[]
  edges: GEdge[]
  lanes: GLane[]
  width: number
  /** Every node's slot, by id: the next layout's `prev`. */
  slots: Readonly<Record<string, { lane: string; col: number; row: number }>>
  bands: Readonly<Record<string, number>>
  order: readonly string[]
  /** How many nodes the cap folded away. */
  folded: number
  /** An opening was asked for and the cap could not hold it: the layout folded it back. */
  cantOpen?: boolean
}

export interface LayoutOpts {
  now: number
  /** "+ future": wakes, routines, watches and projects. */
  future?: boolean
  /** The lanes whose "✓ N done" node is open. */
  expanded?: ReadonlySet<string>
  /** Fold roots the owner opened (`+N folded`): their subtrees are never chosen to fold, unless the cap cannot hold. */
  opened?: ReadonlySet<string>
  cap?: number
}

const ENDED_VIEW = new Set(['done', 'failed', 'cancelled', 'expired'])
const ENDED_RECORD = new Set(['done', 'failed', 'abandoned'])
const FUTURE: ReadonlySet<string> = new Set(['wake', 'routine', 'watch', 'project'])

interface Item {
  id: string
  kind: GKind
  title: string
  state: string
  parentRef?: string
  thread?: string
  startedMs: number
  endedMs?: number
  ended: boolean
  needs: boolean
  data?: DataMark
  view?: WorkView
  record?: TaskRecord
  retryOf?: string
}

/** A view's node id: a conversation or a task by its session, as the board's rows; anything else by its own id. */
export const nodeIdOf = (v: Pick<WorkView, 'id' | 'kind' | 'session_id'>): string =>
  (v.kind === 'conversation' || v.kind === 'task') && v.session_id ? v.session_id : v.id

export function opsLayout(views: readonly WorkView[], records: readonly TaskRecord[], lanes: readonly LaneMeta[], prev: Graph | null, opts: LayoutOpts): Graph {
  return layout(views, records, lanes, prev, opts)
}

function layout(views: readonly WorkView[], records: readonly TaskRecord[], lanes: readonly LaneMeta[], prev: Graph | null, opts: LayoutOpts): Graph {
  // A layout that placed nothing (the page before its first read) holds no order to keep.
  if (prev && prev.nodes.length === 0) prev = null
  const { now, future = false, expanded = new Set<string>(), opened = new Set<string>(), cap = NODE_CAP } = opts
  const items = new Map<string, Item>()
  const alias = new Map<string, string>()
  const asks = new Map<string, number>()
  const bySession = new Map<string, string>()

  for (const v of views) {
    if (v.kind === 'question') { if (v.parent) asks.set(v.parent, (asks.get(v.parent) ?? 0) + 1); continue }
    if (v.kind === 'system' || (FUTURE.has(v.kind) && !future)) continue
    const id = nodeIdOf(v)
    alias.set(v.id, id)
    if (v.session_id && (v.kind === 'conversation' || v.kind === 'task')) bySession.set(v.session_id, id)
    items.set(id, {
      id, kind: v.kind as GKind, title: v.title || v.attention.label || id, state: v.paused ? 'paused' : v.state, parentRef: v.parent, thread: v.thread,
      startedMs: v.started_at_ms || v.state_since_ms, endedMs: v.ended_at_ms ?? (ENDED_VIEW.has(v.state) ? v.state_since_ms : undefined),
      ended: ENDED_VIEW.has(v.state), needs: v.attention.level === 'needs_you' && !v.dismissed, data: v.data, view: v, retryOf: v.retry_of,
    })
  }
  for (const t of records) {
    // A record whose task runs in its own session is that task's node; a step is a node of its own.
    const same = (t.session && bySession.get(t.session)) || (items.has(t.id) ? t.id : undefined)
    if (same) {
      alias.set(t.id, same)
      const it = items.get(same)!
      it.record ??= t
      // A suspended record whose execution ended is open and recoverable: drawn paused, never folded as done.
      if (it.view && it.ended && t.state === 'suspended' && bucketOfWork(it.view, now, t) !== 'ended') { it.ended = false; it.state = 'suspended' }
      continue
    }
    const project = t.kind === 'project' || t.kind === 'milestone'
    if (project && !future) continue
    items.set(t.id, {
      id: t.id, kind: project ? 'project' : 'step', title: t.title, state: t.state, parentRef: project ? undefined : t.parent ?? bySession.get(t.origin.session),
      startedMs: t.created_at_ms, endedMs: ENDED_RECORD.has(t.state) ? t.updated_at_ms : undefined, ended: ENDED_RECORD.has(t.state), needs: !!t.proposal, record: t,
    })
  }
  for (const [k, n] of asks) { const id = alias.get(k); if (id && items.has(id)) asks.set(id, n) }

  // The tree: each item's parent by node id, its lane, its depth.
  const parentOf = new Map<string, string>()
  for (const it of items.values()) {
    const p = it.parentRef ? alias.get(it.parentRef) ?? it.parentRef : undefined
    if (p && p !== it.id && items.has(p)) parentOf.set(it.id, p)
  }
  const live = new Set(lanes.map((l) => l.key))
  const laneOf = new Map<string, string>()
  const lane = (id: string, seen = 0): string => {
    const got = laneOf.get(id)
    if (got) return got
    const it = items.get(id)!
    const p = parentOf.get(id)
    const l = it.kind === 'project' ? HOME : it.thread && live.has(it.thread) ? it.thread : p && seen < 64 ? lane(p, seen + 1) : HOME
    laneOf.set(id, l)
    return l
  }
  const depthOf = new Map<string, number>()
  const depth = (id: string, seen = 0): number => {
    const got = depthOf.get(id)
    if (got !== undefined) return got
    const it = items.get(id)!
    const p = parentOf.get(id)
    const d = it.kind === 'conversation' ? 0 : p && seen < 64 && lane(p) === lane(id) ? depth(p, seen + 1) + 1 : 1
    depthOf.set(id, d)
    return d
  }
  for (const id of items.keys()) { lane(id); depth(id) }

  // What is drawn: every item, then the folds. A fold's node stands for its members, and their edges go to it.
  const shown = new Map<string, { id: string; kind: GKind; lane: string; col: number; parent?: string; members?: string[]; it?: Item }>()
  for (const it of items.values()) shown.set(it.id, { id: it.id, kind: it.kind, lane: laneOf.get(it.id)!, col: depthOf.get(it.id)!, parent: parentOf.get(it.id), it })
  const into = new Map<string, string>()

  // Ended work older than an hour, one "✓ N done" per lane.
  const old = new Map<string, string[]>()
  for (const it of items.values()) {
    if (!(it.kind === 'task' || it.kind === 'step' || it.kind === 'job') || !it.ended || it.needs) continue
    if ((it.endedMs ?? it.startedMs) > now - DONE_FOLD_MS) continue
    const l = laneOf.get(it.id)!
    if (expanded.has(l)) continue
    old.set(l, [...(old.get(l) ?? []), it.id])
  }
  for (const [l, ids] of old) {
    const id = `done:${l}`
    for (const m of ids) { shown.delete(m); into.set(m, id) }
    shown.set(id, { id, kind: 'done', lane: l, col: 1, members: ids })
  }

  // The cap: subtrees fold, the deepest and oldest first, never one that holds something that needs the owner.
  let folded = 0
  let cantOpen = false
  if (shown.size > cap) {
    const kids = new Map<string, string[]>()
    for (const n of shown.values()) if (n.parent && shown.has(n.parent)) kids.set(n.parent, [...(kids.get(n.parent) ?? []), n.id])
    const below = (id: string): string[] => { const out: string[] = []; const walk = (x: string) => { for (const k of kids.get(x) ?? []) if (shown.has(k)) { out.push(k); walk(k) } }; walk(id); return out }
    const roots = [...kids.keys()].filter((id) => shown.get(id)?.it)
      .sort((a, b) => depthOf.get(b)! - depthOf.get(a)! || items.get(a)!.startedMs - items.get(b)!.startedMs || (a < b ? -1 : 1))
    // An opened root, and everything below it, stays as it is.
    const keep = new Set<string>()
    for (const o of opened) if (shown.has(o)) { keep.add(o); for (const m of below(o)) keep.add(m) }
    const pass = (hold: ReadonlySet<string>) => {
      for (const r of roots) {
        if (shown.size <= cap) break
        if (!shown.has(r) || hold.has(r)) continue
        const sub = below(r)
        if (sub.length < 2 || sub.some((m) => items.get(m)?.needs || hold.has(m))) continue
        const id = `fold:${r}`
        for (const m of sub) { shown.delete(m); into.set(m, id) }
        shown.set(id, { id, kind: 'folded', lane: laneOf.get(r)!, col: depthOf.get(r)! + 1, parent: r, members: sub })
        folded += sub.length
      }
    }
    const before = { shown: new Map(shown), into: new Map(into), folded }
    pass(keep)
    // The cap is never raised for an opening: if it is still over with the opening held, lay the folds out again
    // without it. The opening is reported as not opened when the pass without it reaches the cap, or when that pass
    // ends with the opening folded (the graph is over its cap for another reason too, and the opening could not stay).
    if (keep.size && shown.size > cap) {
      shown.clear(); for (const [k, v] of before.shown) shown.set(k, v)
      into.clear(); for (const [k, v] of before.into) into.set(k, v)
      folded = before.folded
      pass(new Set())
      cantOpen = shown.size <= cap || [...keep].some((k) => !shown.has(k))
    }
  }
  const at = (id: string): string | undefined => { let x: string | undefined = alias.get(id) ?? id; for (let i = 0; x && !shown.has(x) && i < 8; i++) x = into.get(x); return x && shown.has(x) ? x : undefined }

  // Slots: the last layout's for every node it placed in the same lane; the next free row of its column for the rest,
  // in start order, so a fresh layout is rows by start time.
  const slots: Record<string, { lane: string; col: number; row: number }> = {}
  const used = new Map<string, number>()
  const cell = (l: string, c: number) => `${l}\u0000${c}`
  const take = (id: string, l: string, c: number, r: number) => { slots[id] = { lane: l, col: c, row: r }; used.set(cell(l, c), Math.max(used.get(cell(l, c)) ?? -1, r)) }
  const placed = prev?.slots ?? {}
  const fresh: Array<{ id: string; lane: string; col: number; t: number }> = []
  for (const n of shown.values()) {
    const p = placed[n.id]
    if (p && p.lane === n.lane) take(n.id, p.lane, p.col, p.row)
    else fresh.push({ id: n.id, lane: n.lane, col: n.col, t: n.it?.startedMs ?? Number.MAX_SAFE_INTEGER })
  }
  // A hole a node left is kept, so the rows after it stay where they were.
  for (const [id, p] of Object.entries(placed)) if (!slots[id] && shown.has(id) === false) used.set(cell(p.lane, p.col), Math.max(used.get(cell(p.lane, p.col)) ?? -1, p.row))
  fresh.sort((a, b) => a.t - b.t || (a.id < b.id ? -1 : 1))
  for (const f of fresh) take(f.id, f.lane, f.col, (used.get(cell(f.lane, f.col)) ?? -1) + 1)

  // The lanes: the last layout's order, then the new ones, needs-you first and home last. A band keeps its rows and
  // a spare one; it grows only when its rows are used up.
  const metas = new Map<string, LaneMeta>([...lanes.map((l) => [l.key, l] as const), [HOME, lanes.find((l) => l.key === HOME) ?? { key: HOME, title: 'Home · no thread', needs: 0 }]])
  const needs = new Map<string, number>()
  for (const it of items.values()) if (it.needs) needs.set(laneOf.get(it.id)!, (needs.get(laneOf.get(it.id)!) ?? 0) + 1)
  for (const [k, m] of metas) metas.set(k, { ...m, needs: Math.max(m.needs, needs.get(k) ?? 0) })
  const ranked = [...metas.values()].filter((l) => l.key !== HOME).sort((a, b) => Number(b.needs > 0) - Number(a.needs > 0)).map((l) => l.key)
  const order = [...(prev?.order ?? []).filter((k) => metas.has(k)), ...[...ranked, HOME].filter((k) => !(prev?.order ?? []).includes(k))]
  const need = new Map<string, number>()
  for (const s of Object.values(slots)) need.set(s.lane, Math.max(need.get(s.lane) ?? 0, s.row + 1))
  for (const [k, r] of used) { const l = k.split('\u0000')[0]; need.set(l, Math.max(need.get(l) ?? 0, r + 1)) }
  const bands: Record<string, number> = {}
  const out: GLane[] = []
  let y = 0
  for (const k of order) {
    const n = need.get(k) ?? 0
    const was = prev?.bands[k]
    const rows = was !== undefined && n <= was ? was : n + 1
    bands[k] = rows
    const height = LANE_HEAD + rows * ROW_H
    out.push({ ...metas.get(k)!, y, height, rows })
    y += height + LANE_GAP
  }
  const top = new Map(out.map((l) => [l.key, l.y]))

  const nodes: GNode[] = []
  let cols = 1
  for (const n of shown.values()) {
    const s = slots[n.id]
    cols = Math.max(cols, s.col + 1)
    const it = n.it
    nodes.push({
      id: n.id, kind: n.kind, lane: s.lane, col: s.col, row: s.row, x: GUTTER + s.col * COL_W, y: top.get(s.lane)! + LANE_HEAD + s.row * ROW_H,
      title: it?.title ?? (n.kind === 'done' ? `✓ ${n.members!.length} done` : `+${n.members!.length} folded`), state: it?.state ?? (n.kind === 'done' ? 'done' : 'folded'),
      view: it?.view, record: it?.record, parent: n.parent && shown.has(n.parent) ? n.parent : undefined, needs: it?.needs ?? false, asks: asks.get(n.id) ?? 0,
      data: it?.data, startedMs: it?.startedMs ?? 0, members: n.members, fresh: prev != null && !placed[n.id],
    })
  }
  nodes.sort((a, b) => (a.id < b.id ? -1 : 1))

  // The edges, each once, between the nodes drawn: a folded end goes to its fold.
  const edges = new Map<string, GEdge>()
  const edge = (from: string | undefined, to: string | undefined, kind: EdgeKind, running = false) => {
    const s = from && at(from)
    const t = to && at(to)
    if (!s || !t || s === t) return
    const id = `${kind}:${s}>${t}`
    const had = edges.get(id)
    edges.set(id, { id, source: s, target: t, kind, running: running || (had?.running ?? false) })
  }
  for (const it of items.values()) {
    const p = parentOf.get(it.id)
    if (p) edge(p, it.id, 'parent', it.state === 'running')
    for (const d of it.record?.deps ?? []) edge(it.id, d, 'deps')
    if (it.retryOf) edge(it.id, it.retryOf, 'retry')
  }
  return { nodes, edges: [...edges.values()].sort((a, b) => (a.id < b.id ? -1 : 1)), lanes: out, width: GUTTER + cols * COL_W, slots, bands, order, folded, ...(cantOpen ? { cantOpen } : {}) }
}

/** A task's kernel state as a work state, for the time machine's graph, which has the tasks but no views. */
const STATE_OF: Readonly<Record<string, WorkView['state']>> = {
  complete: 'done', running: 'running', waiting: 'waiting', queued: 'queued', failed: 'failed', budget_exhausted: 'failed', cancelled: 'cancelled',
}

/** The views the graph lays out from the board's rows: each row's own view, or, under the time machine (no views), one
 *  made from its task or session as it stood. */
export function viewsOf(rows: readonly OpsRow[]): WorkView[] {
  return rows.map((r) => r.view ?? pastView(r)).filter((v): v is WorkView => v != null)
}

function pastView(r: OpsRow): WorkView | undefined {
  const info: TaskInfo | undefined = r.task?.info
  const at = r.activityMs
  const base = { position: 0, at_ms: at, title: r.title, started_at_ms: at, state_since_ms: at, place: r.place }
  if (r.kind === 'task' && info) {
    const state = STATE_OF[info.state] ?? 'unknown'
    return { ...base, id: info.execution_id, kind: 'task', parent: info.parent_execution_id, root: info.parent_execution_id, session_id: info.task_id, state, paused: r.paused, started_at_ms: info.created_at_ms,
      attention: info.attention ?? { level: 'idle', label: state, since_ms: at }, cost: { spent_usd: info.spent_usd, limit_usd: info.limit_usd }, ended_at_ms: r.endedMs } as WorkView
  }
  if (r.kind === 'conversation' && r.session) {
    const s = r.session
    return { ...base, id: s.execution_id ?? s.session_id, kind: 'conversation', root: s.execution_id ?? s.session_id, session_id: s.session_id, state: (r.state ?? 'ready') as WorkView['state'],
      attention: s.attention ?? { level: 'idle', label: r.state ?? 'idle', since_ms: at }, data: s.data } as WorkView
  }
  return undefined
}

/** The lens's memory of its last layout: each call lays out over the last one (sticky by id), and a new `epoch` (the
 *  lens's "tidy") forgets it, so that layout is fresh. */
export function keeper(): { lay: (views: readonly WorkView[], records: readonly TaskRecord[], lanes: readonly LaneMeta[], opts: LayoutOpts, epoch: number) => Graph } {
  let prev: Graph | null = null
  let at = 0
  return {
    lay: (views, records, lanes, opts, epoch) => {
      if (epoch !== at) { prev = null; at = epoch }
      prev = opsLayout(views, records, lanes, prev, opts)
      return prev
    },
  }
}

const locked = (d: DataMark | undefined) => d === 'private' || d === 'sensitive'

/** The title a node or a lane is drawn with. Under redact (`?redact=1`) one whose data mark is private or sensitive
 *  shows its kind and short id (a lane: "a <mark> thread") instead. Every place a title is drawn takes this. */
export function shownTitle(n: { title: string; data?: DataMark; kind?: string; id?: string }, redact: boolean): string {
  if (!redact || !locked(n.data)) return n.title
  return n.kind ? `${n.kind} ${(n.id ?? '').slice(-6)}` : `a ${n.data} thread`
}

/** Whether redact hides this node's or lane's words. */
export const masked = (n: { data?: DataMark }, redact: boolean): boolean => redact && locked(n.data)

const WALKABLE = new Set<GKind>(['conversation', 'task', 'step', 'job'])

/** The node a key moves to: `j`/`k` the nearest node below/above in the same column (across lanes), `h`/`l` the nearest
 *  in the next column left/right (by distance down the page). A fold, a "✓ N done" and a future node are skipped. From
 *  nothing it takes the first node that needs the owner, else the top-left one. Null when there is nowhere to go. */
export function stepNode(g: Pick<Graph, 'nodes'>, fromId: string | null, dx: number, dy: number): string | null {
  const nodes = g.nodes.filter((n) => WALKABLE.has(n.kind))
  const byPos = (a: GNode, b: GNode) => a.y - b.y || a.x - b.x || (a.id < b.id ? -1 : 1)
  const from = fromId ? nodes.find((n) => n.id === fromId || n.record?.id === fromId) : undefined
  if (!from) return [...nodes].sort(byPos).find((n) => n.needs)?.id ?? [...nodes].sort(byPos)[0]?.id ?? null
  if (dy) {
    const col = nodes.filter((n) => n.x === from.x && (dy > 0 ? n.y > from.y : n.y < from.y)).sort(byPos)
    return (dy > 0 ? col[0] : col.at(-1))?.id ?? null
  }
  const side = nodes.filter((n) => (dx > 0 ? n.x > from.x : n.x < from.x))
  if (!side.length) return null
  const x = dx > 0 ? Math.min(...side.map((n) => n.x)) : Math.max(...side.map((n) => n.x))
  return side.filter((n) => n.x === x).sort((a, b) => Math.abs(a.y - from.y) - Math.abs(b.y - from.y) || byPos(a, b))[0].id
}
