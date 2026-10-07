// The chart method for the cockpit (theseus-hnof): the data-viz method's parameters filled in for the brass-and-neon
// theme, and the pure parts of the charts that follow it. It governs what is inside a chart; the frame around it (the
// panel's bezel, its engraved title, the night glass) stays the theme's.
//
// The method, in short: pick the form by the data's job (a stat tile for one number, bars for magnitude, stacked bars for
// part-to-whole, lines for trend, a table when there are more classes than colours); colour by the colour's job
// (categorical for identity, in a fixed order that follows the entity and never its rank; status only for state, always
// with an icon or a label); one axis a chart; thin marks; a legend for two series or more and none for one; text in the
// ink, never in a series colour; every chart with a hover tooltip and a table view.
//
// Every palette here passes the method's checks, dark against the panel faces (#0a1828 and #06101d) and light against
// #fcfcfb (print, export, a light mode to come): `palette.ts` runs them, and test/palette.test.ts holds each palette to
// them in CI. Pure, importing only the protocol's types and other pure modules, so `node --test` runs it as it is
// (test/viz.test.ts).
import type { ProviderCall } from './calls.ts'
import type { Tone } from './taxonomy.ts'
import { contrast, type Mode } from './palette.ts'

export type { Mode }

// ---------------------------------------------------------------- colour

/** The categorical slots in the method's fixed order: blue, orange, aqua, yellow, magenta, green, violet, red. A chart
 *  assigns them in order and never skips one. Dark passes every check on both panel faces (worst adjacent CVD ΔE 8.4,
 *  normal vision 19.3, every slot at 3:1 or more); light passes on #fcfcfb with aqua, yellow, and magenta under 3:1,
 *  which the table view relieves. */
export const CATEGORICAL: Record<Mode, readonly string[]> = {
  dark: ['#3987e5', '#d95926', '#199e70', '#c98500', '#d55181', '#008300', '#9085e9', '#e66767'],
  light: ['#2a78d6', '#eb6834', '#1baf7a', '#eda100', '#e87ba4', '#008300', '#4a3aa7', '#e34948'],
}

/** The de-emphasis gray: "other" (what folds past the eighth slot) and context. 4.8:1 or more on the faces, 3.5:1 light. */
export const OTHER = '#898781'

/** A call's token kinds, in the money river's colours, validated as a set: they pass on both panel faces (worst adjacent
 *  CVD ΔE 22.0, normal vision 28.1) and on #fcfcfb, where output (2.83:1) leans on the table view. */
export const TOKEN_KINDS = [
  { key: 'input', word: 'input', color: '#3b7fdb' },
  { key: 'cacheRead', word: 'cache reads', color: '#b8862c' },
  { key: 'cacheWrite', word: 'cache writes, 5 min', color: '#8b6cf0' },
  { key: 'cacheWrite1h', word: 'cache writes, 1 hour', color: '#c2410c' },
  { key: 'output', word: 'output', color: '#0ea5c6' },
] as const
export type TokenKind = (typeof TOKEN_KINDS)[number]['key']

/** The theme's tones as a chart's series. The tones themselves (index.css) are state colours, bright for text on the
 *  night glass (6.3:1 or more there), and as a series set they fail the method: too light for a mark (L 0.71 to 0.86,
 *  the dark band is 0.48 to 0.67), thinking and tool collapse under deuteranopia (ΔE 0.3), live and ok sit 12.1 apart
 *  for everyone. So a tone carries state with its icon or label, and when a tone's kind is a series (a timeline's model,
 *  tool, and thinking spans), it takes these steps of the same hues, snapped to passing and assigned in this order
 *  (dark: worst adjacent CVD ΔE 21.5, normal 24.1; light: 22.9 and 25.3, every step at 3:1 or more). */
export const TONE_SERIES: Record<Mode, readonly (readonly [tone: string, color: string])[]> = {
  dark: [['live', '#00a4ba'], ['money', '#7c6300'], ['tool', '#009fd7'], ['wait', '#b98a00'], ['think', '#ca5ddb'], ['ok', '#00734f'], ['model', '#987ce9'], ['fault', '#b8334e']],
  light: [['live', '#00a1b7'], ['model', '#5a39a0'], ['wait', '#b98a00'], ['fault', '#990c36'], ['tool', '#009cd3'], ['money', '#b08e00'], ['think', '#d163e2'], ['ok', '#006c4a']],
}

/** A tone drawn as a mark: a budget's line, a failed run's triangle, a state's share of a bar, a span of its kind. The
 *  tone itself stays for text, pills, and glows, where its brightness is its job; a mark takes the same hue's step in the
 *  dark band (`TONE_SERIES`), so it reads as a mark on the night glass and passes the method's checks. Idle is the
 *  de-emphasis gray. A tone as a mark still says its state in words beside it (the legend, the tip), never by colour
 *  alone. */
export const TONE_MARK: Record<Tone, string> = { ...Object.fromEntries(TONE_SERIES.dark), idle: OTHER } as Record<Tone, string>

/** A chart's chrome and ink. Dark is the cockpit's own ink on the night glass, with brass hairlines; light is the
 *  method's set, for print and export. */
export const CHROME = {
  dark: { surface: '#0a1828', text: '#efe3c8', secondary: '#c8bb9b', muted: '#9c907a', grid: 'rgba(176,141,87,0.13)', axis: 'rgba(176,141,87,0.36)' },
  light: { surface: '#fcfcfb', text: '#0b0b0b', secondary: '#52514e', muted: '#898781', grid: '#e1e0d9', axis: '#c3c2b7' },
} as const

/** The deepest face of the night glass (`.panel`'s dark end): the dark ink set inside a light fill. */
export const DEEP = '#06101d'

/** Words set inside a coloured fill (a tile, a span): the ink or the deep face, whichever stands out more from it. */
export function inkOn(fill: string, mode: Mode = 'dark'): string {
  const light = mode === 'dark' ? CHROME.dark.text : '#ffffff', dark = mode === 'dark' ? DEEP : CHROME.light.text
  return contrast(light, fill) >= contrast(dark, fill) ? light : dark
}

/** The slots of a chart's series, in the order the record first names them: a series keeps its colour as the record
 *  grows and when a filter drops others. Past `cap`, the last slot and every key after it fold into "other". */
export function slots(keys: Iterable<string>, cap = 8): { slot: Map<string, number>; folded: string[] } {
  const all = [...new Set(keys)]
  const kept = all.length > cap ? cap - 1 : all.length
  return { slot: new Map(all.slice(0, kept).map((k, i) => [k, i])), folded: all.slice(kept) }
}

/** A key's colour: its slot's, or the de-emphasis gray when it folded into "other". */
export function slotColor(slot: Map<string, number>, key: string, mode: Mode = 'dark'): string {
  const i = slot.get(key)
  return i === undefined ? OTHER : CATEGORICAL[mode][i]
}

// ---------------------------------------------------------------- marks

/** The mark specs: bars 24 px at most, a 4 px rounded data end; 2 px lines; 8 px markers in a 2 px ring of the surface;
 *  a 2 px surface gap between touching fills; an area is its line's hue at a 10% wash. */
export const MARK = { bar: 24, radius: 4, line: 2, marker: 8, ring: 2, gap: 2, wash: 0.1 } as const

/** A bar's corners in ECharts' order (top-left, top-right, bottom-right, bottom-left): the data end rounded, the baseline
 *  square. */
export function barRadius(horizontal: boolean, r: number = MARK.radius): [number, number, number, number] {
  return horizontal ? [0, r, r, 0] : [r, r, 0, 0]
}

/** In a stack, only the last segment with a value has the data end. For each category, the index of the last series
 *  with a value there, or -1. */
export function stackTop(values: readonly (readonly number[])[]): number[] {
  const n = Math.max(0, ...values.map((v) => v.length))
  return Array.from({ length: n }, (_, j) => {
    for (let i = values.length - 1; i >= 0; i--) if ((values[i][j] ?? 0) > 0) return i
    return -1
  })
}

/** A strip's offsets: the golden-ratio sequence in [-0.5, 0.5), so the dots spread evenly and never move between draws. */
export function jitter(i: number): number {
  return ((i * 0.6180339887498949) % 1) - 0.5
}

// ---------------------------------------------------------------- figures

/** A step for a value axis: 1, 2, 2.5, or 5 times a power of ten, the smallest that covers `span` in `count` steps. */
export function niceStep(span: number, count = 4): number {
  if (!(span > 0) || !Number.isFinite(span)) return 1
  const raw = span / Math.max(1, count)
  const p = 10 ** Math.floor(Math.log10(raw))
  for (const m of [1, 2, 2.5, 5, 10]) if (m * p >= raw * (1 - 1e-9)) return +(m * p).toPrecision(12)
  return 10 * p
}

/** A value axis from zero: its top and its step, every tick a clean number, the step at least `minStep` (1 for a
 *  count, so no tick reads half a commit). An empty axis gets one step. */
export function niceScale(max: number, count = 4, minStep = 0): { max: number; interval: number } {
  if (!(max > 0) || !Number.isFinite(max)) return { max: Math.max(1, minStep), interval: Math.max(1, minStep) }
  const interval = Math.max(niceStep(max, count), minStep)
  return { max: +(Math.ceil(max / interval - 1e-9) * interval).toPrecision(12), interval }
}

/** The decimals a step needs for its ticks to print apart: 0.0025 needs 4, 0.5 needs 1, 20 needs 0. */
export function stepDecimals(step: number): number {
  for (let d = 0; d < 12; d++) {
    const x = Math.abs(step) * 10 ** d
    if (x >= 1 - 1e-9 && Math.abs(Math.round(x) - x) <= 1e-6 * x) return d
  }
  return 12
}

const grouped = (v: number, d: number) => v.toLocaleString('en-US', { minimumFractionDigits: d, maximumFractionDigits: d })

/** Dollars on an axis of step `step`: each tick at the decimals the step needs, so no two ticks read alike. */
export function usdTick(step: number): (v: number) => string {
  const d = stepDecimals(step)
  return (v) => `${v < 0 ? '−' : ''}$${grouped(Math.abs(v), d)}`
}

/** Milliseconds on an axis of step `step` (a linear axis): ms, or seconds from 1,000 ms, at the decimals the step needs. */
export function msTick(step: number): (v: number) => string {
  const inS = step >= 1000
  const d = stepDecimals(inS ? step / 1000 : step)
  return (v) => (inS ? `${grouped(v / 1000, d)} s` : `${grouped(v, d)} ms`)
}

/** Milliseconds on a log axis (ticks at powers of ten, or between them on a short range): 100 µs, 1 ms, 10 ms, 1 s, 10 s.
 *  Three significant figures, so two ticks never print alike. */
export function msLogTick(v: number): string {
  const sig = (x: number) => (+x.toPrecision(3)).toLocaleString('en-US', { maximumFractionDigits: 3 })
  if (v < 1) return `${sig(v * 1000)} µs`
  if (v < 1000) return `${sig(v)} ms`
  return `${sig(v / 1000)} s`
}

/** A count on an axis: whole numbers, grouped. */
export const countTick = (v: number): string => grouped(Math.round(v), 0)

/** A plain number on an axis of step `step` (frames, say), at the decimals the step needs. */
export function numTick(step: number): (v: number) => string {
  const d = stepDecimals(step)
  return (v) => grouped(v, d)
}

/** Tokens on an axis of step `step`: whole tokens, then thousands (k) and millions (M) from a step of a thousand, at the
 *  decimals the step needs in that unit (2.5k, 5k, 7.5k), so no two ticks read alike. */
export function tokenTick(step: number): (v: number) => string {
  const [div, unit] = step >= 1e6 ? [1e6, 'M'] : step >= 1e3 ? [1e3, 'k'] : [1, '']
  const d = stepDecimals(step / div)
  return (v) => (v === 0 ? '0' : `${grouped(v / div, d)}${unit}`)
}

/** Microseconds on a linear axis of step `step` µs (a turn's own clock): µs, ms from a step of 1,000 µs, s from a step
 *  of a second, at the decimals the step needs. */
export function usTick(step: number): (v: number) => string {
  const [div, unit] = step >= 1e6 ? [1e6, 's'] : step >= 1e3 ? [1e3, 'ms'] : [1, 'µs']
  const d = stepDecimals(step / div)
  return (v) => `${grouped(v / div, d)} ${unit}`
}

/** A time axis's labels: the day where the day turns ("Oct 6"), the hour elsewhere. */
export const TIME_LABELS = { year: '{yyyy}', month: '{MMM}', day: '{MMM} {d}', hour: '{HH}:{mm}', minute: '{HH}:{mm}', second: '{HH}:{mm}:{ss}' } as const

/** Each value's share of their sum, all 0 when the sum is 0. */
export function shares(values: readonly number[]): number[] {
  const sum = values.reduce((a, b) => a + b, 0)
  return values.map((v) => (sum > 0 ? v / sum : 0))
}

/** The q-quantile of `xs` (the same rule as derive.ts's, kept here so this module imports nothing). */
export function quantile(xs: readonly number[], q: number): number | undefined {
  if (!xs.length) return undefined
  const s = [...xs].sort((a, b) => a - b)
  return s[Math.min(s.length - 1, Math.floor(q * s.length))]
}

// ---------------------------------------------------------------- the record, for the charts

export interface Bins { starts: number[]; ends: number[]; counts: Record<string, number[]>; totals: number[] }

/** Things into `n` equal bins from `start` to `end`, counted by key (a ledger row's family, say), each key's counts in
 *  the order given. A thing outside the span, or with a key not given, is left out; the end belongs to the last bin. */
export function binByKey<T>(items: readonly T[], at: (x: T) => number, key: (x: T) => string, keys: readonly string[], start: number, end: number, n: number): Bins {
  const bins = Math.max(1, Math.floor(n))
  // An empty span is a millisecond wide, never a division by zero.
  const stop = Math.max(end, start + 1)
  const size = (stop - start) / bins
  const counts: Record<string, number[]> = Object.fromEntries(keys.map((k) => [k, new Array<number>(bins).fill(0)]))
  const totals = new Array<number>(bins).fill(0)
  for (const x of items) {
    const t = at(x)
    if (t < start || t > stop) continue
    const row = counts[key(x)]
    if (!row) continue
    const i = Math.min(bins - 1, Math.floor((t - start) / size))
    row[i]++
    totals[i]++
  }
  const starts = Array.from({ length: bins }, (_, i) => start + i * size)
  return { starts, ends: starts.map((s, i) => (i === bins - 1 ? stop : s + size)), counts, totals }
}

export interface Rect { x: number; y: number; w: number; h: number }

/** The squarified treemap (Bruls, Huizing, and van Wijk): each value a rectangle of its share of `r`'s area, laid in rows
 *  that keep the tiles as near square as their order allows, the largest first. The rectangles come back in the values'
 *  own order; a value of zero or less gets an empty one. */
export function squarify(values: readonly number[], r: Rect): Rect[] {
  const out: Rect[] = values.map(() => ({ x: r.x, y: r.y, w: 0, h: 0 }))
  const order = values.map((v, i) => [v, i] as const).filter(([v]) => v > 0).sort((a, b) => b[0] - a[0] || a[1] - b[1]).map(([, i]) => i)
  const total = order.reduce((a, i) => a + values[i], 0)
  if (!(total > 0) || !(r.w > 0) || !(r.h > 0)) return out
  const scale = (r.w * r.h) / total
  const area = (i: number) => values[i] * scale
  let { x, y, w, h } = r
  // How far a row's worst tile is from square, laid along a side of this length.
  const worst = (row: number[], side: number) => {
    const s = row.reduce((a, i) => a + area(i), 0)
    let max = 0, min = Infinity
    for (const i of row) { max = Math.max(max, area(i)); min = Math.min(min, area(i)) }
    return Math.max((side * side * max) / (s * s), (s * s) / (side * side * min))
  }
  const lay = (row: number[]) => {
    const s = row.reduce((a, i) => a + area(i), 0)
    if (w >= h) {
      const t = s / h
      let yy = y
      for (const i of row) { const hh = area(i) / t; out[i] = { x, y: yy, w: t, h: hh }; yy += hh }
      x += t; w -= t
    } else {
      const t = s / w
      let xx = x
      for (const i of row) { const ww = area(i) / t; out[i] = { x: xx, y, w: ww, h: t }; xx += ww }
      y += t; h -= t
    }
  }
  let row: number[] = []
  for (let k = 0; k < order.length;) {
    const side = Math.min(w, h)
    if (!row.length || worst([...row, order[k]], side) <= worst(row, side)) { row.push(order[k]); k++ }
    else { lay(row); row = [] }
  }
  if (row.length) lay(row)
  return out
}

export interface Growth { session: string; points: [number, number][]; latest: number; max: number; first: number; last: number }

/** Each session's estimated prompt size, compile by compile: its points, its latest and largest, its first and last
 *  compile's time; the largest latest prompt first. */
export function growthBySession(series: ReadonlyMap<string, readonly [number, number][]>): Growth[] {
  return [...series].filter(([, pts]) => pts.length).map(([session, pts]) => ({
    session, points: [...pts] as [number, number][], latest: pts[pts.length - 1][1], max: Math.max(...pts.map((p) => p[1])),
    first: pts[0][0], last: pts[pts.length - 1][0],
  })).sort((a, b) => b.latest - a.latest || b.last - a.last || a.session.localeCompare(b.session))
}

export const BUCKETS = ['5 min', 'hour', 'day'] as const
export type Bucket = (typeof BUCKETS)[number]

/** The start of the local five minutes, hour, or day a moment falls in (ms). */
export function bucketStart(at: number, bucket: Bucket): number {
  const d = new Date(at)
  if (bucket === 'day') d.setHours(0, 0, 0, 0)
  else if (bucket === 'hour') d.setMinutes(0, 0, 0)
  else d.setMinutes(d.getMinutes() - (d.getMinutes() % 5), 0, 0)
  return d.getTime()
}

/** The end of the bucket that starts at `start` (a day is the calendar's, 23 or 25 hours across a clock change). */
export function bucketEnd(start: number, bucket: Bucket): number {
  const d = new Date(start)
  if (bucket === 'day') d.setDate(d.getDate() + 1)
  else if (bucket === 'hour') d.setHours(d.getHours() + 1)
  else d.setMinutes(d.getMinutes() + 5)
  return d.getTime()
}

/** The bucket a record of this span reads best in: enough columns to show a shape, never one column for a short record. */
export function bucketFor(spanMs: number): Bucket {
  return spanMs < 3 * 3600_000 ? '5 min' : spanMs < 2 * 86_400_000 ? 'hour' : 'day'
}

type Spent = Pick<ProviderCall, 'at' | 'model' | 'cost'>

export interface SpendSeries {
  /** Each bucket with a billed call, oldest first: its start and its end. */
  starts: number[]; ends: number[]
  /** Spend per bucket, by key, keys in the order the record first names them. */
  series: { key: string; values: number[] }[]
  /** Each bucket's spend, and the running total at its end. */
  totals: number[]; cumulative: number[]
}

/** Spend by local hour or day and by key (a model, or "other" past the slots). */
export function spendByBucket(calls: readonly Spent[], bucket: Bucket, keyOf: (model: string) => string = (m) => m): SpendSeries {
  const by = new Map<number, Map<string, number>>()
  const keys: string[] = []
  for (const c of calls) {
    const k = bucketStart(c.at, bucket)
    const key = keyOf(c.model)
    if (!keys.includes(key)) keys.push(key)
    const m = by.get(k) ?? new Map<string, number>()
    m.set(key, (m.get(key) ?? 0) + c.cost)
    by.set(k, m)
  }
  const starts = [...by.keys()].sort((a, b) => a - b)
  const totals = starts.map((s) => [...by.get(s)!.values()].reduce((a, b) => a + b, 0))
  let run = 0
  return {
    starts, ends: starts.map((s) => bucketEnd(s, bucket)),
    series: keys.map((key) => ({ key, values: starts.map((s) => by.get(s)!.get(key) ?? 0) })),
    totals, cumulative: totals.map((t) => (run += t)),
  }
}

export interface SpendNode { key: string; cost: number; calls: number; children: SpendNode[] }

/** Spend as a tree, provider → model → session, each level most first. Sessions are their ids (two sessions with one
 *  title stay two); a call with no session counts under "—". */
export function spendTree(calls: readonly Pick<ProviderCall, 'provider' | 'model' | 'session_id' | 'cost'>[]): SpendNode[] {
  const root = new Map<string, Map<string, Map<string, { cost: number; calls: number }>>>()
  for (const c of calls) {
    const p = root.get(c.provider) ?? new Map(); root.set(c.provider, p)
    const m = p.get(c.model) ?? new Map(); p.set(c.model, m)
    const sid = c.session_id ?? '—'
    const s = m.get(sid) ?? { cost: 0, calls: 0 }
    s.cost += c.cost; s.calls += 1
    m.set(sid, s)
  }
  const order = (xs: SpendNode[]) => xs.sort((a, b) => b.cost - a.cost || a.key.localeCompare(b.key))
  return order([...root].map(([prov, models]) => {
    const ms = order([...models].map(([model, sessions]) => {
      const ss = order([...sessions].map(([sid, v]) => ({ key: sid, cost: v.cost, calls: v.calls, children: [] })))
      return { key: model, cost: sum(ss.map((s) => s.cost)), calls: sum(ss.map((s) => s.calls)), children: ss }
    }))
    return { key: prov, cost: sum(ms.map((m) => m.cost)), calls: sum(ms.map((m) => m.calls)), children: ms }
  }))
}

const sum = (xs: number[]) => xs.reduce((a, b) => a + b, 0)

export interface SessionSpend { session: string; cost: number; calls: number; byModel: { model: string; cost: number }[] }

/** Each session's spend, most first, split by model in the order the record first names the models. */
export function spendBySession(calls: readonly Pick<ProviderCall, 'session_id' | 'model' | 'cost'>[], models: readonly string[]): SessionSpend[] {
  const m = new Map<string, { cost: number; calls: number; by: Map<string, number> }>()
  for (const c of calls) {
    if (!c.session_id) continue
    const r = m.get(c.session_id) ?? { cost: 0, calls: 0, by: new Map<string, number>() }
    r.cost += c.cost; r.calls += 1
    r.by.set(c.model, (r.by.get(c.model) ?? 0) + c.cost)
    m.set(c.session_id, r)
  }
  return [...m].map(([session, r]) => ({
    session, cost: r.cost, calls: r.calls,
    byModel: [...r.by].sort((a, b) => rank(models, a[0]) - rank(models, b[0])).map(([model, cost]) => ({ model, cost })),
  })).sort((a, b) => b.cost - a.cost || a.session.localeCompare(b.session))
}

const rank = (xs: readonly string[], x: string) => { const i = xs.indexOf(x); return i < 0 ? xs.length : i }

export interface Latency {
  model: string
  first: { at: number; ms: number }[]; total: { at: number; ms: number }[]
  firstP50?: number; firstP95?: number; totalP50?: number; totalP95?: number
}

/** Each model's first-token and total latencies, call by call, with their p50 and p95; models in the order given. */
export function latencyByModel(calls: readonly Pick<ProviderCall, 'at' | 'model' | 'first_token_ms' | 'total_ms'>[], models: readonly string[]): Latency[] {
  return models.map((model) => {
    const mine = calls.filter((c) => c.model === model)
    const first = mine.filter((c) => typeof c.first_token_ms === 'number').map((c) => ({ at: c.at, ms: c.first_token_ms! }))
    const total = mine.filter((c) => typeof c.total_ms === 'number').map((c) => ({ at: c.at, ms: c.total_ms! }))
    const f = first.map((x) => x.ms), t = total.map((x) => x.ms)
    return { model, first, total, firstP50: quantile(f, 0.5), firstP95: quantile(f, 0.95), totalP50: quantile(t, 0.5), totalP95: quantile(t, 0.95) }
  }).filter((l) => l.first.length || l.total.length)
}

/** The tokens of each kind across the calls: the 1-hour cache writes apart from the 5-minute ones, as money.ts prices them. */
export function kindTokens(calls: readonly Pick<ProviderCall, 'usage'>[]): Record<TokenKind, number> {
  const t: Record<TokenKind, number> = { input: 0, cacheRead: 0, cacheWrite: 0, cacheWrite1h: 0, output: 0 }
  for (const { usage: u } of calls) {
    const w1h = Math.min(u.cache_creation_1h_input_tokens ?? 0, u.cache_creation_input_tokens)
    t.input += u.input_tokens; t.cacheRead += u.cache_read_input_tokens
    t.cacheWrite += u.cache_creation_input_tokens - w1h; t.cacheWrite1h += w1h; t.output += u.output_tokens
  }
  return t
}

// ---------------------------------------------------------------- ECharts pieces (plain objects, typed where they're used)

const MONO = "'JetBrains Mono Variable', ui-monospace, monospace"
const SANS = "'Inter Variable', ui-sans-serif, system-ui, sans-serif"
export const FONTS = { mono: MONO, sans: SANS } as const

/** A value axis: no axis line or ticks, solid hairline grid, muted tabular tick labels. */
export function valueAxis(mode: Mode = 'dark') {
  const c = CHROME[mode]
  return {
    type: 'value' as const,
    axisLine: { show: false }, axisTick: { show: false },
    splitLine: { show: true, lineStyle: { color: c.grid, width: 1, type: 'solid' as const } },
    axisLabel: { color: c.muted, fontSize: 10, fontFamily: MONO },
  }
}

/** A category or time axis: a hairline baseline, no grid, muted labels. */
export function baseAxis(mode: Mode = 'dark') {
  const c = CHROME[mode]
  return {
    axisLine: { show: true, lineStyle: { color: c.axis, width: 1, type: 'solid' as const } },
    axisTick: { show: false }, splitLine: { show: false },
    axisLabel: { color: c.muted, fontSize: 10, fontFamily: MONO, hideOverlap: true },
  }
}

/** A budget's line (an ECharts markLine datum): solid, in the fault tone's step for marks, its words in the ink beside it,
 *  so the colour never carries it alone. */
export function budgetLine(at: { xAxis: number } | { yAxis: number }, words: string, position: 'start' | 'end' | 'insideEndTop') {
  return { ...at, lineStyle: { color: TONE_MARK.fault, width: 1, type: 'solid' as const }, label: { formatter: words, color: CHROME.dark.secondary, fontSize: 10, fontFamily: MONO, position } }
}

/** The tooltip's frame, the cockpit's (chart.ts's): night glass in a brass rim. The content is the caller's element. */
export const TIP_FRAME = {
  backgroundColor: 'rgba(6,15,27,0.96)',
  borderColor: 'rgba(214,165,72,0.55)',
  padding: [7, 10] as number[],
  textStyle: { color: CHROME.dark.text, fontSize: 12, fontFamily: SANS },
  extraCssText: 'border-radius: 8px; box-shadow: 0 10px 28px rgba(0,0,0,.6), 0 0 16px -6px rgba(34,211,238,.35);',
}
