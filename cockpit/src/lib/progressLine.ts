// The one progress line (contract C1, theseus-ypnxt.2): the same words every surface shows, here from a `WorkView`.
// `paused · step 3 of 5 · $0.40 of $2 · ~6 min left`, each part left out when its data is absent. Pure: the view and
// the clock in, one string out. Tested by test/progressLine.test.ts; if it disagrees with the Rust golden
// (`crates/theseus-protocol/src/work/golden/progress_line.json`), the golden wins.
import type { WorkView } from '@protocol'

/** Dollars as `theseus_protocol::push::usd` says them: `$10`, `$10.02`, and a fraction of a cent to four places. */
export function lineUsd(x: number): string {
  if (x > 0 && x < 0.01) return `$${x.toFixed(4)}`
  const cents = Math.round(x * 100)
  return cents % 100 === 0 ? `$${cents / 100}` : `$${(cents / 100).toFixed(2)}`
}

/** A span of minutes as the line rounds it: `~6 min`, `~2 h`. */
function coarse(min: number): string {
  const m = Math.max(1, Math.round(min))
  return m < 60 ? `~${m} min` : `~${Math.round(m / 60)} h`
}

/** The time part: what is left of the usual time (the median while it lasts, then the 90th percentile), or that it is
 *  past; nothing without a forecast made from finished tasks. */
function timeLeft(v: WorkView, nowMs: number): string | undefined {
  const f = v.forecast
  if (!f || f.n === 0) return undefined
  const elapsed = v.started_at_ms > 0 ? Math.max(0, nowMs - v.started_at_ms) / 60_000 : 0
  const left = f.min_p50 - elapsed > 0 ? f.min_p50 - elapsed : f.min_p90 - elapsed
  return f.past || left <= 0 ? 'past its usual time' : `${coarse(left)} left`
}

export function progressLine(v: WorkView, nowMs: number): string | undefined {
  const p = v.progress
  const parts = [
    v.paused ? 'paused' : undefined,
    p && p.steps_total > 0 ? `step ${Math.min(p.steps_done + 1, p.steps_total)} of ${p.steps_total}` : undefined,
    v.cost ? `${lineUsd(v.cost.spent_usd)} of ${lineUsd(v.cost.limit_usd)}` : undefined,
    timeLeft(v, nowMs),
  ].filter((s): s is string => s !== undefined)
  return parts.length ? parts.join(' · ') : undefined
}
