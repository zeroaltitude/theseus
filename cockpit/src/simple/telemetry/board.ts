// The telemetry board's numbers (issue 17): what each pane of the one board says, worked out from what the Ship's own
// reads already return (`health`, `session.list`, `execution.list`, `confirm.list`) and the ledger's `provider.call`
// rows, which `lib/calls.ts` already reads. Pure, so `npm test` runs it; the panes draw these and their table views
// list the same rows.
import type { ExecutionInfo, Health, LedgerEntry } from '@protocol'
import type { ProviderCall } from '../../lib/calls.ts'
import { totalIn } from '../../lib/calls.ts'
import { ledgerKind } from '../../lib/taxonomy.ts'
import { quantile } from '../../lib/viz.ts'

// The quantile is the Ship's own (`lib/viz.ts`), so a typical or slow figure here is the figure on the Bridge and Economics.
export { quantile }

export const RANGES = ['30m', '6h', '24h', '7d', 'all'] as const
export type Range = (typeof RANGES)[number]
const RANGE_MS: Record<Exclude<Range, 'all'>, number> = { '30m': 30 * 60_000, '6h': 6 * 3600_000, '24h': 24 * 3600_000, '7d': 7 * 86400_000 }
export const RANGE_WORDS: Record<Range, string> = { '30m': 'the last 30 minutes', '6h': 'the last 6 hours', '24h': 'the last 24 hours', '7d': 'the last 7 days', all: 'all of the record' }

/** The window a range names, as its first millisecond (`all` reaches back to the first row). */
export function rangeStart(range: Range, now: number, rows: LedgerEntry[]): number {
  return range === 'all' ? Math.min(now - 60_000, rows[0]?.at_unix_ms ?? now) : now - RANGE_MS[range]
}

/** The smallest window with at least 25 rows in it, as the Bridge chooses: a busy Theseus opens on its last half hour,
 *  an idle one on its history. */
export function autoRange(rows: LedgerEntry[], now: number): Range {
  for (const r of ['30m', '6h', '24h', '7d'] as const) {
    let n = 0
    for (let i = rows.length - 1; i >= 0 && n < 25; i--) if (rows[i].at_unix_ms > now - RANGE_MS[r]) n++
    if (n >= 25) return r
  }
  return 'all'
}

// ---------------------------------------------------------------- what is running

export interface RunningFact { key: string; label: string; n: number; note: string; tone?: 'live' | 'wait' | 'ok' | 'fault' }

const SETTLED = ['succeeded', 'failed', 'cancelled', 'denied', 'settled']

/** What is running now, from the kernel's counts: the same numbers the Bridge's tiles show. */
export function runningOf(h: Health | undefined, executions: ExecutionInfo[], approvals: number): RunningFact[] {
  const k = h?.kernel
  const states = k?.executions_by_state ?? {}
  const running = states.running ?? 0
  const others = Object.entries(states).filter(([s, n]) => s !== 'running' && n > 0).map(([s, n]) => `${n} ${s}`).join(' · ')
  const flight = Object.entries(k?.actions_by_state ?? {}).filter(([s]) => !SETTLED.includes(s)).reduce((a, [, n]) => a + n, 0)
  const wakes = h?.wakes?.length ?? 0
  return [
    { key: 'running', label: 'Working now', n: running, note: others || 'nothing else', tone: running ? 'live' : undefined },
    { key: 'tools', label: 'Tool calls in flight', n: flight, note: `${k?.actions_by_state?.succeeded ?? 0} done · ${k?.actions_by_state?.failed ?? 0} failed` },
    { key: 'approvals', label: 'Approvals waiting', n: approvals, note: approvals ? 'something is stopped until you answer' : 'nothing waits', tone: approvals ? 'wait' : 'ok' },
    { key: 'held', label: 'Turns held', n: k?.turns_held ?? 0, note: `of ${k?.admission_ceiling ?? '?'} Theseus will run at once` },
    { key: 'wakes', label: 'Wakes due', n: wakes, note: h?.wakes?.[0] ? `next ${h.wakes[0].due_local}` : 'none set', tone: wakes ? 'live' : undefined },
    { key: 'executions', label: 'Executions in all', n: executions.length, note: 'every conversation or task with a run, working or not' },
  ]
}

// ---------------------------------------------------------------- latency

export interface LatencyRow { model: string; calls: number; first50?: number; first95?: number; total50?: number; total95?: number }

/** Each model's time to first token and total time over its calls in range, the slowest median first. */
export function latencyOf(calls: ProviderCall[]): LatencyRow[] {
  const by = new Map<string, ProviderCall[]>()
  for (const c of calls) by.set(c.model, [...(by.get(c.model) ?? []), c])
  const num = (xs: (number | undefined)[]) => xs.filter((x): x is number => typeof x === 'number')
  return [...by.entries()].map(([model, cs]) => {
    const first = num(cs.map((c) => c.first_token_ms)), total = num(cs.map((c) => c.total_ms))
    return { model, calls: cs.length, first50: quantile(first, 0.5), first95: quantile(first, 0.95), total50: quantile(total, 0.5), total95: quantile(total, 0.95) }
  }).sort((a, b) => (b.total50 ?? 0) - (a.total50 ?? 0) || a.model.localeCompare(b.model))
}

// ---------------------------------------------------------------- rate-limit headroom

export interface Gauge { label: string; remaining: number; limit: number; fraction: number }
export interface HeadroomRow { provider: string; at: number; gauges: Gauge[]; fraction: number | undefined; word: string }

/** The word for a share left: plenty above half, low to 15 per cent, then nearly out. */
export function headroomWord(f: number): string { return f > 0.5 ? 'plenty left' : f > 0.15 ? 'running low' : 'nearly out' }

/** Each provider's headroom from its newest call that reported limits; the tightest provider first. */
export function headroomOf(calls: ProviderCall[]): HeadroomRow[] {
  const latest = new Map<string, ProviderCall>()
  for (const c of calls) if (c.rate) latest.set(c.provider, c)
  return [...latest.values()].map((c) => {
    const r = c.rate!
    const gauges: Gauge[] = ([['requests', r.requests_remaining, r.requests_limit], ['tokens', r.tokens_remaining, r.tokens_limit]] as const)
      .flatMap(([label, remaining, limit]) => (remaining !== undefined && limit ? [{ label, remaining, limit, fraction: Math.max(0, Math.min(1, remaining / limit)) }] : []))
    const fraction = gauges.length ? Math.min(...gauges.map((g) => g.fraction)) : undefined
    return { provider: c.provider, at: c.at, gauges, fraction, word: fraction === undefined ? 'this provider sends no limits' : headroomWord(fraction) }
  }).sort((a, b) => (a.fraction ?? 2) - (b.fraction ?? 2) || a.provider.localeCompare(b.provider))
}

// ---------------------------------------------------------------- spend

export interface SpendRow { model: string; calls: number; tokens: number; cost: number }
export interface Spend { total: number; hour: number; today: number; byModel: SpendRow[] }

/** Spend: the range's by model, and the running windows (the last hour, since local midnight) over every call given. */
export function spendOf(all: ProviderCall[], inRange: ProviderCall[], now: number): Spend {
  const midnight = new Date(now); midnight.setHours(0, 0, 0, 0)
  const sum = (cs: ProviderCall[]) => cs.reduce((a, c) => a + c.cost, 0)
  const by = new Map<string, SpendRow>()
  for (const c of inRange) {
    const r = by.get(c.model) ?? { model: c.model, calls: 0, tokens: 0, cost: 0 }
    r.calls++; r.tokens += totalIn(c.usage) + c.usage.output_tokens; r.cost += c.cost
    by.set(c.model, r)
  }
  return {
    total: sum(inRange), hour: sum(all.filter((c) => c.at > now - 3600_000)), today: sum(all.filter((c) => c.at >= midnight.getTime())),
    byModel: [...by.values()].sort((a, b) => b.cost - a.cost || a.model.localeCompare(b.model)),
  }
}

// ---------------------------------------------------------------- failures

export interface FailureKind { kind: string; n: number; last: number }

/** The ledger's fault rows in range (the cockpit's own reading of a kind: failed, denied, refused, error and the like),
 *  counted by kind, the most frequent first, and the newest ones whole. */
export function failuresOf(rows: LedgerEntry[]): { kinds: FailureKind[]; recent: LedgerEntry[]; total: number } {
  const faults = rows.filter((r) => ledgerKind(r.kind).tone === 'fault')
  const by = new Map<string, FailureKind>()
  for (const r of faults) {
    const k = by.get(r.kind) ?? { kind: r.kind, n: 0, last: 0 }
    k.n++; k.last = Math.max(k.last, r.at_unix_ms)
    by.set(r.kind, k)
  }
  return { kinds: [...by.values()].sort((a, b) => b.n - a.n || b.last - a.last), recent: faults.slice(-12).reverse(), total: faults.length }
}

// ---------------------------------------------------------------- the machine

export interface MachineFact { key: string; label: string; value: string; state: 'ok' | 'wait' | 'fault'; fraction?: number }

/** The machine's own condition, in the words a person reads: what Theseus reports of its disk, memory, store, secrets and
 *  configuration, each with a state. A fact Theseus did not report is not listed. */
export function machineOf(h: Health | undefined, fmt: { bytes: (n: number) => string; uptime: (s: number) => string }): MachineFact[] {
  if (!h) return []
  const out: MachineFact[] = []
  out.push({ key: 'up', label: 'Running for', value: fmt.uptime(h.uptime_secs), state: 'ok' })
  out.push({ key: 'accepting', label: 'Taking new work', value: h.kernel.accepting ? 'yes' : 'no, held', state: h.kernel.accepting ? 'ok' : 'wait' })
  if (h.disk) {
    const free = h.disk.total_mb ? h.disk.free_mb / h.disk.total_mb : undefined
    const state = h.disk.free_mb <= h.disk.floor_mb ? 'fault' : h.disk.free_mb <= h.disk.warn_mb ? 'wait' : 'ok'
    out.push({ key: 'disk', label: 'Disk free', value: `${(h.disk.free_mb / 1024).toFixed(1)} GB of ${(h.disk.total_mb / 1024).toFixed(1)} GB`, state, fraction: free })
  }
  if (h.resident) out.push({ key: 'memory', label: 'Memory in use', value: fmt.bytes(h.resident.rss_bytes), state: 'ok' })
  if (h.store) {
    const refused = h.store.refused_records
    out.push({ key: 'store', label: 'Store', value: refused ? `${refused} records refused` : 'every record accepted', state: refused ? 'fault' : 'ok' })
  }
  if (h.secrets) {
    const failed = h.secrets.failed.length
    out.push({ key: 'secrets', label: 'Secrets', value: failed ? `${failed} did not resolve` : `${h.secrets.ready.length} ready`, state: failed ? 'fault' : h.secrets.resolving.length ? 'wait' : 'ok' })
  }
  if (h.config) out.push({ key: 'config', label: 'Configuration', value: h.config.state, state: h.config.state === 'held' ? 'wait' : 'ok' })
  out.push({ key: 'ledger', label: 'Ledger rows', value: h.ledger_rows.toLocaleString('en-US'), state: 'ok' })
  out.push({ key: 'errors', label: 'Model errors since start', value: String(h.provider_errors), state: h.provider_errors ? 'wait' : 'ok' })
  return out
}
