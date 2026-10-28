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
// - **A band is never shorter than its own gutter label** (`labelHeight`, theseus-6ln2q): a lane with no card drawn
//   still carries its wrapped title, its warmth words and its needs-you badge, so the band holds them and no two
//   lanes overlap. Before that, a lane the retire bar had emptied was `LANE_HEAD` tall and its label overprinted the
//   lanes below it.
// - Past `cap` nodes (400), subtrees fold, the deepest and oldest first, each into one "+N folded" node; a subtree that
//   holds something that needs the owner never folds.
// - Edges: a view's parent (solid; it flows while the child runs), a record's deps (dashed, with an arrow: "waits
//   on"), and `retry_of` (dotted).
import type { DataMark, TaskInfo, TaskRecord, WorkView } from '@protocol'
import { boardRows, type OpsRow } from './board.ts'
import { queueOf } from './queue.ts'
import { formsOf, type RowForm } from './forms.ts'
import { isEndedState, type StateBucket } from './state.ts'

export const COL_W = 288
export const ROW_H = 140
export const GUTTER = 212
export const LANE_HEAD = 18
export const LANE_GAP = 22
/** The gutter label's box in `GraphNode`'s `Lane`: `left-4 top-4 w-[180px]`, and the same inset again under it. */
export const LANE_LABEL_W = 180
export const LANE_LABEL_PAD = 16
/** One line of the display title (`text-[15px] leading-tight`), measured at 19 px in the browser. */
export const LANE_TITLE_LINE = 19
/** The warmth words under the title: `mt-1`, then a `text-[10.5px] leading-snug` line each. */
export const LANE_WORDS_TOP = 4
export const LANE_WORDS_LINE = 15
/** A badge row under them: `mt-1.5` and the pill. Both the needs-you count and the "fold N done" button are one. */
export const LANE_BADGE_H = 24
/** The lock a sealed lane's title carries (`Lock size={11}` and its `mr-1`). */
const LOCK_W = 15
/** The advance of every character the display face draws a title with — 15 px, `font-bold`, `tracking-[0.12em]`,
 *  and `uppercase`, so every letter is a cap. Measured in headless Chrome against the real font (theseus-6ln2q);
 *  the width of a title is not one number, since W is more than twice I. A character the table lacks is charged
 *  `TITLE_WIDEST`, so a glyph we have not measured can only make the band taller, never shorter. */
const TITLE_ADVANCE: Readonly<Record<string, number>> = {
  A: 12.633, B: 11.806, C: 12.633, D: 12.633, E: 11.806, F: 10.964, G: 13.469, H: 13.469, I: 7.638,
  J: 9.3, K: 13.469, L: 11.806, M: 15.958, N: 12.633, O: 13.469, P: 10.964, Q: 13.469, R: 12.633,
  S: 10.144, T: 11.806, U: 12.633, V: 12.633, W: 16.8, X: 12.633, Y: 12.633, Z: 11.806,
  0: 9.3, 1: 8.556, 2: 9.3, 3: 9.3, 4: 9.3, 5: 9.3, 6: 9.3, 7: 9.3, 8: 9.3, 9: 9.3,
  ' ': 3.896, '·': 5.55, ',': 5.55, '.': 5.55, '-': 6.795,
}
const TITLE_WIDEST = 16.8
/** The warmth words' face is JetBrains Mono at 10.5 px: one advance for every character. */
const WORDS_ADVANCE = 6.322
export const NODE_W = 248
export const NODE_CAP = 400
export const DONE_FOLD_MS = 3_600_000
export const HOME = 'home'

export type GKind = 'conversation' | 'task' | 'step' | 'job' | 'wake' | 'routine' | 'watch' | 'project' | 'ask' | 'done' | 'folded'

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
  /** Its row's bucket (`ops/state.ts`'s `bucketOfCard`): the one the counters count it by. Absent on a fold. */
  bucket?: StateBucket
  /** Its form (`ops/forms.ts`): a plan's progress, a step's glyph, the badge of the task it carries. Absent on a fold. */
  form?: RowForm
  /** A plan: its children are drawn (open), or folded into it, one node toward the cap. */
  open?: boolean
  /** A thread's own conversation, drawn as its lane's head card. */
  head?: boolean
}

export type EdgeKind = 'parent' | 'deps' | 'retry'

export interface GEdge { id: string; source: string; target: string; kind: EdgeKind; running: boolean }

/** A lane's words, from the page (the Threads lens's `threadFacts`): the layout reads `key`, `needs` and, for the
 *  band's floor, the words the gutter draws (`title`, `words`, `sealed`). */
export interface LaneMeta { key: string; title: string; needs: number; words?: string; sealed?: boolean; data?: DataMark; /** The thread's own conversation (its session), drawn as the lane's head card. */ head?: string }

export interface GLane extends LaneMeta { y: number; height: number; rows: number }

/** The lines a run of text wraps onto in a box this wide, the way the browser breaks it: at a space, never inside a
 *  word. `lead` is what already sits on the first line (a lock). A word wider than the whole box keeps its own line
 *  and runs past the edge — the gutter sets no `break-words`, so that is what the browser does too. */
function wrapLines(text: string, width: number, advance: (ch: string) => number, lead = 0): number {
  const words = text.trim().split(/\s+/).filter(Boolean)
  if (!words.length) return 1
  const span = (s: string) => [...s].reduce((w, ch) => w + advance(ch), 0)
  const gap = advance(' ')
  let lines = 1
  let used = lead
  for (const w of words) {
    const own = span(w)
    if (used && used + gap + own > width) { lines += 1; used = own } else used += (used ? gap : 0) + own
  }
  return lines
}

const titleAdvance = (ch: string): number => TITLE_ADVANCE[ch] ?? TITLE_WIDEST

/** The lines the gutter wraps a lane's title onto. The face uppercases, so the title is measured in caps; a seal
 *  leaves room for its lock on the first line. */
export const titleLines = (title: string, sealed = false): number =>
  wrapLines(title.toUpperCase(), LANE_LABEL_W, titleAdvance, sealed ? LOCK_W : 0)

/** The lines the warmth words wrap onto. They are not one line: `details` adds today's dollars and the persona
 *  (`lib/threadboard.ts`), and a warm thread that spent money today runs to two (theseus-6ln2q join fix). */
export const wordsLines = (words: string): number => wrapLines(words, LANE_LABEL_W, () => WORDS_ADVANCE)

/** How tall a lane's gutter label is, so the band can never be shorter than its own words (theseus-6ln2q): the
 *  inset above and below, the wrapped title, the wrapped warmth words, the needs-you badge, and — for a lane whose
 *  ended work is open — the "fold N done" button under them.
 *
 *  Each term is the browser's measure rounded up, never down, since a short band is the bug and a long one is a
 *  little space. Redact (`?redact=1`) drops the words but can *lengthen* a short title ("QX-LAB" becomes "a
 *  sensitive thread"), so a locked lane is measured both ways and keeps the taller: the band holds whichever the
 *  address asks for. */
export function labelHeight(l: LaneMeta, opened = false): number {
  const lines = Math.max(titleLines(l.title, l.sealed), titleLines(shownTitle(l, true), l.sealed))
  return LANE_LABEL_PAD * 2 + lines * LANE_TITLE_LINE
    + (l.words ? LANE_WORDS_TOP + wordsLines(l.words) * LANE_WORDS_LINE : 0)
    + (l.needs > 0 ? LANE_BADGE_H : 0) + (opened ? LANE_BADGE_H : 0)
}

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
  /** How many nodes `LayoutOpts.keep` left out of the drawing. */
  hidden: number
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
  /** The ids a counter's click keeps and the context around them: every other node is left out of the drawing, its slot
   *  kept as a hole (so nothing moves), and a lane with none of them shrinks to its head. Absent, nothing is hidden. */
  keep?: ReadonlySet<string>
  /** The owner's plan folds, by plan id (true open, false folded); a plan with none is open while it is open work and
   *  folded once it ends. The counter filter's own set (`filter`) opens a folded plan that holds a kept card. */
  folds?: ReadonlyMap<string, boolean>
  /** The counter filter's own kept set: it, and only it, opens a folded plan that holds a kept card. */
  filter?: ReadonlySet<string>
}

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
  bucket: StateBucket
  asks: number
}

/** A view's node id: a conversation or a task by its session, as the board's rows; anything else by its own id. */
export const nodeIdOf = (v: Pick<WorkView, 'id' | 'kind' | 'session_id'>): string =>
  (v.kind === 'conversation' || v.kind === 'task') && v.session_id ? v.session_id : v.id

/** The graph of views and records, laid out through the board's rows (`boardRows`): the form the tests of the layout
 *  take. The page lays out `layoutRows` of the one work set. */
export function opsLayout(views: readonly WorkView[], records: readonly TaskRecord[], lanes: readonly LaneMeta[], prev: Graph | null, opts: LayoutOpts): Graph {
  const queue = queueOf({ confirms: [], views }).filter((i) => i.kind === 'question')
  return layout(boardRows({ views, tasks: [], sessions: [], records, queue, now: opts.now, past: false }), lanes, prev, opts)
}

/** Every node is a row of the work set, and every row a node (a wake, a routine, a watch and a project only under
 *  "+ future"; an ask no card owns is a node of its own). */
export const layoutRows = (rows: readonly OpsRow[], lanes: readonly LaneMeta[], prev: Graph | null, opts: LayoutOpts): Graph => layout(rows, lanes, prev, opts)

function layout(rows: readonly OpsRow[], lanes: readonly LaneMeta[], prev: Graph | null, opts: LayoutOpts): Graph {
  // A layout that placed nothing (the page before its first read) holds no order to keep.
  if (prev && prev.nodes.length === 0) prev = null
  const { now, future = false, expanded = new Set<string>(), opened = new Set<string>(), cap = NODE_CAP } = opts
  const items = new Map<string, Item>()
  const alias = new Map<string, string>()
  const asks = new Map<string, number>()

  for (const r of rows) {
    const v = r.view ?? pastView(r)
    const rec = r.record
    for (const k of [r.id, v?.id, r.recordId, r.task?.info?.execution_id]) if (k) alias.set(k, r.id)
    if (r.kind === 'ask') {
      items.set(r.id, { id: r.id, kind: 'ask', title: r.title, state: 'needs_you', thread: r.thread, startedMs: r.activityMs, ended: false, needs: true, bucket: r.bucket, asks: 1 })
      continue
    }
    if (v) {
      if (v.kind === 'question' || v.kind === 'system' || (FUTURE.has(v.kind) && !future)) continue
      const it: Item = {
        id: r.id, kind: v.kind as GKind, title: v.title || v.attention.label || r.id, state: v.paused ? 'paused' : v.state, parentRef: r.parent, thread: v.thread,
        startedMs: v.started_at_ms || v.state_since_ms, endedMs: v.ended_at_ms ?? (isEndedState(v.state) ? v.state_since_ms : undefined),
        ended: isEndedState(v.state), needs: (v.attention.level === 'needs_you' && !v.dismissed) || r.bucket === 'needs', data: v.data, view: v, retryOf: v.retry_of, record: rec,
        bucket: r.bucket, asks: r.asks,
      }
      // A suspended record whose execution ended is open and recoverable: drawn paused, never folded as done.
      if (it.ended && rec?.state === 'suspended' && r.bucket !== 'ended') { it.ended = false; it.state = 'suspended' }
      items.set(r.id, it)
      continue
    }
    if (!rec) continue
    const project = rec.kind === 'project' || rec.kind === 'milestone'
    if (project && !future) continue
    items.set(r.id, {
      id: r.id, kind: project ? 'project' : 'step', title: rec.title, state: rec.state, parentRef: r.parent,
      startedMs: rec.created_at_ms, endedMs: ENDED_RECORD.has(rec.state) ? rec.updated_at_ms : undefined, ended: ENDED_RECORD.has(rec.state), needs: r.bucket === 'needs', record: rec,
      bucket: r.bucket, asks: r.asks,
    })
  }
  for (const it of items.values()) if (it.asks) asks.set(it.id, it.asks)

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

  // Plans: an open plan shows its children, a folded one stands for them (one node toward the cap). What needs the owner
  // is drawn anyway, and a filter opens a folded plan that holds a kept card.
  const forms = formsOf(rows, now)
  const kidsOf = new Map<string, string[]>()
  for (const [c, p] of parentOf) kidsOf.set(p, [...(kidsOf.get(p) ?? []), c])
  const under = (id: string): string[] => { const out: string[] = []; const seen = new Set([id]); const walk = (x: string) => { for (const k of kidsOf.get(x) ?? []) if (!seen.has(k)) { seen.add(k); out.push(k); walk(k) } }; walk(id); return out }
  const planOpen = new Map<string, boolean>()
  const plans = [...items.values()].filter((it) => forms.get(it.id)?.form === 'plan').sort((a, b) => depthOf.get(a.id)! - depthOf.get(b.id)! || (a.id < b.id ? -1 : 1))
  for (const it of plans) {
    const sub = under(it.id)
    let open = opts.folds?.get(it.id) ?? it.bucket !== 'ended'
    if (!open && opts.filter && sub.some((m) => opts.filter!.has(m))) open = true
    planOpen.set(it.id, open)
    if (open || !shown.has(it.id)) continue
    for (const m of sub) if (shown.has(m) && !items.get(m)!.needs) { shown.delete(m); into.set(m, it.id) }
  }

  // Ended work older than an hour, one "✓ N done" per lane.
  const old = new Map<string, string[]>()
  for (const it of items.values()) {
    if (!(it.kind === 'task' || it.kind === 'step' || it.kind === 'job') || !it.ended || it.needs) continue
    if (into.has(it.id)) continue // already standing in its folded plan
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

  // A node `keep` leaves out keeps its slot but is not drawn; a fold stays when any of its members does.
  const hidden = new Set<string>()
  if (opts.keep) for (const n of shown.values()) if (!opts.keep.has(n.id) && !n.members?.some((m) => opts.keep!.has(m))) hidden.add(n.id)
  const seenRow = new Map<string, number>()
  for (const [id, sl] of Object.entries(slots)) if (!hidden.has(id)) seenRow.set(sl.lane, Math.max(seenRow.get(sl.lane) ?? 0, sl.row + 1))

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
    const drawn = opts.keep ? seenRow.get(k) ?? 0 : rows
    // The band holds its rows, and never less than its own gutter label: with the retire bar on, a cold thread has
    // no card drawn at all, and a band of `LANE_HEAD` alone let its title and warmth words run over the lane below
    // (theseus-6ln2q).
    const meta = metas.get(k)!
    const height = Math.max(LANE_HEAD + drawn * ROW_H, labelHeight(meta, expanded.has(k)))
    out.push({ ...meta, y, height, rows: drawn })
    y += height + LANE_GAP
  }
  const top = new Map(out.map((l) => [l.key, l.y]))

  const nodes: GNode[] = []
  let cols = 1
  for (const n of shown.values()) {
    if (hidden.has(n.id)) continue
    const s = slots[n.id]
    cols = Math.max(cols, s.col + 1)
    const it = n.it
    nodes.push({
      id: n.id, kind: n.kind, lane: s.lane, col: s.col, row: s.row, x: GUTTER + s.col * COL_W, y: top.get(s.lane)! + LANE_HEAD + s.row * ROW_H,
      title: it?.title ?? (n.kind === 'done' ? `✓ ${n.members!.length} done` : `+${n.members!.length} folded`), state: it?.state ?? (n.kind === 'done' ? 'done' : 'folded'),
      view: it?.view, record: it?.record, parent: n.parent && shown.has(n.parent) ? n.parent : undefined, needs: it?.needs ?? false, asks: asks.get(n.id) ?? 0,
      data: it?.data, startedMs: it?.startedMs ?? 0, members: n.members, fresh: prev != null && !placed[n.id], ...(it ? { bucket: it.bucket } : {}),
      ...(it && forms.has(it.id) ? { form: forms.get(it.id) } : {}),
      ...(planOpen.has(n.id) ? { open: planOpen.get(n.id) } : {}),
      ...(it?.kind === 'conversation' && s.lane !== HOME && metas.get(s.lane)?.head === it.id ? { head: true } : {}),
    })
  }
  nodes.sort((a, b) => (a.id < b.id ? -1 : 1))

  // The edges, each once, between the nodes drawn: a folded end goes to its fold.
  const edges = new Map<string, GEdge>()
  const edge = (from: string | undefined, to: string | undefined, kind: EdgeKind, running = false) => {
    const s = from && at(from)
    const t = to && at(to)
    if (!s || !t || s === t || hidden.has(s) || hidden.has(t)) return
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
  return { nodes, edges: [...edges.values()].sort((a, b) => (a.id < b.id ? -1 : 1)), lanes: out, width: GUTTER + cols * COL_W, slots, bands, order, folded, hidden: hidden.size, ...(cantOpen ? { cantOpen } : {}) }
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
export function keeper(): { lay: (rows: readonly OpsRow[], lanes: readonly LaneMeta[], opts: LayoutOpts, epoch: number) => Graph } {
  let prev: Graph | null = null
  let at = 0
  return {
    lay: (rows, lanes, opts, epoch) => {
      if (epoch !== at) { prev = null; at = epoch }
      prev = layoutRows(rows, lanes, prev, opts)
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

/** What the owner has opened that the canvas can fold back: `lanes`, each lane in `expanded` whose "✓ N done" fold the
 *  layout without openings (`base`) draws (`n` is its members), and `roots`, each root in `opened` whose "+N folded"
 *  the layout with the lanes' openings but no opened roots (`baseRoots`) draws and `g` shows open, so a fold opened
 *  inside an opened lane has its way back too. Nothing when nothing is open, or an opening that changed nothing. */
export function foldBacks(g: Graph, base: Graph, expanded: ReadonlySet<string>, opened: ReadonlySet<string>, baseRoots: Graph = base): { lanes: Array<{ lane: string; n: number }>; roots: string[] } {
  const lanes: Array<{ lane: string; n: number }> = []
  for (const l of expanded) {
    const done = base.nodes.find((x) => x.id === `done:${l}`)
    if (done && !g.nodes.some((x) => x.id === done.id)) lanes.push({ lane: l, n: done.members?.length ?? 0 })
  }
  const roots = [...opened].filter((r) => baseRoots.nodes.some((x) => x.id === `fold:${r}`) &&!g.nodes.some((x) => x.id === `fold:${r}`) && g.nodes.some((x) => x.id === r))
  return { lanes, roots }
}
