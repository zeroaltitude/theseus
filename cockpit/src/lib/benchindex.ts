// The benchmark manifest's pure half (theseus-jda0z): `docs/benchmarks/index.json`, which bench/report/manifest.py writes
// and every run's report step appends to. One row per measured arm of a dataset; this reads it as text, checks it, and
// gives the Benchmarks page the newest measurement of each (benchmark, tier, arm) with its history. A manifest it cannot
// trust gives no series and says why: the page shows the reason, never a crash.

export type Interval = { value: number; lo: number; hi: number }

/** One arm of one dataset, as the manifest states it. */
export type IndexRow = {
  report: string; benchmark: string; tier: string | null; arm: string; variant: string; harness: string
  model: string | null; date: string; commit: string | null; dataset: string
  trials: number; tasks: number | null
  /** The pass rate with its Wilson interval, and the counts behind it. */
  pass: Interval & { k: number; n: number }
  usdPerTrial: Interval | null
  /** A comparison arm the report drew against, or a row repeating an earlier report's figures: not a new measurement. */
  carried: boolean
}

/** The newest measurement of one (benchmark, tier, arm), and the measurements before it, oldest to newest. */
export type Series = { key: string; benchmark: string; tier: string | null; arm: string; newest: IndexRow; history: IndexRow[] }

export type Manifest = { rows: IndexRow[]; series: Series[]; reason: string | null }

const EMPTY = (reason: string): Manifest => ({ rows: [], series: [], reason })

const isNum = (v: unknown): v is number => typeof v === 'number' && Number.isFinite(v)
const isStr = (v: unknown): v is string => typeof v === 'string' && v.length > 0
const isObj = (v: unknown): v is Record<string, unknown> => typeof v === 'object' && v !== null && !Array.isArray(v)
const orNull = <T>(v: unknown, ok: (x: unknown) => x is T): T | null => (ok(v) ? v : null)

function interval(v: unknown): Interval | null {
  return isObj(v) && isNum(v.value) && isNum(v.lo) && isNum(v.hi) ? { value: v.value, lo: v.lo, hi: v.hi } : null
}

/** A row, or the first thing wrong with it. */
function rowOf(raw: unknown): IndexRow | string {
  if (!isObj(raw)) return 'is not an object'
  for (const f of ['report', 'benchmark', 'arm', 'variant', 'harness', 'date', 'dataset'] as const) {
    if (!isStr(raw[f])) return `has no \`${f}\``
  }
  if (!/^\d{4}-\d{2}-\d{2}$/.test(raw.date as string)) return `has a \`date\` that is not YYYY-MM-DD`
  if (!isNum(raw.trials)) return 'has no `trials`'
  const m = raw.metrics
  const p = isObj(m) && isObj(m.pass_rate) ? m.pass_rate : null
  if (!p || !isNum(p.value) || !isNum(p.lo) || !isNum(p.hi) || !isNum(p.k) || !isNum(p.n)) return 'has no whole `metrics.pass_rate`'
  return {
    report: raw.report as string, benchmark: raw.benchmark as string, tier: orNull(raw.tier, isStr),
    arm: raw.arm as string, variant: raw.variant as string, harness: raw.harness as string,
    model: orNull(raw.model, isStr), date: raw.date as string, commit: orNull(raw.commit, isStr), dataset: raw.dataset as string,
    trials: raw.trials, tasks: orNull(raw.tasks, isNum),
    pass: { value: p.value, lo: p.lo, hi: p.hi, k: p.k, n: p.n },
    usdPerTrial: isObj(m) ? interval(m.usd_per_trial) : null,
    carried: raw.comparison === true || isStr(raw.carried_from),
  }
}

const order = (a: IndexRow, b: IndexRow) =>
  a.date.localeCompare(b.date) || a.report.localeCompare(b.report) || a.variant.localeCompare(b.variant)

/** The series of the measured rows. Within a report that ran several variants of an arm, the last by name speaks for it. */
export function seriesOf(rows: readonly IndexRow[]): Series[] {
  const groups = new Map<string, IndexRow[]>()
  for (const r of rows.filter((x) => !x.carried).sort(order)) {
    const key = `${r.benchmark}\u0000${r.tier ?? ''}\u0000${r.arm}`
    const g = groups.get(key)
    if (g) g.push(r); else groups.set(key, [r])
  }
  const out: Series[] = []
  for (const [key, g] of groups) {
    const history = g.filter((r, i) => i === g.length - 1 || g[i + 1].report !== r.report)
    const newest = history[history.length - 1]
    out.push({ key, benchmark: newest.benchmark, tier: newest.tier, arm: newest.arm, newest, history })
  }
  // Theseus first within a benchmark and tier, the rest by name.
  const rank = (s: Series) => (s.arm === 'theseus' ? '' : s.arm)
  return out.sort((a, b) =>
    a.benchmark.localeCompare(b.benchmark) || (a.tier ?? '').localeCompare(b.tier ?? '') || rank(a).localeCompare(rank(b)))
}

/** Read the manifest's text. Anything it cannot trust gives an empty result and the reason, never a throw. */
export function readManifest(text: string | null | undefined): Manifest {
  if (!text) return EMPTY('the build embeds no benchmark manifest')
  let doc: unknown
  try { doc = JSON.parse(text) } catch { return EMPTY('the manifest is not valid JSON') }
  if (!isObj(doc)) return EMPTY('the manifest is not an object')
  if (doc.version !== 1) return EMPTY(`the manifest is version ${String(doc.version)}; this build reads version 1`)
  if (!Array.isArray(doc.runs)) return EMPTY('the manifest has no `runs` list')
  const rows: IndexRow[] = []
  for (const [i, raw] of doc.runs.entries()) {
    const r = rowOf(raw)
    if (typeof r === 'string') return EMPTY(`manifest row ${i} ${r}`)
    rows.push(r)
  }
  return { rows, series: seriesOf(rows), reason: null }
}

/** A series' points for a small line: x and y in a w by h box, y from the lowest to the highest pass rate (a flat series centres). */
export function sparkline(values: readonly number[], w: number, h: number): [number, number][] {
  if (!values.length) return []
  const lo = Math.min(...values), hi = Math.max(...values)
  return values.map((v, i) => [
    values.length === 1 ? w / 2 : (i / (values.length - 1)) * w,
    hi === lo ? h / 2 : h - ((v - lo) / (hi - lo)) * h,
  ])
}

// ---------------------------------------------------------------- which row leads an arm

/** Which of an arm's measurements speaks for it on a card. THE OWNER'S OPEN QUESTION lives here, in this one constant:
 *  - `'newest'`: the arm's newest row, whatever it measured. Theseus's newest is a 10-23 retest of 6 tasks (22.2% on
 *    18 trials), which the card ranks by rate (`benchcards.ts`), so it sits at the bottom, 55 points under Theseus's
 *    77.2% over 89 tasks, not above the rivals' 75.3% on 267 trials.
 *  - `'newest-comparable'`: the newest row whose task count equals the group's largest, else the newest.
 *  Either way every row prints its trials and tasks, and a short row carries a chip. Do not change this without the
 *  owner's answer; changing it is this one line. */
export type LeadRule = 'newest' | 'newest-comparable'
export const LEAD: LeadRule = 'newest-comparable'

/** The most tasks any measurement in a group (the series of one benchmark and tier) covered. */
export const groupTasks = (group: readonly Series[]): number => Math.max(0, ...group.flatMap((s) => s.history.map((r) => r.tasks ?? 0)))

/** The row that speaks for a series under a rule; `largest` is the group's `groupTasks`. */
export function leadRow(s: Series, rule: LeadRule, largest: number = groupTasks([s])): IndexRow {
  if (rule === 'newest') return s.newest
  for (let i = s.history.length - 1; i >= 0; i--) if ((s.history[i].tasks ?? 0) === largest) return s.history[i]
  return s.newest
}
