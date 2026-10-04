// Jev's judgments (M5 23b) as the cockpit reads them: a `judge.call` row in short ("Jev (shadow): complete 0.93 ·
// agrees"), each pack's calls, cost, and latency by workload class, and an answer's probabilities as bars. Pure, so
// `npm test` runs it with node alone: it imports nothing but the protocol's types.
import type { LedgerEntry } from '@protocol'

type D = Record<string, any>

/** One judgment, as a log line and the session's view show it. */
export interface Judged {
  id: string
  at: number
  session: string | null
  turn: string | null
  pack: string
  version: number
  mode: string
  /** `answered`, `skipped`, or `failed`. */
  outcome: string
  /** Why it was skipped, or the failure's class. */
  why?: string
  /** The headline answer: what it leans to, its number, and its band. */
  lean?: string
  value?: number
  band?: string
  disagrees: boolean
  drift: boolean
  costMicros: number | null
  totalMs: number
  /** The workload class the core gave it (`reply`, `tools`, `task`, …). */
  cls: string
}

/** What an answer leans to: a Choice's option, a Score's level, a Noul's yes or no. */
export function leanOf(top: D | undefined): string {
  if (!top) return '?'
  switch (top.kind) {
    case 'choice': return String(top.value)
    case 'level': return `level ${top.value}`
    case 'noul': return top.value ? 'yes' : 'no'
  }
  return '?'
}

export function judged(r: LedgerEntry): Judged {
  const d = (r.data ?? {}) as D
  const o = (d.outcome ?? {}) as D
  // The answer it is said by: the row's `headline` question (the core's, in its pack's order), else the first.
  const answers = (d.answers ?? []) as D[]
  const first = answers.find((a) => a.question === d.headline && a.about == null) ?? answers[0]
  return {
    id: String(d.id ?? ''),
    at: r.at_unix_ms,
    session: r.session_id ?? null,
    turn: r.turn_id ?? null,
    pack: String(d.pack ?? '?'),
    version: Number(d.version ?? 0),
    mode: String(d.mode ?? '?'),
    outcome: String(o.outcome ?? '?'),
    ...(o.outcome === 'skipped' ? { why: String(o.reason ?? '?') } : o.outcome === 'failed' ? { why: String(o.class ?? '?') } : {}),
    ...(first ? { lean: leanOf(first.band?.top), value: Number(first.band?.value ?? 0), band: String(first.band?.band ?? '?') } : {}),
    disagrees: d.disagrees === true,
    drift: d.model_drift === true,
    costMicros: typeof d.cost_micros === 'number' ? d.cost_micros : null,
    totalMs: Number(d.timing?.total_ms ?? 0),
    cls: String(d.context?.class ?? 'unknown'),
  }
}

/** The short line: `Jev (shadow): complete 0.93 · agrees`, or why there is no answer. */
export function line(j: Judged): string {
  const head = `Jev (${j.mode})`
  if (j.outcome === 'skipped') return `${head}: skipped · ${j.why}`
  if (j.outcome === 'failed') return `${head}: failed · ${j.why}`
  const what = j.lean === undefined ? 'no answer' : `${j.lean} ${(j.value ?? 0).toFixed(2)}`
  return `${head}: ${what} · ${j.drift ? 'drift, not acted on' : j.disagrees ? 'disagrees' : 'agrees'}`
}

/** The `q` quantile of `xs` (nearest rank), or null when there are none. */
export function quantile(xs: number[], q: number): number | null {
  if (!xs.length) return null
  const s = [...xs].sort((a, b) => a - b)
  return s[Math.min(s.length - 1, Math.max(0, Math.ceil(q * s.length) - 1))]
}

export interface ClassStats { cls: string; calls: number; p50: number | null; p95: number | null }

export interface PackStats {
  pack: string
  version: number
  calls: number
  answered: number
  failed: number
  skipped: number
  disagrees: number
  costMicros: number
  /** Latency by workload class, over the judgments that reached Jev. */
  classes: ClassStats[]
}

/** Each pack's numbers over `js` (the judgments read), packs by name, classes by name. */
export function packStats(js: Judged[]): PackStats[] {
  const by = new Map<string, Judged[]>()
  for (const j of js) by.set(j.pack, [...(by.get(j.pack) ?? []), j])
  return [...by.entries()].sort(([a], [b]) => a.localeCompare(b)).map(([pack, xs]) => {
    const reached = xs.filter((j) => j.outcome !== 'skipped')
    const classes = new Map<string, number[]>()
    for (const j of reached) classes.set(j.cls, [...(classes.get(j.cls) ?? []), j.totalMs])
    return {
      pack,
      version: Math.max(...xs.map((j) => j.version)),
      calls: xs.length,
      answered: xs.filter((j) => j.outcome === 'answered').length,
      failed: xs.filter((j) => j.outcome === 'failed').length,
      skipped: xs.filter((j) => j.outcome === 'skipped').length,
      disagrees: xs.filter((j) => j.disagrees).length,
      costMicros: xs.reduce((s, j) => s + (j.costMicros ?? 0), 0),
      classes: [...classes.entries()].sort(([a], [b]) => a.localeCompare(b))
        .map(([cls, t]) => ({ cls, calls: t.length, p50: quantile(t, 0.5), p95: quantile(t, 0.95) })),
    }
  })
}

/** One bar of an answer: its label, its probability, and whether it is the answer's lean. */
export interface Bar { label: string; p: number; chosen: boolean }

/** An answer's probabilities as bars: a Choice's options, a Score's levels, a Noul's yes and no. */
export function bars(a: D): Bar[] {
  const ans = (a.answer ?? {}) as D
  switch (ans.type) {
    case 'choice':
      return ((ans.probabilities ?? []) as [string, number][]).map(([label, p]) => ({ label, p: Number(p), chosen: label === ans.choice }))
    case 'score': {
      const top = a.band?.top?.value
      return ((ans.probabilities ?? []) as number[]).map((p, i) => ({ label: `level ${i}`, p: Number(p), chosen: top === i }))
    }
    case 'noul': {
      const p = Number(ans.noul ?? 0)
      return [{ label: 'yes', p, chosen: p >= 0.5 }, { label: 'no', p: 1 - p, chosen: p < 0.5 }]
    }
  }
  return []
}

/** A turn's trace marks: each judgment its turn dispatched, with the loop it judged. */
export interface Mark { judgment: string; pack: string; point: string; mode: string; loop: number | null; at: number }

export function marksOf(trace: { name: string; kind: string; start_us: number; attrs?: unknown; children?: unknown[] } | null | undefined): Mark[] {
  const out: Mark[] = []
  const walk = (s: any) => {
    if (s.name === 'judge' && s.kind === 'mark') {
      const a = (s.attrs ?? {}) as D
      out.push({
        judgment: String(a.judgment ?? ''), pack: String(a.pack ?? ''), point: String(a.point ?? ''), mode: String(a.mode ?? ''),
        loop: typeof a.loop === 'number' ? a.loop : null, at: s.start_us,
      })
    }
    for (const c of s.children ?? []) walk(c)
  }
  if (trace) walk(trace)
  return out
}
