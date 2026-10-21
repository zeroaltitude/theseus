// The one progress line (contract C1, theseus-ypnxt.2): the same words every surface shows, here from a `WorkView`.
// `paused · step 3 of 5 · $0.40 of ~$0.60 (limit $2) · ~6 min left · on sonnet`, each part left out when its data is
// absent; the dollars read against the forecast's median (`$0.40 of $2` without one), and the last part is for a task a
// checkpoint moved off its base. Pure: the view and
// the clock in, one string out. test/progressLine.test.ts runs the Rust golden
// (`crates/theseus-protocol/src/work/golden/progress_line.json`) and the Rust function's edges.
import type { WorkView } from '@protocol'

/** Dollars as `theseus_protocol::push::usd` says them: `$10`, `$10.02`, and a fraction of a cent to four places. */
export function lineUsd(x: number): string {
  if (x > 0 && x < 0.01) return `$${x.toFixed(4)}`
  const cents = Math.round(x * 100)
  return cents % 100 === 0 ? `$${cents / 100}` : `$${(cents / 100).toFixed(2)}`
}

/** Minutes left as the Rust line says them (`left` in progress_line.rs): `<1 min`, `6 min`, `2 h`. */
function left(minutes: number): string {
  const m = Math.round(minutes)
  if (m === 0) return '<1 min'
  return m < 60 ? `${m} min` : `${Math.round(minutes / 60)} h`
}

const ENDED: readonly string[] = ['done', 'failed', 'cancelled', 'expired']

/** The time part: what is left of the usual time (the countdown runs to the median), or that it is past, by the clock,
 *  the spend or the forecast's own flag; nothing for an ended task, one with no known start, or a forecast made from no
 *  finished task. */
function timeLeft(v: WorkView, nowMs: number): string | undefined {
  const f = v.forecast
  if (!f || f.n === 0 || ENDED.includes(v.state) || !v.started_at_ms) return undefined
  const elapsed = Math.max(0, nowMs - v.started_at_ms) / 60_000
  const toP50 = f.min_p50 - elapsed
  if (f.past || toP50 <= 0 || (v.cost?.spent_usd ?? 0) > f.usd_p50) return 'past its usual time'
  return `~${left(toP50)} left`
}

/** The dollars part (`cost_part` in progress_line.rs): the spend against the median of the tasks like it and the limit
 *  after it, or the spend against the limit when the forecast is made from no finished task. */
function costPart(v: WorkView): string | undefined {
  const c = v.cost
  if (!c) return undefined
  const f = v.forecast
  if (!f || f.n === 0) return `${lineUsd(c.spent_usd)} of ${lineUsd(c.limit_usd)}`
  const spent = c.spent_usd > f.usd_p50 ? `${lineUsd(c.spent_usd)}, past its usual` : `${lineUsd(c.spent_usd)} of`
  return `${spent} ~${lineUsd(f.usd_p50)} (limit ${lineUsd(c.limit_usd)})`
}

export function progressLine(v: WorkView, nowMs: number): string | undefined {
  const p = v.progress
  const parts = [
    v.paused ? 'paused' : undefined,
    p && p.steps_total > 0 ? `step ${Math.min(p.steps_done + 1, p.steps_total)} of ${p.steps_total}` : undefined,
    costPart(v),
    timeLeft(v, nowMs),
    v.moved_to ? `on ${v.moved_to}` : undefined,
  ].filter((s): s is string => s !== undefined)
  return parts.length ? parts.join(' · ') : undefined
}
