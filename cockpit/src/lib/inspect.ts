// The turn inspector's pure parts (theseus-x9swu): a turn's trace laid out in lanes on one time axis (the loop, each
// model call, parallel tool calls in lanes of their own, waits, compaction, recall, the judges, and the time spent
// waiting on a person), the words for a loop's request against the one before it, and a turn's totals. No React and no
// protocol client, so `node --test` runs it (test/inspect.test.ts).
import type { InspectLoop, InspectRow, Span } from '@protocol'
import { flatten, type Flat } from './spans.ts'

/** The lanes, top to bottom. */
export const LANES = ['loops', 'model', 'tools', 'waits', 'person', 'compile', 'recall', 'judges', 'store', 'marks'] as const
export type Lane = (typeof LANES)[number]

/** One bar of the flame: where it sits (a lane and a row in it), its span on the turn's clock (µs), and what it is. */
export interface Bar {
  id: string
  lane: Lane
  row: number
  start: number
  end: number
  name: string
  kind: string
  attrs: unknown
  /** The loop it belongs to, when the trace says. */
  loop: number | null
}

/** A span's lane, by its kind and its name. */
export function laneOf(f: Pick<Flat, 'name' | 'kind'>): Lane {
  const n = f.name.toLowerCase()
  if (f.kind === 'mark') return 'marks'
  if (f.kind === 'turn' || f.kind === 'loop') return 'loops'
  if (/job_wait|wait|lock|admission|queue/.test(n) || f.kind === 'lock') return 'waits'
  if (f.kind === 'provider' || /provider|model/.test(n)) return 'model'
  if (f.kind === 'tool' || f.kind === 'tools') return 'tools'
  if (/recall/.test(n)) return 'recall'
  if (/compact|compile|summar/.test(n) || f.kind === 'compile') return 'compile'
  if (/judge|route|jev/.test(n)) return 'judges'
  return 'store'
}

/** The loop a span sits in: the nearest `loop N` above it in the flat order. */
function loopOf(flat: readonly Flat[], i: number): number | null {
  const d = flat[i].depth
  for (let j = i; j >= 0; j--) {
    if (flat[j].kind === 'loop' && flat[j].depth <= d) {
      const m = /(\d+)/.exec(flat[j].name)
      return m ? Number(m[1]) : null
    }
  }
  return null
}

/** Rows in a lane: each bar takes the first row whose last bar ended before it began, so calls that ran together
 *  stack, each in its own row, and calls that ran in turn share one. */
export function pack(bars: Omit<Bar, 'row'>[]): Bar[] {
  const ends: Record<string, number[]> = {}
  return [...bars]
    .sort((a, b) => a.start - b.start || a.end - b.end)
    .map((b) => {
      const rows = (ends[b.lane] ??= [])
      let r = rows.findIndex((e) => e <= b.start)
      if (r < 0) { r = rows.length; rows.push(b.end) } else rows[r] = Math.max(b.end, b.start)
      return { ...b, row: r }
    })
}

/** The time a person was waited on: each `tool.confirm_requested` row to the next answer, on the turn's clock.
 *  `originMs` is the unix ms the clock began at. */
export function personWaits(rows: readonly InspectRow[], originMs: number): Omit<Bar, 'row'>[] {
  const out: Omit<Bar, 'row'>[] = []
  let open: InspectRow | null = null
  for (const r of rows) {
    if (r.kind === 'tool.confirm_requested' && !open) open = r
    else if (open && (r.kind === 'action.confirm_answered' || r.kind === 'action.confirmed' || r.kind === 'action.declined' || r.kind === 'action.expired')) {
      out.push({ id: `person-${open.position}`, lane: 'person', start: (open.at_ms - originMs) * 1000, end: Math.max((r.at_ms - originMs) * 1000, (open.at_ms - originMs) * 1000 + 1), name: `waiting on you (${r.kind.split('.')[1]})`, kind: 'person', attrs: { asked: open.data, answered: r.data }, loop: null })
      open = null
    }
  }
  return out
}

/** The whole turn as bars: the trace's spans by lane, and the person's waits from the rows. */
export function layout(trace: Span | null | undefined, rows: readonly InspectRow[] = [], endedMs: number | null = null, startedMs = 0): { bars: Bar[]; end: number; rowsIn: Record<Lane, number> } {
  const flat = trace ? flatten(trace) : []
  const end = Math.max(1, ...flat.map((f) => f.end))
  const bars: Omit<Bar, 'row'>[] = flat.map((f, i) => ({
    id: `s${i}`, lane: laneOf(f), start: f.start, end: f.end, name: f.name, kind: f.kind, attrs: f.attrs, loop: loopOf(flat, i),
  }))
  // The trace's clock starts at the turn's arrival; the rows' times are unix ms. Anchor on the turn's end when known.
  const origin = endedMs != null && trace ? endedMs - Math.round(end / 1000) : startedMs
  bars.push(...personWaits(rows, origin))
  const packed = pack(bars)
  const rowsIn = Object.fromEntries(LANES.map((l) => [l, 0])) as Record<Lane, number>
  for (const b of packed) rowsIn[b.lane] = Math.max(rowsIn[b.lane], b.row + 1)
  return { bars: packed, end: Math.max(end, ...packed.map((b) => b.end)), rowsIn }
}

/** The bars in the order the keyboard walks them: by start, then by lane. */
export function walk(bars: readonly Bar[]): Bar[] {
  return [...bars].sort((a, b) => a.start - b.start || LANES.indexOf(a.lane) - LANES.indexOf(b.lane) || a.row - b.row)
}

/** The bar after or before `id` in the walk; the first/last when `id` is none. */
export function step(bars: readonly Bar[], id: string | null, by: 1 | -1): Bar | null {
  const w = walk(bars)
  if (!w.length) return null
  const i = w.findIndex((b) => b.id === id)
  if (i < 0) return by > 0 ? w[0] : w[w.length - 1]
  return w[Math.min(w.length - 1, Math.max(0, i + by))]
}

/** A view window on the axis (µs), zoomed about `at` by `factor` (< 1 zooms in) and kept inside `[0, end]`. */
export function zoom(win: [number, number], at: number, factor: number, end: number): [number, number] {
  const span = Math.min(end, Math.max(end / 2000, (win[1] - win[0]) * factor))
  let lo = at - ((at - win[0]) / (win[1] - win[0] || 1)) * span
  lo = Math.min(Math.max(0, lo), end - span)
  return [lo, lo + span]
}

/** The window moved by `by` µs, kept inside `[0, end]`. */
export function pan(win: [number, number], by: number, end: number): [number, number] {
  const span = win[1] - win[0]
  const lo = Math.min(Math.max(0, win[0] + by), Math.max(0, end - span))
  return [lo, lo + span]
}

/** What a loop's request changed against the one before it, in words and numbers. */
export interface LoopDiff {
  tokens: number | null
  messages: number | null
  tools: number | null
  digestChanged: boolean
  layers: string[]
  breakpointsChanged: boolean
  addedNodes: number
  recompiled: boolean
}

export function loopDiff(prev: InspectLoop | undefined, cur: InspectLoop): LoopDiff {
  const a = prev?.compiled
  const b = cur.compiled
  const num = (x: number | undefined, y: number | undefined) => (x == null || y == null ? null : y - x)
  const layers: string[] = []
  const la = a?.cache?.layers ?? {}
  const lb = b?.cache?.layers ?? {}
  for (const k of new Set([...Object.keys(la), ...Object.keys(lb)])) if (la[k] !== lb[k]) layers.push(k)
  return {
    tokens: num(a?.est_tokens, b?.est_tokens),
    messages: num(a?.messages, b?.messages),
    tools: num(a?.tools, b?.tools),
    digestChanged: !!a && !!b && a.digest !== b.digest,
    layers,
    breakpointsChanged: !!a && !!b && a.cache.breakpoints.join() !== b.cache.breakpoints.join(),
    addedNodes: cur.added.length,
    recompiled: b?.decision === 'recompile',
  }
}

/** A signed count for a diff: `+1,240`, `−3`, `±0`; a missing figure is a dash. */
export function signed(n: number | null): string {
  if (n == null) return '—'
  if (n === 0) return '±0'
  return `${n > 0 ? '+' : '−'}${Math.abs(n).toLocaleString('en-US')}`
}

/** A content block as a heading: its type, and what size it is. */
export function blockWords(b: Record<string, unknown>): string {
  const len = (k: string) => (typeof b[k] === 'string' ? (b[k] as string).length : (typeof b.chars === 'number' ? b.chars : 0))
  switch (b.type) {
    case 'thinking': return `thinking · ${len('thinking').toLocaleString('en-US')} chars`
    case 'text': return `text · ${len('text').toLocaleString('en-US')} chars`
    case 'tool_use': return `tool_use · ${String(b.name ?? '')}`
    case 'redacted_thinking': return 'redacted thinking'
    default: return String(b.type ?? 'block')
  }
}

/** The share of a request's input that the cache served: read over read + written + uncached. */
export function cacheShare(u: { input_tokens: number; cache_read_input_tokens: number; cache_creation_input_tokens: number }): number | null {
  const all = u.input_tokens + u.cache_read_input_tokens + u.cache_creation_input_tokens
  return all > 0 ? u.cache_read_input_tokens / all : null
}
