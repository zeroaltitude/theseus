// "Your work" (own-benchmark, theseus-pw1q.12): the Benchmarks view's row for the owner's own turns, from
// `route.report`. Pure: the words of a cell and the totals, so the panel and its test read the same sentences.
// The numbers are the daemon's; nothing is computed here but sentences and shares.

import type { RouteCell } from '../protocol.gen/RouteCell.ts'
import type { RouteReportResult } from '../protocol.gen/RouteReportResult.ts'

const usd = (v: number): string => (v !== 0 && Math.abs(v) < 0.01 ? `$${v.toFixed(4)}` : `$${v.toFixed(2)}`)
const pct = (v: number): string => `${Math.round(v * 100)}%`

/** A cell's effort as the owner reads it: an empty one is the profile's own. */
export const effortWord = (e: string): string => (e === '' ? 'default' : e)

/** What a cell says of moving one rung down, in a sentence; or why it says nothing yet. A cell whose whole turns were
 * replayed (tools and all, theseus-pw1q.20) says those too, and how many had the same tests. */
export function verdictWords(c: RouteCell, r: Pick<RouteReportResult, 'bar' | 'confidence'>): string {
  const whole = c.whole_replayed > 0 ? `; whole turns: ${c.whole_matched} of ${c.whole_replayed} matched, ${c.tests_same} with the same tests` : ''
  if (!c.rung_down) return `not replayed yet${whole}`
  if (c.replayed === 0) return `replays on ${c.rung_down} not judged yet${whole}`
  const floor = c.match_floor === undefined ? '' : `; at ${pct(r.confidence)} confidence at least ${pct(c.match_floor)} match`
  return `${c.matched} of ${c.replayed} replayed on ${c.rung_down} matched (same or better at ${pct(r.bar)}+)${floor}${whole}`
}

/** The cells worth a row, dearest first (the daemon sorts them): every cell, since a cell with nothing replayed is
 * the one the sampler has yet to reach. */
export function rows(r: RouteReportResult): { cell: RouteCell; words: string; share: number }[] {
  const top = Math.max(0, ...r.cells.map((c) => c.spend_usd))
  return r.cells.map((cell) => ({ cell, words: verdictWords(cell, r), share: top > 0 ? cell.spend_usd / top : 0 }))
}

/** The line under the table: the movable dollars, and the replays' cost against the day's cap. */
export function footer(r: RouteReportResult): string {
  const cap = r.sampler ? `replays today ${usd(r.replay_today_usd)} (${r.sampler})` : `replays today ${usd(r.replay_today_usd)} of ${usd(r.replay_day_usd)}`
  const fed = r.labels_fed > 0 ? `; ${r.labels_fed} machine labels fed to routing's learning (train split only)` : ''
  return `movable at ${pct(r.confidence)} confidence: ${usd(r.total_movable_usd)} of ${usd(r.total_spend_usd)}; ${cap}${fed}`
}

export const money = usd
