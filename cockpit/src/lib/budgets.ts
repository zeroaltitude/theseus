// Money in the cockpit (M7 42b): `budget.list`'s tree flattened, its totals worked out again from the top rows (a
// task's spend is its parent's too, so adding every row would count it twice), the burn per hour over a window of
// the history's `provider.call` rows, and the recent resets from its `budget.reset` rows. Pure, so `npm test` runs it.
import type { AwsHandsStatus, BudgetRow, DayCeilingBudget, LedgerEntry } from '@protocol'

export const HOUR_MS = 3_600_000
/** The window the burn is measured over: the last hour. */
export const BURN_WINDOW_MS = HOUR_MS

/** A row with its depth under the top row and its parent's session. */
export interface Flat { row: BudgetRow; depth: number; parent?: string }

/** The tree in reading order: each top row, then its tasks under it. */
export function flatten(rows: readonly BudgetRow[]): Flat[] {
  const out: Flat[] = []
  const walk = (r: BudgetRow, depth: number, parent?: string) => {
    out.push({ row: r, depth, parent })
    for (const t of r.tasks ?? []) walk(t, depth + 1, r.session_id)
  }
  for (const r of rows) walk(r, 0)
  return out
}

/** The rows whose budget question waits for the operator, in reading order: Money shows them above its river while any
 *  waits (theseus-v6vc). */
export function questionsWaiting(rows: readonly BudgetRow[]): Flat[] {
  return flatten(rows).filter((f) => f.row.question)
}

export interface Sums { limit: number; spent: number; reserved: number; heldUnknown: number; available: number; lifetime: number }

/** The totals, as the daemon states its rule: the money figures add the top rows only (a task's spend is its parent's
 *  too, and its carve its parent's reservation); the lifetime adds every row's, since each session counts its own. */
export function sums(rows: readonly BudgetRow[]): Sums {
  const s: Sums = { limit: 0, spent: 0, reserved: 0, heldUnknown: 0, available: 0, lifetime: 0 }
  for (const r of rows) {
    s.limit += r.limit_usd
    s.spent += r.spent_usd
    s.reserved += r.reserved_usd
    s.heldUnknown += r.held_unknown_usd
    s.available += r.available_usd
  }
  for (const f of flatten(rows)) s.lifetime += f.row.lifetime_usd
  return s
}

/** A call's cost and when, whichever view read it: what the burn needs. */
export interface Spend { at: number; cost: number; session_id: string | null }

/** Dollars an hour, over the `windowMs` that ends at `end`: what the calls of the sessions in it cost, scaled to an
 *  hour. A call at the window's start is outside it, one at its end inside. */
export function burnPerHour(calls: readonly Spend[], sessions: ReadonlySet<string>, end: number, windowMs = BURN_WINDOW_MS): number {
  let sum = 0
  for (const c of calls) if (c.session_id && sessions.has(c.session_id) && c.at > end - windowMs && c.at <= end) sum += c.cost
  return (sum * HOUR_MS) / windowMs
}

/** The sessions a row spends from: its own, and its tasks'. */
export function sessionsOf(r: BudgetRow): Set<string> {
  return new Set(flatten([r]).map((f) => f.row.session_id))
}

/** `budget.reset` rows, newest first: when, who approved, and what the spend was before. */
export interface ResetRow { at: number; session: string; execution?: string; by: string; before: number }

export function recentResets(rows: readonly LedgerEntry[], n = 8, asOf: number | null = null): ResetRow[] {
  const out: ResetRow[] = []
  for (let i = rows.length - 1; i >= 0 && out.length < n; i--) {
    const r = rows[i]
    if (r.kind !== 'budget.reset' || !r.session_id || (asOf !== null && r.at_unix_ms > asOf)) continue
    const d = (r.data ?? {}) as { execution_id?: string; by?: string; spent_before_usd?: number }
    out.push({ at: r.at_unix_ms, session: r.session_id, execution: d.execution_id, by: d.by ?? '?', before: Number(d.spent_before_usd ?? 0) })
  }
  return out
}

/** Where a limit comes from, in words. */
export function limitWords(r: Pick<BudgetRow, 'limit_from' | 'limit_by'>): string {
  switch (r.limit_from) {
    case 'config': return 'the config’s limit'
    case 'place': return `the place’s ceiling${r.limit_by ? ` (${r.limit_by})` : ''}`
    case 'carve': return `carved from its parent${r.limit_by ? ` (${r.limit_by.slice(0, 12)})` : ''}`
    case 'pinned': return 'pinned when it opened'
    default: return r.limit_from
  }
}

/** The AWS hands' lines, from health: what runs, the hour's meter against its line, and runaway mode said plainly. */
export function handsLines(h: AwsHandsStatus, now: number): { text: string; tone: 'ok' | 'wait' | 'fault' }[] {
  const usd = (m: number) => `$${(m / 1e6).toFixed(2)}`
  const out: { text: string; tone: 'ok' | 'wait' | 'fault' }[] = []
  if (h.runaway && (h.runaway_until_unix_ms === undefined || h.runaway_until_unix_ms > now)) {
    out.push({ text: `runaway mode: ${h.runaway}${h.runaway_until_unix_ms ? `; new AWS actions that reserve are refused until ${new Date(h.runaway_until_unix_ms).toISOString().slice(11, 16)} UTC` : ''}`, tone: 'fault' })
  }
  const running = h.running_lambda + h.running_fargate
  out.push({ text: `${running} hand${running === 1 ? '' : 's'} running (${h.running_lambda} Lambda, ${h.running_fargate} Fargate), ${usd(h.reserved_micros)} reserved`, tone: 'ok' })
  const past = h.alerted_hour_unix_ms !== undefined
  out.push({ text: `this hour ${usd(h.hour_micros)} of its ${usd(h.hour_line_micros)} line${past ? ': past it, alerted' : ''}`, tone: past ? 'wait' : 'ok' })
  if (h.reaper_failures > 0) out.push({ text: `TTL reaper failed ${h.reaper_failures}×${h.reaper_last_failure ? `: ${h.reaper_last_failure}` : ''}`, tone: 'fault' })
  return out
}

/** The daemon's day ceiling (`[kernel] daily_spend_ceiling_usd`, theseus-kp20) as the Budgets panel shows it: today's
 *  model spend against the ceiling, a quiet bar while under it, and a clear stop once reached, since no model call is
 *  made until the day turns. `share` is the bar's fill, 0 to 1 (what calls in flight hold counts toward it). */
export function dayCeilingView(d: DayCeilingBudget): { text: string; detail: string; share: number; tone: 'ok' | 'wait' | 'fault'; stopped: boolean } {
  const usd = (n: number) => `$${n.toFixed(2)}`
  const used = d.spent_usd + d.held_usd
  const share = d.ceiling_usd > 0 ? Math.min(1, Math.max(0, used / d.ceiling_usd)) : 0
  const held = d.held_usd > 0 ? `, ${usd(d.held_usd)} held by calls in flight` : ''
  if (d.reached) {
    return {
      text: `stopped: today’s model spend reached the ${usd(d.ceiling_usd)} daily ceiling`,
      detail: `${usd(d.spent_usd)} spent on ${d.day}${held} · no model call until ${d.turns_at} local time · raise [kernel] daily_spend_ceiling_usd to go on sooner`,
      share: 1,
      tone: 'fault',
      stopped: true,
    }
  }
  return {
    text: `${usd(d.spent_usd)} of ${usd(d.ceiling_usd)} today`,
    detail: `${d.day}${held} · the day turns at ${d.turns_at} local time`,
    share,
    tone: share >= 0.8 ? 'wait' : 'ok',
    stopped: false,
  }
}
