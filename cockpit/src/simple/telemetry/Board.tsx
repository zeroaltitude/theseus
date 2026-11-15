// Telemetry, the board: the instruments that matter most, one pane each, in the order an operator reads them. What is
// running, what has gone wrong, how fast the model answers, how much room the providers have left, what it costs, and
// the machine itself. Every pane has its "?" and its table view (the same numbers as rows, `?table=<id>`); the Ship's
// own instrument pages are the other tabs, one click behind. Nothing here is its own measurement: the numbers are
// `board.ts`'s reading of what `health`, `session.list`, `execution.list`, `confirm.list` and the ledger already say,
// so a figure here is the figure on the Ship's page.
import { useDeferredValue, useMemo } from 'react'
import { useSearchParams } from 'react-router'
import { AlertTriangle, Clock, Coins, Cpu, Gauge, Rocket } from 'lucide-react'
import type { ConfirmRequest, ExecutionInfo, Health, LedgerEntry, SessionInfo } from '@protocol'
import { ChartPanel, type TableSpec } from '@/components/ChartPanel'
import { Segmented } from '@/components/ui'
import { providerCalls } from '@/lib/derive'
import { ago, bytes, ms, pct, stamp, tokens, uptime, usd } from '@/lib/format'
import { useHistoryRows } from '@/lib/history'
import { useRpc } from '@/lib/rpc'
import { summarize } from '@/lib/summary'
import { titleOf } from '@/components/instrumentTables'
import { useTick } from '@/lib/hooks'
import { Stat } from '../kit'
import {
  RANGES, RANGE_WORDS, autoRange, failuresOf, headroomOf, latencyOf, machineOf, rangeStart, runningOf, spendOf,
  type FailureKind, type HeadroomRow, type LatencyRow, type MachineFact, type Range, type RunningFact, type SpendRow,
} from './board'

const EMPTY: LedgerEntry[] = []

/** A bar of a share, in a state's colour. The words beside it say the state, so colour is never the only signal. */
function Meter({ fraction, tone = 'live' }: { fraction: number; tone?: string }) {
  return (
    <div className="h-1.5 w-full rounded-full" style={{ background: 'color-mix(in srgb, var(--color-ink-faint) 25%, transparent)' }}>
      <div className="h-full rounded-full" style={{ width: `${Math.max(0, Math.min(1, fraction)) * 100}%`, background: `var(--color-${tone})` }} />
    </div>
  )
}

export default function TelemetryBoard() {
  const { data: h } = useRpc<Health>('health', undefined, 2000)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 2000)
  const { data: el } = useRpc<{ executions: ExecutionInfo[] }>('execution.list', undefined, 2000)
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 2000)
  const history = useHistoryRows()
  const rows = useDeferredValue(history.ready || history.rows.length ? history.rows : EMPTY)
  const now = useTick(5000)
  const [params, setParams] = useSearchParams()
  const picked = RANGES.find((r) => r === params.get('range'))
  const range: Range = picked ?? (rows.length ? autoRange(rows, now) : '30m')
  const setRange = (r: Range) => setParams((p) => { p.set('range', r); return p }, { replace: true })
  const start = rangeStart(range, now, rows)
  const inRange = useMemo(() => rows.filter((r) => r.at_unix_ms >= start), [rows, start])
  const calls = useMemo(() => providerCalls(rows), [rows])
  const callsIn = useMemo(() => calls.filter((c) => c.at >= start), [calls, start])
  const title = useMemo(() => titleOf(sl?.sessions ?? []), [sl])

  const running = useMemo(() => runningOf(h, el?.executions ?? [], cl?.confirms.length ?? 0), [h, el, cl])
  const failures = useMemo(() => failuresOf(inRange), [inRange])
  const latency = useMemo(() => latencyOf(callsIn), [callsIn])
  const headroom = useMemo(() => headroomOf(calls), [calls])
  const spend = useMemo(() => spendOf(calls, callsIn, now), [calls, callsIn, now])
  const machine = useMemo(() => machineOf(h, { bytes, uptime }), [h])

  return (
    <div className="flex flex-col gap-3">
      <div className="flex flex-wrap items-center gap-x-3 gap-y-1" title="the failures, latency, spend and the table views read this range; what is running, the headroom and the machine are as of now">
        <span className="simple-eyebrow">Failures, speed and spend over</span>
        <Segmented value={range} options={RANGES} onChange={setRange} />
        <span className="text-[12px] text-ink-faint">
          {RANGE_WORDS[range]}{picked ? '' : ' (chosen for you: the smallest window with 25 rows)'} · {inRange.length.toLocaleString('en-US')} of {rows.length.toLocaleString('en-US')} ledger rows
          {!history.ready && ' read so far'}
        </span>
      </div>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <RunningPane facts={running} className="xl:col-span-2" />
        <FailuresPane failures={failures} errors={h?.provider_errors ?? 0} title={title} />
      </div>
      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2 xl:grid-cols-3">
        <LatencyPane rows={latency} n={callsIn.length} />
        <HeadroomPane rows={headroom} now={now} />
        <SpendPane spend={spend} limit={h?.kernel.spend_limit_usd} total={h?.cost_usd_total} />
      </div>
      <MachinePane facts={machine} />
    </div>
  )
}

// ---------------------------------------------------------------- what is running

function RunningPane({ facts, className }: { facts: RunningFact[]; className?: string }) {
  const table: TableSpec<RunningFact> = {
    caption: 'what is running now, from the kernel’s own counts', rows: facts, rowKey: (r) => r.key,
    columns: [
      { key: 'what', label: 'what', cell: (r) => r.label },
      { key: 'n', label: 'now', num: true, cell: (r) => r.n },
      { key: 'note', label: 'and', cell: (r) => r.note },
    ],
  }
  return (
    <ChartPanel id="t-running" help="simple.telemetry.running" title="What is running now" icon={<Rocket size={13} />} className={className} height={190} table={table}>
      <div className="grid h-full grid-cols-2 content-start gap-2 overflow-auto md:grid-cols-3">
        {facts.map((f) => <Stat key={f.key} n={f.n} label={f.label} tone={f.tone} title={f.note} />)}
      </div>
    </ChartPanel>
  )
}

// ---------------------------------------------------------------- failures

function FailuresPane({ failures, errors, title }: { failures: ReturnType<typeof failuresOf>; errors: number; title: (sid: string | null | undefined) => string }) {
  const table: TableSpec<LedgerEntry> = {
    caption: 'the newest failures in this range, whole', rows: failures.recent, rowKey: (r) => String(r.position),
    columns: [
      { key: 'at', label: 'when', cell: (r) => stamp(r.at_unix_ms) },
      { key: 'kind', label: 'what', cell: (r) => r.kind },
      { key: 'session', label: 'in', cell: (r) => title(r.session_id), title: (r) => title(r.session_id) },
      { key: 'words', label: 'says', cell: (r) => summarize(r), title: (r) => summarize(r), className: 'max-w-[34ch] truncate' },
    ],
  }
  const errorWords = errors ? ` · ${errors} model ${errors === 1 ? 'error' : 'errors'} since Theseus started` : ''
  return (
    <ChartPanel id="t-failures" help="simple.telemetry.failures" title="What went wrong" icon={<AlertTriangle size={13} />} height={190} table={table}
      empty={failures.total ? undefined : `no failures in this range${errorWords}`}>
      <div className="flex h-full flex-col gap-1 overflow-auto">
        <div className="text-[13px] text-ink"><span className="num font-semibold" style={{ color: 'var(--color-fault)' }}>{failures.total}</span> failed, in {failures.kinds.length} {failures.kinds.length === 1 ? 'kind' : 'kinds'}{errorWords}</div>
        {failures.kinds.slice(0, 6).map((k: FailureKind) => (
          <div key={k.kind} className="flex items-baseline gap-2 text-[12.5px]">
            <span className="num w-8 text-right font-semibold text-ink">{k.n}</span>
            <span className="min-w-0 flex-1 truncate text-ink-dim" title={k.kind}>{k.kind}</span>
            <span className="text-[11.5px] text-ink-faint">{ago(k.last)}</span>
          </div>
        ))}
      </div>
    </ChartPanel>
  )
}

// ---------------------------------------------------------------- latency

function LatencyPane({ rows, n }: { rows: LatencyRow[]; n: number }) {
  const slowest = Math.max(1, ...rows.map((r) => r.total50 ?? 0))
  const table: TableSpec<LatencyRow> = {
    caption: 'how long each model took over the calls in this range', rows, rowKey: (r) => r.model,
    columns: [
      { key: 'model', label: 'model', cell: (r) => r.model },
      { key: 'calls', label: 'calls', num: true, cell: (r) => r.calls },
      { key: 'f50', label: 'first word, typical', num: true, cell: (r) => ms(r.first50) },
      { key: 'f95', label: 'first word, slow', num: true, cell: (r) => ms(r.first95) },
      { key: 't50', label: 'whole answer, typical', num: true, cell: (r) => ms(r.total50) },
      { key: 't95', label: 'whole answer, slow', num: true, cell: (r) => ms(r.total95) },
    ],
  }
  return (
    <ChartPanel id="t-latency" help="simple.telemetry.latency" title="How fast it answers" icon={<Clock size={13} />} height={230} table={table}
      empty={rows.length ? undefined : 'no model calls in this range'}>
      <div className="flex h-full flex-col gap-2.5 overflow-auto px-1">
        <div className="text-[12px] text-ink-faint">{n} model {n === 1 ? 'call' : 'calls'}. Typical is the middle call; slow is the one in twenty that took longest.</div>
        {rows.map((r) => (
          <div key={r.model}>
            <div className="flex items-baseline justify-between gap-2 text-[12.5px]">
              <span className="min-w-0 truncate font-semibold text-ink" title={r.model}>{r.model}</span>
              <span className="num shrink-0 text-ink-dim">{ms(r.first50)} to the first word · {ms(r.total50)} in all</span>
            </div>
            <Meter fraction={(r.total50 ?? 0) / slowest} tone="live" />
            <div className="num mt-0.5 text-[11.5px] text-ink-faint">slow: {ms(r.first95)} to the first word · {ms(r.total95)} in all · {r.calls} {r.calls === 1 ? 'call' : 'calls'}</div>
          </div>
        ))}
      </div>
    </ChartPanel>
  )
}

// ---------------------------------------------------------------- headroom

function HeadroomPane({ rows, now }: { rows: HeadroomRow[]; now: number }) {
  const left = (r: HeadroomRow, label: string) => {
    const g = r.gauges.find((x) => x.label === label)
    return g ? `${tokens(g.remaining)} of ${tokens(g.limit)} (${pct(g.fraction, 1)})` : '—'
  }
  const table: TableSpec<HeadroomRow> = {
    caption: 'each provider’s room left, from the headers of its newest call', rows, rowKey: (r) => r.provider,
    columns: [
      { key: 'provider', label: 'provider', cell: (r) => r.provider },
      { key: 'at', label: 'as of', cell: (r) => stamp(r.at) },
      { key: 'requests', label: 'requests left', num: true, cell: (r) => left(r, 'requests') },
      { key: 'tokens', label: 'tokens left', num: true, cell: (r) => left(r, 'tokens') },
      { key: 'word', label: 'so', cell: (r) => r.word },
    ],
  }
  return (
    <ChartPanel id="t-headroom" help="simple.telemetry.headroom" title="Room left with each provider" icon={<Gauge size={13} />} height={230} table={table}
      empty={rows.length ? undefined : 'no model call has reported its limits yet'}>
      <div className="flex h-full flex-col gap-3 overflow-auto px-1">
        {rows.map((r) => (
          <div key={r.provider}>
            <div className="flex items-baseline justify-between text-[12.5px]">
              <span className="font-semibold text-ink">{r.provider}</span>
              <span className="text-[11.5px] text-ink-faint">{r.word} · as of {ago(r.at, now)}</span>
            </div>
            {r.gauges.map((g) => (
              <div key={g.label} className="mt-1">
                <div className="flex justify-between text-[11.5px] text-ink-faint"><span>{g.label}</span><span className="num text-ink-dim">{tokens(g.remaining)} of {tokens(g.limit)} ({pct(g.fraction, 1)})</span></div>
                <Meter fraction={g.fraction} tone={g.fraction > 0.5 ? 'live' : g.fraction > 0.15 ? 'wait' : 'fault'} />
              </div>
            ))}
          </div>
        ))}
      </div>
    </ChartPanel>
  )
}

// ---------------------------------------------------------------- spend

function SpendPane({ spend, limit, total }: { spend: ReturnType<typeof spendOf>; limit?: number; total?: number }) {
  const top = Math.max(0.0001, ...spend.byModel.map((r) => r.cost))
  const table: TableSpec<SpendRow> = {
    caption: 'the spend by model over this range', rows: spend.byModel, rowKey: (r) => r.model,
    columns: [
      { key: 'model', label: 'model', cell: (r) => r.model },
      { key: 'calls', label: 'calls', num: true, cell: (r) => r.calls },
      { key: 'tokens', label: 'tokens', num: true, cell: (r) => tokens(r.tokens) },
      { key: 'cost', label: 'spent', num: true, cell: (r) => usd(r.cost) },
    ],
  }
  return (
    <ChartPanel id="t-spend" help="simple.telemetry.spend" title="What it costs" icon={<Coins size={13} />} height={230} table={table}
      empty={spend.byModel.length ? undefined : 'no model calls in this range'}>
      <div className="flex h-full flex-col gap-2 overflow-auto px-1">
        <div className="grid grid-cols-3 gap-2">
          <Stat n={usd(spend.today)} label="today" tone="money" />
          <Stat n={usd(spend.hour)} label="last hour" tone="money" />
          <Stat n={usd(spend.total)} label="in this range" tone="money" />
        </div>
        <div className="text-[11.5px] text-ink-faint">{total !== undefined ? `${usd(total)} since Theseus began` : ''}{limit ? ` · each conversation stops for your answer at ${usd(limit)}` : ''}</div>
        {spend.byModel.slice(0, 5).map((r) => (
          <div key={r.model}>
            <div className="flex items-baseline justify-between gap-2 text-[12.5px]"><span className="min-w-0 truncate text-ink" title={r.model}>{r.model}</span><span className="num text-ink-dim">{usd(r.cost)}</span></div>
            <Meter fraction={r.cost / top} tone="money" />
          </div>
        ))}
      </div>
    </ChartPanel>
  )
}

// ---------------------------------------------------------------- the machine

function MachinePane({ facts }: { facts: MachineFact[] }) {
  const table: TableSpec<MachineFact> = {
    caption: 'the machine, as Theseus reports it', rows: facts, rowKey: (r) => r.key,
    columns: [
      { key: 'what', label: 'what', cell: (r) => r.label },
      { key: 'value', label: 'now', cell: (r) => r.value },
      { key: 'state', label: 'state', cell: (r) => (r.state === 'ok' ? 'fine' : r.state === 'wait' ? 'worth a look' : 'wrong') },
    ],
  }
  return (
    <ChartPanel id="t-machine" help="simple.telemetry.machine" title="The machine" icon={<Cpu size={13} />} height={140} table={table}
      empty={facts.length ? undefined : 'reading health…'}>
      <div className="grid h-full grid-cols-1 content-start gap-x-6 gap-y-2 overflow-auto px-1 md:grid-cols-2 xl:grid-cols-3">
        {facts.map((f) => (
          <div key={f.key} className="text-[12.5px]">
            <div className="flex items-baseline justify-between gap-2">
              <span className="text-ink-faint">{f.label}</span>
              <span className="num text-ink" style={f.state === 'ok' ? undefined : { color: `var(--color-${f.state})` }}>{f.value}{f.state === 'ok' ? '' : f.state === 'wait' ? ' · worth a look' : ' · wrong'}</span>
            </div>
            {f.fraction !== undefined && <Meter fraction={f.fraction} tone={f.state === 'ok' ? 'live' : f.state} />}
          </div>
        ))}
      </div>
    </ChartPanel>
  )
}
