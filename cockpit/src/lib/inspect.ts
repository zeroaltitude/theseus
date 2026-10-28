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
  if (/^continuation|^advancer|^session\./.test(n)) return 'store'
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

/** The time a person was waited on: each `tool.confirm_requested` row to its answer, on the turn's clock. The answer is
 *  the row that carries the question's correlation id, else the next answer row. `originMs` is the unix ms the clock
 *  began at. The wait may outlast the turn (a parked turn waits for its answer elsewhere): `waited_us` in the
 *  bar's attributes is the whole of it. */
export function personWaits(rows: readonly InspectRow[], originMs: number): Omit<Bar, 'row'>[] {
  const out: Omit<Bar, 'row'>[] = []
  const answers = rows.filter((r) => r.kind === 'action.confirm_answered' || r.kind === 'action.confirmed' || r.kind === 'action.declined' || r.kind === 'action.expired')
  const used = new Set<number>()
  for (const q of rows.filter((r) => r.kind === 'tool.confirm_requested')) {
    const cid = (q.data as { correlation_id?: string } | null)?.correlation_id
    const a = answers.find((r) => !used.has(r.position) && r.position > q.position && (cid ? (r.data as { correlation_id?: string } | null)?.correlation_id === cid : true))
    if (a) used.add(a.position)
    const start = (q.at_ms - originMs) * 1000
    const end = a ? Math.max((a.at_ms - originMs) * 1000, start + 1) : start + 1
    out.push({
      id: `person-${q.position}`, lane: 'person', start, end,
      name: a ? `waiting on you (${a.kind.split('.')[1]}, ${waitWords(end - start)})` : 'waiting on you (unanswered)',
      kind: 'person', attrs: { asked: q.data, answered: a?.data ?? null, waited_us: end - start }, loop: null,
    })
  }
  return out
}

/** Where a request carried a value the vault resolved: each `request.redacted` row of a model call, a mark at the
 *  moment it was sent, beside that loop's provider call. `originMs` is the unix ms the clock began at. */
export function redactionMarks(rows: readonly InspectRow[], originMs: number): Omit<Bar, 'row'>[] {
  return rows
    .filter((r) => r.kind === 'request.redacted' && (r.data as { op?: string } | null)?.op === 'stream')
    .map((r) => {
      const d = r.data as { names?: string[]; count?: number; substituted?: boolean; loop?: number | null }
      const start = (r.at_ms - originMs) * 1000
      const what = d.names?.length ? d.names.join(', ') : `${d.count ?? 0} vault value${d.count === 1 ? '' : 's'}`
      return {
        id: `redacted-${r.position}`, lane: 'marks' as Lane, start, end: start + 1,
        name: `request.redacted: ${what} (${d.substituted ? 'substituted' : 'sent as written'})`,
        kind: 'mark', attrs: r.data, loop: d.loop ?? null,
      }
    })
}

/** A wait in a few words. */
export function waitWords(us: number): string {
  const s = us / 1e6
  if (s < 1) return `${Math.round(us / 1000)} ms`
  if (s < 120) return `${s.toFixed(1)} s`
  if (s < 7200) return `${Math.round(s / 60)} min`
  return `${(s / 3600).toFixed(1)} h`
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
  // A wait that outlasts the turn is drawn to the turn's end, so it does not flatten the rest on the axis.
  bars.push(...personWaits(rows, origin).filter((b) => b.start < end).map((b) => ({ ...b, end: Math.min(b.end, end) })))
  bars.push(...redactionMarks(rows, origin).filter((b) => b.start < end))
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

/** A loop's model call timed from the trace: the call, the first byte and the first token (µs from the call's start),
 *  and the streaming after the first token. Null when the loop has no provider span. */
export interface CallTiming { call: number; firstByte: number | null; firstToken: number | null; streaming: number | null }

export function callTiming(bars: readonly Bar[], loop: number): CallTiming | null {
  const call = bars.find((b) => b.loop === loop && b.lane === 'model' && b.kind === 'provider')
  if (!call) return null
  const mark = (n: string) => bars.find((b) => b.loop === loop && b.kind === 'mark' && b.name === n && b.start >= call.start && b.start <= call.end)
  const fb = mark('first_byte')
  const ft = mark('first_token')
  return {
    call: call.end - call.start,
    firstByte: fb ? fb.start - call.start : null,
    firstToken: ft ? ft.start - call.start : null,
    streaming: ft ? call.end - ft.start : null,
  }
}

/** The ledger rows that name a tool call: those carrying its tool_use_id, or its action's correlation id. */
export function rowsOfCall<T extends { data: unknown }>(rows: readonly T[], toolUseId: string, correlationId: string | null | undefined): T[] {
  return rows.filter((r) => {
    const d = r.data as Record<string, unknown> | null
    if (!d || typeof d !== 'object') return false
    return d.tool_use_id === toolUseId || (!!correlationId && d.correlation_id === correlationId)
  })
}

/** Where a request's cache breakpoints fall among its parts: the index of the last part of each breakpoint's block (or,
 *  for a layer named by a part, the part of that name). */
export function breakpointParts(parts: readonly { block: string; name: string }[], breakpoints: readonly string[]): Set<number> {
  const at = new Set<number>()
  for (const bp of breakpoints) {
    let k = -1
    parts.forEach((p, i) => { if (p.block === bp) k = i })
    if (k < 0) parts.forEach((p, i) => { if (p.name === bp) k = i })
    if (k >= 0) at.add(k)
  }
  return at
}
