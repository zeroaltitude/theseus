// The Bridge: the whole harness at a glance. Everything here is live; everything clicks through.
//
// The charts follow the chart method (`lib/viz.ts`, theseus-hnof): each one's form by its job, the method's axes and
// marks, a model in its slot's colour (the same as on Economics), a state's colour only with a shape and a word, the
// figures in the ink, a tooltip built as DOM, and a table of the same numbers behind each panel's toggle. One range, in
// the address (`?range=`), scopes every chart under it; the tiles above it each say what they count. The rows are the
// page's one copy of the ledger (`useHistoryRows`): the Bridge keeps no ledger loop of its own, only one tail read while
// that copy is still being read, so it opens on the newest rows.
import { useDeferredValue, useMemo } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import {
  Activity, Bot, Brain, Clock, Coins, Flame, Gauge, Hourglass, Radar, Rocket, ShieldCheck, Siren, Timer, Wrench,
} from 'lucide-react'
import { ContextRanks, Startup, TokenMix, ToolBoard, TurnsChart } from '@/components/instruments'
import { STARTUP_LEGEND, TOKEN_LEGEND, TURNS_LEGEND, contextTable, startupTable, titleOf, tokenMixTable, turnsTable } from '@/components/instrumentTables'
import { NowStrip } from '@/components/NowStrip'
import type { ConfirmRequest, ExecutionInfo, Health, LedgerEntry, SessionInfo } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { providerCalls, spendCurve, totalIn, turnRows, useLedger, type ProviderCall, type RateLimit } from '@/lib/derive'
import { useHistoryRows } from '@/lib/history'
import { ago, clock, ms, pct, short, stamp, tokens, usd } from '@/lib/format'
import { ledgerKind, stateTone, type Tone } from '@/lib/taxonomy'
import { Echart } from '@/components/Echart'
import type { EChartsOption } from '@/lib/chart'
import {
  CATEGORICAL, CHROME, FONTS, MARK, OTHER, TIME_LABELS, TIP_FRAME, TONE_MARK, barRadius, baseAxis, binByKey, countTick, msLogTick,
  quantile, slots, spendTree, valueAxis, type Bins,
} from '@/lib/viz'
import { tip, type TipRow } from '@/lib/viztip'
import { ChartPanel, StatTile, Swatch, TipArea, TipBody, TipTarget, type LegendItem, type TableSpec } from '@/components/ChartPanel'
import { useWidth } from '@/lib/chartview'
import { AttentionPill, Panel, Pill, Segmented } from '@/components/ui'
import { useHistory, useTick } from '@/lib/hooks'

const C = CHROME.dark
const ACCENT = CATEGORICAL.dark[0]

const RANGES = ['30m', '6h', '24h', '7d', 'all'] as const
type Range = (typeof RANGES)[number]
const RANGE_MS: Record<Exclude<Range, 'all'>, number> = { '30m': 30 * 60_000, '6h': 6 * 3600_000, '24h': 24 * 3600_000, '7d': 7 * 86400_000 }
const RANGE_WORDS: Record<Range, string> = { '30m': 'the last 30 minutes', '6h': 'the last 6 hours', '24h': 'the last 24 hours', '7d': 'the last 7 days', all: 'all of the record' }

/** The record read so far with the newest rows after it, each once: what the Bridge draws while the whole is still read. */
function withNewest(rows: LedgerEntry[], newest: LedgerEntry[] | undefined): LedgerEntry[] {
  if (!newest?.length) return rows
  const last = rows.length ? rows[rows.length - 1].position : -1
  const after = newest.filter((r) => r.position > last)
  return after.length ? rows.concat(after) : rows
}

/** The smallest window with enough to see: a busy daemon opens on its last half hour, an idle one on its history. */
function autoRange(rows: LedgerEntry[], now: number): Range {
  for (const r of ['30m', '6h', '24h', '7d'] as const) {
    let n = 0
    for (let i = rows.length - 1; i >= 0 && n < 25; i--) if (rows[i].at_unix_ms > now - RANGE_MS[r]) n++
    if (n >= 25) return r
  }
  return 'all'
}

export function Bridge() {
  const nav = useNavigate()
  const { data: h } = useRpc<Health>('health', undefined, 2000)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 2000)
  const { data: el } = useRpc<{ executions: ExecutionInfo[] }>('execution.list', undefined, 2000)
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 2000)
  // The whole ledger, shared and followed; deferred, so a burst of rows never holds up a click. The record is read oldest
  // first, so until it is whole one tail read brings the newest rows, and the Bridge opens on now; then that read stops.
  const history = useHistoryRows()
  const { data: tail } = useLedger(1000, 2000, undefined, undefined, !history.ready)
  const merged = useMemo(() => withNewest(history.rows, history.ready ? undefined : tail?.rows), [history.rows, history.ready, tail])
  const rows = useDeferredValue(merged)
  const calls = useMemo(() => providerCalls(rows), [rows])
  const sessions = useMemo(() => sl?.sessions ?? [], [sl])
  const title = useMemo(() => titleOf(sessions), [sessions])

  const k = h?.kernel
  const exStates = k?.executions_by_state ?? {}
  const running = exStates.running ?? 0
  const inFlight = Object.entries(k?.actions_by_state ?? {})
    .filter(([s]) => !['succeeded', 'failed', 'cancelled', 'denied', 'settled'].includes(s))
    .reduce((a, [, n]) => a + n, 0)
  const confirms = cl?.confirms ?? []
  const usage = h?.usage_total
  const cacheRate = usage ? usage.cache_read_input_tokens / Math.max(1, totalIn(usage)) : 0
  const now = useTick(5000)
  const lastHour = useMemo(() => calls.filter((c) => c.at > now - 3600_000), [calls, now])
  const tokHour = lastHour.reduce((a, c) => a + totalIn(c.usage) + c.usage.output_tokens, 0)
  // The last hour's tokens in twelve five-minute bins: the tile's sparkline.
  const tokSpark = useMemo(() => {
    const bins = new Array<number>(12).fill(0)
    for (const c of lastHour) bins[Math.min(11, Math.floor((c.at - (now - 3600_000)) / 300_000))] += totalIn(c.usage) + c.usage.output_tokens
    return bins
  }, [lastHour, now])
  const spendHist = useMemo(() => { const v = spendCurve(calls).map(([, x]) => x); const step = Math.max(1, Math.ceil(v.length / 40)); return v.filter((_, i) => i % step === 0 || i === v.length - 1) }, [calls])
  const runHist = useHistory(running)
  const flightHist = useHistory(inFlight)

  // The range every chart below reads, in the address (?range=); unset, the smallest window with enough to see.
  const [params, setParams] = useSearchParams()
  const picked = RANGES.find((r) => r === params.get('range'))
  const range = picked ?? (rows.length ? autoRange(rows, now) : '30m')
  const setRange = (r: Range) => setParams((p) => { p.set('range', r); return p }, { replace: true })
  const start = range === 'all' ? Math.min(now - 60_000, rows[0]?.at_unix_ms ?? now) : now - RANGE_MS[range]
  const span = useMemo(() => [start, now] as const, [start, now])
  const inRange = useMemo(() => rows.filter((r) => r.at_unix_ms >= start), [rows, start])
  const callsIn = useMemo(() => calls.filter((c) => c.at >= start), [calls, start])
  const turns = useMemo(() => turnRows(inRange), [inRange])
  // A model's colour is its slot over the whole record, in the order it first names the models: Economics' colours.
  const modelSlot = useMemo(() => slots(calls.map((c) => c.model)).slot, [calls])

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4 2xl:grid-cols-8">
        <StatTile label="Executions running" icon={<Rocket size={12} />} value={running} format={(n) => n.toFixed(0)} tone={running ? 'live' : undefined}
          hint={Object.entries(exStates).filter(([s]) => s !== 'running').map(([s, n]) => `${n} ${s}`).join(' · ') || 'none other'}
          spark={runHist} onClick={() => nav('/fleet')} title="the executions running now, and the others by state; a click opens the fleet" />
        <StatTile label="Tools in flight" icon={<Wrench size={12} />} value={inFlight} format={(n) => n.toFixed(0)} tone={inFlight ? 'tool' : undefined}
          hint={`${k?.actions_by_state?.succeeded ?? 0} succeeded · ${k?.actions_by_state?.failed ?? 0} failed`} spark={flightHist} onClick={() => nav('/actions')}
          title="tool calls planned and not yet settled; a click opens the actions" />
        <StatTile label="Spent" icon={<Coins size={12} />} value={h?.cost_usd_total ?? 0} format={(n) => usd(n)} tone="money"
          hint={`limit per session ${usd(k?.spend_limit_usd)}`} spark={spendHist} onClick={() => nav('/economics')}
          title="every session's spend, and its running total over the record; a click opens Economics" />
        <StatTile label="Tokens · last hour" icon={<Bot size={12} />} value={tokHour} format={tokens} tone="model"
          hint={`${lastHour.length} model calls`} spark={tokSpark} onClick={() => nav('/economics')}
          title="tokens in and out of the last hour's model calls, in five-minute bins; a click opens Economics" />
        <StatTile label="Cache hit" icon={<Brain size={12} />} value={cacheRate} format={(n) => pct(n, 1)} tone="think"
          hint={usage ? `${tokens(usage.cache_read_input_tokens)} read from cache` : undefined} onClick={() => nav('/economics')} />
        <StatTile label="Approvals waiting" icon={<ShieldCheck size={12} />} value={confirms.length} format={(n) => n.toFixed(0)}
          tone={confirms.length ? 'wait' : 'ok'} hint={confirms[0] ? `${confirms[0].tool} · ${ago(confirms[0].requested_at_ms)}` : 'nothing waits'} onClick={() => nav('/actions')} />
        <StatTile label="External-text holds" icon={<Siren size={12} />} value={h?.external_text?.length ?? 0} format={(n) => n.toFixed(0)}
          tone={h?.external_text?.length ? 'wait' : 'ok'} hint={h?.external_text?.[0] ? `${h.external_text[0].held.tool} ${ago(h.external_text[0].held.since_ms)}` : 'every session trusted'}
          onClick={() => nav('/boundaries')} />
        <StatTile label="Wakes due" icon={<Hourglass size={12} />} value={h?.wakes?.length ?? 0} format={(n) => n.toFixed(0)} tone={h?.wakes?.length ? 'live' : undefined}
          hint={h?.wakes?.[0] ? `next ${h.wakes[0].due_local}` : 'none set'} />
      </div>

      <NowStrip sessions={sessions} executions={el?.executions ?? []} />

      <div className="flex flex-wrap items-center gap-x-3 gap-y-1 px-1" title="every chart below reads this range; the tiles above say what each counts">
        <span className="ship-engraved text-[9.5px]">the charts below</span>
        <Segmented value={range} options={RANGES} onChange={setRange} />
        <span className="num text-[11px] text-ink-faint">
          {RANGE_WORDS[range]}{picked ? '' : ' (chosen for you: the smallest window with 25 rows)'} · {inRange.length.toLocaleString()} of {rows.length.toLocaleString()} ledger rows read
          {!history.ready && ` · still reading the record (${history.rows.length.toLocaleString()} of ${history.total ? history.total.toLocaleString() : '…'}), the newest rows first`}
        </span>
      </div>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <PulsePanel rows={inRange} span={span} ready={history.ready || rows.length > 0} />
        <FleetPanel sessions={sessions} executions={el?.executions ?? []} />
      </div>

      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2 2xl:grid-cols-4">
        <FuelPanel calls={calls} />
        <ChartPanel id="latency" title="Latency · first token and total" icon={<Clock size={13} />} height={250}
          legend={latencyLegend(callsIn, modelSlot)} empty={callsIn.length ? undefined : 'no model calls in this range'} table={latencyTable(callsIn, title)}>
          <div className="flex h-full flex-col">
            <div className="num px-2 pt-0.5 text-[11px] text-ink-dim">{latencyLine(callsIn)}</div>
            <div className="min-h-0 flex-auto"><Latency calls={callsIn} slot={modelSlot} title={title} span={span} /></div>
          </div>
        </ChartPanel>
        <ChartPanel id="flow" title="Spend · provider → model → session" icon={<Flame size={13} />} height={FLOW_H}
          empty={callsIn.some((c) => c.cost > 0) ? undefined : 'no spend in this range'} table={flowTable(callsIn, title)}>
          <SpendFlow calls={callsIn} slot={modelSlot} title={title} onPick={(sid) => nav(`/session/${sid}`)} />
        </ChartPanel>
        <ChartPanel id="start" title="Last start · its phases" icon={<Rocket size={13} />} height={250}
          legend={STARTUP_LEGEND} empty={h ? undefined : 'reading health…'} table={startupTable(h?.startup ?? [])}>
          <Startup phases={h?.startup ?? []} />
        </ChartPanel>
      </div>

      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2 2xl:grid-cols-4">
        <ChartPanel id="turns" title="Turns · duration, loops, and cost" icon={<Timer size={13} />} height={260}
          legend={TURNS_LEGEND} empty={turns.some((t) => t.elapsed_ms !== undefined) ? undefined : 'no finished turns in this range'} table={turnsTable(turns, title)}>
          <TurnsChart turns={turns} title={title} span={span} onPick={(sid) => nav(`/session/${sid}`)} />
        </ChartPanel>
        <Panel title="Tools · calls, failures, and time" icon={<Wrench size={13} />} bodyClassName="h-[260px]">
          <ToolBoard rows={inRange} />
        </Panel>
        <ChartPanel id="context" title="Context · prompt size, largest first" icon={<Brain size={13} />} height={260}
          empty={inRange.some((r) => r.kind === 'context.compiled' || r.kind === 'context.recompiled') ? undefined : 'no context compiles in this range'}
          table={contextTable(inRange, title)}>
          <ContextRanks rows={inRange} sessions={sessions} onPick={(sid) => nav(`/session/${sid}?tab=context`)} />
        </ChartPanel>
        <ChartPanel id="mix" title="Token mix · the newest 48 calls" icon={<Bot size={13} />} height={260}
          legend={TOKEN_LEGEND} empty={callsIn.length ? undefined : 'no model calls in this range'} table={tokenMixTable(callsIn, title)}>
          <TokenMix calls={callsIn} title={title} />
        </ChartPanel>
      </div>
    </div>
  )
}

// ---------------------------------------------------------------- the pulse

/** The ledger's nine families, the agent's own work first. Nine is past the method's ceiling for colours that must tell
 *  series apart, and as stacked neighbours the tones collide (turns and tools are 5 apart for any reader): so the pulse
 *  draws each family on its own row, named where it is, and a row's colour (its tone's step for marks) only echoes the
 *  cockpit's language. */
const FAMILIES: { tone: Tone; word: string }[] = [
  { tone: 'live', word: 'turns & executions' }, { tone: 'model', word: 'model' }, { tone: 'tool', word: 'tools' },
  { tone: 'think', word: 'context' }, { tone: 'wait', word: 'policy & sessions' }, { tone: 'ok', word: 'discord' },
  { tone: 'money', word: 'budget' }, { tone: 'fault', word: 'faults' }, { tone: 'idle', word: 'other' },
]
const PULSE_BINS = 60
const PULSE_H = 236

/** The span's bins, in words: the time, with the day when the span crosses one. */
function binWords(b: Bins, i: number): string {
  const long = b.ends[b.ends.length - 1] - b.starts[0] > 20 * 3600_000
  const day = (t: number) => new Date(t).toLocaleDateString([], { month: 'short', day: 'numeric' })
  return long ? `${day(b.starts[i])} ${clock(b.starts[i]).slice(0, 5)} – ${day(b.ends[i])} ${clock(b.ends[i]).slice(0, 5)}` : `${clock(b.starts[i])} – ${clock(b.ends[i])}`
}

function PulsePanel({ rows, span, ready }: { rows: LedgerEntry[]; span: readonly [number, number]; ready: boolean }) {
  const bins = useMemo(() => binByKey(rows, (r) => r.at_unix_ms, (r) => ledgerKind(r.kind).tone, FAMILIES.map((f) => f.tone), span[0], span[1], PULSE_BINS), [rows, span])
  const present = FAMILIES.filter((f) => bins.counts[f.tone].some((n) => n > 0))
  return (
    <ChartPanel id="pulse" title="Pulse · ledger events, a row per family" icon={<Activity size={13} />} className="xl:col-span-2" height={PULSE_H}
      empty={!ready ? 'reading the ledger…' : rows.length ? undefined : 'no ledger rows in this range'} table={pulseTable(bins, present)}>
      <Pulse bins={bins} families={present} />
    </ChartPanel>
  )
}

/** The pulse as small multiples: a row per family with its name and its rows in range said beside it, each row's
 *  columns on its own scale (a quiet family's rhythm reads as well as a busy one's), every row on the one time axis. The
 *  crosshair runs through all of them, and its tip lists every family in the bin with the bin's total. */
function Pulse({ bins, families }: { bins: Bins; families: { tone: Tone; word: string }[] }) {
  const option = useMemo<EChartsOption>(() => {
    const mids = bins.starts.map((s, i) => (s + bins.ends[i]) / 2)
    const n = Math.max(1, families.length)
    const top = 6, axisBand = 22
    const rowH = Math.min(40, Math.floor((PULSE_H - 16 - top - axisBand) / n))
    const axis = baseAxis()
    const total = (t: Tone) => bins.counts[t].reduce((a, b) => a + b, 0)
    return {
      grid: families.map((_, i) => ({ left: 156, right: 16, top: top + i * rowH, height: rowH - 5 })),
      title: families.map((f, i) => ({
        text: `{w|${f.word}}  {n|${countTick(total(f.tone))}}`, left: 6, top: top + i * rowH + (rowH - 5) / 2 - 7,
        textStyle: { rich: { w: { color: C.secondary, fontSize: 11, fontFamily: FONTS.sans }, n: { color: C.muted, fontSize: 10, fontFamily: FONTS.mono } } },
      })),
      axisPointer: { link: [{ xAxisIndex: 'all' }] },
      tooltip: {
        ...TIP_FRAME, trigger: 'axis', axisPointer: { type: 'line', lineStyle: { color: C.axis, width: 1, type: 'solid' } },
        formatter: (ps: any) => {
          const i = (Array.isArray(ps) ? ps[0] : ps)?.dataIndex
          if (i === undefined || bins.starts[i] === undefined) return ''
          const rows: TipRow[] = families.filter((f) => bins.counts[f.tone][i] > 0)
            .map((f) => ({ value: countTick(bins.counts[f.tone][i]), label: f.word, color: TONE_MARK[f.tone], mark: 'rect' }))
          rows.push({ value: countTick(bins.totals[i]), label: bins.totals[i] === 1 ? 'row in all' : 'rows in all' })
          return tip(binWords(bins, i), rows)
        },
      },
      xAxis: families.map((_, i) => ({
        type: 'time' as const, gridIndex: i, min: bins.starts[0], max: bins.ends[bins.ends.length - 1], ...axis,
        axisLabel: { ...axis.axisLabel, show: i === families.length - 1, formatter: TIME_LABELS },
      })),
      yAxis: families.map((f, i) => ({ type: 'value' as const, gridIndex: i, min: 0, max: Math.max(1, ...bins.counts[f.tone]), show: false })),
      series: families.map((f, i) => ({
        name: f.word, type: 'bar' as const, xAxisIndex: i, yAxisIndex: i, barWidth: '70%', barMaxWidth: MARK.bar,
        itemStyle: { color: TONE_MARK[f.tone], borderRadius: barRadius(false, 2) },
        data: mids.map((m, j) => [m, bins.counts[f.tone][j]]),
      })),
    }
  }, [bins, families])
  return <Echart key={families.map((f) => f.tone).join('|')} option={option} />
}

function pulseTable(b: Bins, families: { tone: Tone; word: string }[]): TableSpec<number> {
  const rows = b.starts.map((_, i) => i).filter((i) => b.totals[i] > 0).reverse()
  return {
    caption: 'ledger rows a bin, by family, the newest first', rows, rowKey: (i: number) => String(b.starts[i]),
    columns: [
      { key: 'when', label: 'when', cell: (i: number) => binWords(b, i) },
      ...families.map((f) => ({ key: f.tone, label: f.word, num: true, cell: (i: number) => countTick(b.counts[f.tone][i]) })),
      { key: 'all', label: 'in all', num: true, cell: (i: number) => countTick(b.totals[i]) },
    ],
  }
}

// ---------------------------------------------------------------- the fleet

/** The executions by state, as one bar of their shares (each state's tone in its step, a 2 px gap between them) over
 *  their words and counts, then the six sessions active last. */
function FleetPanel({ sessions, executions }: { sessions: SessionInfo[]; executions: ExecutionInfo[] }) {
  const nav = useNavigate()
  const now = useTick(5000)
  const byState = useMemo(() => {
    const m = new Map<string, number>()
    for (const e of executions) m.set(e.state, (m.get(e.state) ?? 0) + 1)
    return [...m.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
  }, [executions])
  const recent = useMemo(() => [...sessions].sort((a, b) => (b.last_active_ms ?? 0) - (a.last_active_ms ?? 0)), [sessions])
  const total = executions.length
  return (
    <ChartPanel id="fleet" title="Fleet" icon={<Radar size={13} />} height={230} table={fleetTable(recent, now)}
      empty={sessions.length || executions.length ? undefined : 'no sessions yet'}>
      <div className="flex h-full flex-col">
        <TipArea className="border-b border-line px-1.5 pb-2 pt-1">
          <div className="flex h-3 w-full gap-[2px]" role="group" aria-label="the executions by state">
            {byState.map(([s, n], i) => (
              <TipTarget key={s} label={`${s}: ${n}`} className="viz-mark h-full min-w-[2px]"
                style={{ flex: `${n} 1 0`, background: TONE_MARK[stateTone(s)], borderRadius: i === byState.length - 1 ? '0 4px 4px 0' : 0 }}
                tip={<TipBody rows={[{ value: String(n), label: s, color: TONE_MARK[stateTone(s)], mark: 'rect' }]} foot={`${pct(total ? n / total : 0)} of ${total} executions`} />} />
            ))}
          </div>
          <div className="mt-2 flex flex-wrap items-center gap-1.5">
            {byState.map(([s, n]) => <Pill key={s} tone={stateTone(s)}><Swatch color={TONE_MARK[stateTone(s)]} />{s} <span className="num font-semibold">{n}</span></Pill>)}
            <span className="num ml-auto text-[11px] text-ink-faint">{total} executions · {sessions.length} sessions</span>
          </div>
        </TipArea>
        <div className="min-h-0 flex-1 overflow-auto">
          {recent.slice(0, 6).map((s) => (
            <button key={s.session_id} onClick={() => nav(`/session/${s.session_id}`)}
              className="flex w-full items-center gap-2 border-b border-line/60 px-2 py-1.5 text-left text-[12px] hover:bg-white/[0.03]">
              <AttentionPill a={s.attention} state={s.execution_state} />
              <span className="min-w-0 flex-1 truncate text-ink">{s.title || s.label || short(s.session_id)}</span>
              <span className="num text-[11px] text-ink-faint">{s.turns}t</span>
              <span className="num w-14 text-right text-[11px] text-ink">{usd(s.cost_usd)}</span>
              <span className="num w-14 text-right text-[11px] text-ink-faint">{ago(s.last_active_ms, now)}</span>
            </button>
          ))}
        </div>
      </div>
    </ChartPanel>
  )
}

function fleetTable(sessions: SessionInfo[], now: number): TableSpec<SessionInfo> {
  type R = SessionInfo
  return {
    caption: 'every session, the latest active first', rows: sessions, rowKey: (r: R) => r.session_id,
    columns: [
      { key: 'state', label: 'state', cell: (r: R) => r.attention?.label ?? r.execution_state ?? 'idle', title: (r: R) => r.attention?.label ?? undefined, className: 'max-w-[16ch]' },
      { key: 'title', label: 'session', cell: (r: R) => r.title || r.label || short(r.session_id), title: (r: R) => r.title ?? undefined },
      { key: 'turns', label: 'turns', num: true, cell: (r: R) => r.turns },
      { key: 'cost', label: 'cost', num: true, cell: (r: R) => usd(r.cost_usd) },
      { key: 'active', label: 'active', num: true, cell: (r: R) => ago(r.last_active_ms, now) },
    ],
  }
}

// ---------------------------------------------------------------- fuel

/** Each provider's rate-limit headroom, from its newest call's headers: requests and tokens left of their limits, as a
 *  meter whose fill is the accent while there is room, then the wait and fault tones' steps with their words. */
function FuelPanel({ calls }: { calls: ProviderCall[] }) {
  const latest = useMemo(() => {
    const m = new Map<string, ProviderCall>()
    for (const c of calls) if (c.rate) m.set(c.provider, c)
    return [...m.values()]
  }, [calls])
  return (
    <ChartPanel id="fuel" title="Fuel · rate-limit headroom" icon={<Gauge size={13} />} height={250}
      empty={latest.length ? undefined : 'no model call has reported its limits yet'} table={fuelTable(latest)}>
      <div className="flex h-full flex-col gap-3 overflow-auto px-2 py-1">
        {latest.map((c) => {
          const r = c.rate!
          const gauges: [string, number | undefined, number | undefined][] = [['requests', r.requests_remaining, r.requests_limit], ['tokens', r.tokens_remaining, r.tokens_limit]]
          return (
            <div key={c.provider}>
              <div className="mb-1 flex items-baseline justify-between">
                <span className="text-[12px] font-semibold text-ink">{c.provider}</span>
                <span className="num text-[10px] text-ink-faint">as of {ago(c.at)}</span>
              </div>
              {gauges.map(([label, rem, lim]) => rem !== undefined && lim ? <Headroom key={label} label={label} rem={rem} lim={lim} /> : null)}
              {r.input_tokens_remaining != null && r.output_tokens_remaining != null && (
                <div className="num text-[11px] text-ink-faint">input left {tokens(r.input_tokens_remaining)} · output left {tokens(r.output_tokens_remaining)}</div>
              )}
              {gauges.every(([, rem, lim]) => rem == null || !lim) && r.input_tokens_remaining == null && (
                <div className="text-[11px] text-ink-faint">this provider sends no rate-limit headers</div>
              )}
            </div>
          )
        })}
      </div>
    </ChartPanel>
  )
}

function Headroom({ label, rem, lim }: { label: string; rem: number; lim: number }) {
  const f = Math.max(0, Math.min(1, rem / lim))
  const [color, word] = f > 0.5 ? [ACCENT, ''] : f > 0.15 ? [TONE_MARK.wait, 'running low'] : [TONE_MARK.fault, 'nearly out']
  return (
    <div className="mb-2">
      <div className="mb-1 flex justify-between text-[11px]">
        <span className="text-ink-faint">{label}{word && <span className="text-ink"> · {word}</span>}</span>
        <span className="num text-ink">{tokens(rem)} / {tokens(lim)} <span className="text-ink-faint">({pct(f, 1)})</span></span>
      </div>
      <div className="h-1.5 w-full rounded-r-[3px]" style={{ background: `${color}33` }}>
        <div className="h-full rounded-r-[3px]" style={{ width: `${f * 100}%`, background: color }} />
      </div>
    </div>
  )
}

function fuelTable(latest: ProviderCall[]): TableSpec<ProviderCall> {
  type R = ProviderCall
  const left = (rem?: number, lim?: number) => (rem !== undefined && lim ? `${tokens(rem)} of ${tokens(lim)} (${pct(rem / lim, 1)})` : '—')
  const rate = (r: R): RateLimit => r.rate ?? {}
  return {
    caption: 'each provider\'s rate-limit headroom, from its newest call', rows: latest, rowKey: (r: R) => r.provider,
    columns: [
      { key: 'provider', label: 'provider', cell: (r: R) => r.provider },
      { key: 'at', label: 'as of', cell: (r: R) => stamp(r.at) },
      { key: 'requests', label: 'requests left', num: true, cell: (r: R) => left(rate(r).requests_remaining, rate(r).requests_limit) },
      { key: 'tokens', label: 'tokens left', num: true, cell: (r: R) => left(rate(r).tokens_remaining, rate(r).tokens_limit) },
      { key: 'in', label: 'input left', num: true, cell: (r: R) => tokens(rate(r).input_tokens_remaining) },
      { key: 'out', label: 'output left', num: true, cell: (r: R) => tokens(rate(r).output_tokens_remaining) },
    ],
  }
}

// ---------------------------------------------------------------- latency

/** A scatter carries three series at most (the method's all-pairs cap): the models in the first three slots keep their
 *  colours, and the rest fold into "other models", in the de-emphasis gray. */
const SCATTER_SLOTS = 3
const modelColor = (slot: Map<string, number>, model: string) => { const i = slot.get(model); return i !== undefined && i < SCATTER_SLOTS ? CATEGORICAL.dark[i] : OTHER }

function latencyLegend(calls: ProviderCall[], slot: Map<string, number>): LegendItem[] {
  const models = [...new Set(calls.map((c) => c.model))].sort((a, b) => (slot.get(a) ?? 99) - (slot.get(b) ?? 99))
  const items: LegendItem[] = models.filter((m) => (slot.get(m) ?? 99) < SCATTER_SLOTS).map((m) => ({ key: m, label: m, color: modelColor(slot, m), mark: 'dot' }))
  if (models.some((m) => (slot.get(m) ?? 99) >= SCATTER_SLOTS)) items.push({ key: 'other', label: 'other models', color: OTHER, mark: 'dot' })
  // Then the two measures, in the neutral: the colour is the model; the shape is the measure.
  return [...items, { key: 'first', label: 'first token', color: C.secondary, mark: 'ring' }, { key: 'total', label: 'total', color: C.secondary, mark: 'dot' }]
}

/** Each model call over time on one log axis of milliseconds: its first token a ring and its total a dot, in its model's
 *  colour; the range's p50 of each a hairline, said in words over the chart. The hover target is wider than the mark. */
function Latency({ calls, slot, title, span }: { calls: ProviderCall[]; slot: Map<string, number>; title: (sid: string | null | undefined) => string; span: readonly [number, number] }) {
  const option = useMemo<EChartsOption>(() => {
    const first = calls.filter((c) => typeof c.first_token_ms === 'number')
    const total = calls.filter((c) => typeof c.total_ms === 'number')
    const f50 = quantile(first.map((c) => c.first_token_ms!), 0.5), t50 = quantile(total.map((c) => c.total_ms!), 0.5)
    const vaxis = valueAxis(), axis = baseAxis()
    const hits = [
      ...first.map((c) => ({ value: [c.at, Math.max(0.01, c.first_token_ms!)], c, which: 'first' as const })),
      ...total.map((c) => ({ value: [c.at, Math.max(0.01, c.total_ms!)], c, which: 'total' as const })),
    ]
    // The p50s are hairlines; their words are the line over the chart (latencyLine), never on the dots.
    const p50 = (v: number | undefined) => (v === undefined ? [] : [{ yAxis: Math.max(0.01, v), lineStyle: { color: C.secondary, opacity: 0.5, width: 1, type: 'solid' as const }, label: { show: false } }])
    return {
      grid: { left: 12, right: 18, top: 12, bottom: 22, containLabel: true },
      tooltip: {
        ...TIP_FRAME, trigger: 'item',
        formatter: (x: any) => {
          const c: ProviderCall | undefined = x.data?.c
          if (!c) return ''
          const color = modelColor(slot, c.model)
          return tip(c.model, [
            { value: ms(c.first_token_ms), label: 'to the first token', color, mark: 'ring' },
            { value: ms(c.total_ms), label: 'in all', color, mark: 'dot' },
          ], `${stamp(c.at)} · ${title(c.session_id)}`)
        },
      },
      xAxis: { type: 'time', min: span[0], max: span[1], ...axis, axisLabel: { ...axis.axisLabel, formatter: TIME_LABELS } },
      yAxis: { ...vaxis, type: 'log', logBase: 10, axisLabel: { ...vaxis.axisLabel, formatter: msLogTick } },
      series: [
        {
          type: 'scatter', name: 'first token', silent: true, symbolSize: MARK.marker + MARK.ring,
          data: first.map((c) => ({ value: [c.at, Math.max(0.01, c.first_token_ms!)], itemStyle: { color: C.surface, borderColor: modelColor(slot, c.model), borderWidth: MARK.ring } })),
        },
        {
          type: 'scatter', name: 'total', silent: true, symbolSize: MARK.marker + MARK.ring,
          data: total.map((c) => ({ value: [c.at, Math.max(0.01, c.total_ms!)], itemStyle: { color: modelColor(slot, c.model), borderColor: C.surface, borderWidth: MARK.ring } })),
        },
        {
          type: 'scatter', name: 'hit', symbolSize: 22, z: 5, itemStyle: { color: 'rgba(0,0,0,0)' }, emphasis: { disabled: true }, data: hits,
          markLine: { silent: true, symbol: 'none', data: [...p50(t50), ...p50(f50)] },
        },
      ],
    }
  }, [calls, slot, title, span])
  return <Echart option={option} />
}

/** The range's p50s, in words: what the two hairlines mark. */
function latencyLine(calls: ProviderCall[]): string {
  const f = quantile(calls.flatMap((c) => (typeof c.first_token_ms === 'number' ? [c.first_token_ms] : [])), 0.5)
  const t = quantile(calls.flatMap((c) => (typeof c.total_ms === 'number' ? [c.total_ms] : [])), 0.5)
  return `p50 first token ${ms(f)} · total ${ms(t)} (the hairlines) · ${calls.length} calls`
}

function latencyTable(calls: ProviderCall[], title: (sid: string | null | undefined) => string): TableSpec<ProviderCall> {
  type R = ProviderCall
  return {
    caption: 'each model call\'s first token and total, the newest first', rows: [...calls].reverse(), rowKey: (r: R) => String(r.position),
    columns: [
      { key: 'at', label: 'when', cell: (r: R) => stamp(r.at) },
      { key: 'model', label: 'model', cell: (r: R) => r.model },
      { key: 'session', label: 'session', cell: (r: R) => title(r.session_id), title: (r: R) => title(r.session_id) },
      { key: 'first', label: 'first token', num: true, cell: (r: R) => ms(r.first_token_ms) },
      { key: 'total', label: 'total', num: true, cell: (r: R) => ms(r.total_ms) },
    ],
  }
}

// ---------------------------------------------------------------- spend flow

/** Text width in the chart's sans at 11 px, measured, so a session's name gets the room it needs, never a guess. */
let measurer: CanvasRenderingContext2D | null = null
function textWidth(s: string): number {
  measurer ??= document.createElement('canvas').getContext('2d')
  if (!measurer) return s.length * 6.2
  measurer.font = `11px ${FONTS.sans}`
  return measurer.measureText(s).width
}

/** The spend as a flow: each provider into its models (in their slots' colours, Economics' too), each model into its
 *  sessions, a link's width its dollars. The sessions' names get the right margin they need, up to two fifths of the
 *  panel, and a name longer than that ends in an ellipsis (never a cut), whole in the tip and the table. */
function SpendFlow({ calls, slot, title, onPick }: { calls: ProviderCall[]; slot: Map<string, number>; title: (sid: string | null | undefined) => string; onPick: (sid: string) => void }) {
  const [ref, width] = useWidth<HTMLDivElement>()
  const tree = useMemo(() => spendTree(calls.filter((c) => c.cost > 0)), [calls])
  const option = useMemo<EChartsOption>(() => {
    const total = tree.reduce((a, p) => a + p.cost, 0)
    const value = new Map<string, number>()
    const links: { source: string; target: string; value: number; lineStyle: { color: string; opacity: number } }[] = []
    const sessionNames = new Map<string, string>()
    for (const p of tree) {
      value.set(`p:${p.key}`, p.cost)
      for (const m of p.children) {
        value.set(`m:${m.key}`, (value.get(`m:${m.key}`) ?? 0) + m.cost)
        const hue = slotHue(slot, m.key)
        links.push({ source: `p:${p.key}`, target: `m:${m.key}`, value: m.cost, lineStyle: { color: hue, opacity: 0.32 } })
        for (const s of m.children) {
          const id = `s:${s.key}`
          value.set(id, (value.get(id) ?? 0) + s.cost)
          sessionNames.set(id, s.key === '—' ? 'no session' : title(s.key))
          links.push({ source: `m:${m.key}`, target: id, value: s.cost, lineStyle: { color: hue, opacity: 0.32 } })
        }
      }
    }
    const room = Math.min(Math.max(...[...sessionNames.values()].map(textWidth), 40) + 14, Math.max(80, (width || 400) * 0.44))
    // A node too thin to hold its name keeps it in the tip and the table, so names never pile up: the sessions' column,
    // the one with the most nodes, sets the scale a dollar is drawn at.
    const gaps = Math.max(0, sessionNames.size - 1) * FLOW_GAP
    const perDollar = total > 0 ? Math.max(0, FLOW_H - 12 - gaps) / total : 0
    const named = (n: string) => !n.startsWith('s:') || (value.get(n) ?? 0) * perDollar >= 12
    const nameOf = (n: string) => (n.startsWith('s:') ? sessionNames.get(n) ?? n.slice(2) : n.slice(2))
    const share = (v: number) => (total > 0 ? ` · ${pct(v / total, 1)} of the spend` : '')
    return {
      tooltip: {
        ...TIP_FRAME, trigger: 'item',
        formatter: (x: any) => {
          if (x.dataType === 'edge') {
            return tip(`${nameOf(x.data.source)} → ${nameOf(x.data.target)}`, [{ value: usd(x.data.value), label: 'spent along this link', color: x.data.lineStyle?.color, mark: 'line' }], share(x.data.value).replace(/^ · /, ''))
          }
          const v = value.get(x.name) ?? 0
          const kind = x.name.startsWith('p:') ? 'a provider' : x.name.startsWith('m:') ? 'a model' : 'a session'
          return tip(kind, [{ value: usd(v), label: nameOf(x.name), color: x.data?.itemStyle?.color, mark: 'rect' }], `${share(v).replace(/^ · /, '')}${x.name.startsWith('s:') && x.name !== 's:—' ? ' · a click opens the session' : ''}`)
        },
      },
      series: [{
        type: 'sankey', left: 4, right: room, top: 6, bottom: 6, nodeWidth: 8, nodeGap: FLOW_GAP, draggable: false, layoutIterations: 32,
        emphasis: { focus: 'adjacency' },
        lineStyle: { curveness: 0.5 },
        label: { color: C.secondary, fontSize: 11, fontFamily: FONTS.sans },
        data: [...value.keys()].map((n) => ({
          name: n,
          itemStyle: { color: n.startsWith('m:') ? slotHue(slot, n.slice(2)) : n.startsWith('p:') ? C.muted : OTHER, borderWidth: 0 },
          label: {
            show: named(n), formatter: () => nameOf(n),
            ...(n.startsWith('s:') ? { width: room - 12, overflow: 'truncate' as const, ellipsis: '…' } : { color: C.text }),
          },
        })),
        links,
      }],
    }
  }, [tree, slot, title, width])
  return <div ref={ref} className="h-full w-full"><Echart option={option} onClick={(x: any) => { if (x?.dataType === 'node' && typeof x.name === 'string' && x.name.startsWith('s:') && x.name !== 's:—') onPick(x.name.slice(2)) }} /></div>
}

const FLOW_H = 250
const FLOW_GAP = 10

/** A model's colour in the flow: its slot (the Sankey's nodes are labelled, so all eight slots are safe), or "other". */
const slotHue = (slot: Map<string, number>, model: string) => { const i = slot.get(model); return i === undefined ? OTHER : CATEGORICAL.dark[i] }

function flowTable(calls: ProviderCall[], title: (sid: string | null | undefined) => string): TableSpec<{ key: string; provider: string; model: string; session: string; calls: number; cost: number }> {
  type R = { key: string; provider: string; model: string; session: string; calls: number; cost: number }
  const rows: R[] = spendTree(calls.filter((c) => c.cost > 0)).flatMap((p) => p.children.flatMap((m) => m.children.map((s) => ({
    key: `${p.key}/${m.key}/${s.key}`, provider: p.key, model: m.key, session: s.key === '—' ? 'no session' : title(s.key), calls: s.calls, cost: s.cost,
  }))))
  return {
    caption: 'the spend by provider, model, and session', rows, rowKey: (r: R) => r.key,
    columns: [
      { key: 'provider', label: 'provider', cell: (r: R) => r.provider },
      { key: 'model', label: 'model', cell: (r: R) => r.model },
      { key: 'session', label: 'session', cell: (r: R) => r.session, title: (r: R) => r.session },
      { key: 'calls', label: 'calls', num: true, cell: (r: R) => r.calls },
      { key: 'cost', label: 'spent', num: true, cell: (r: R) => usd(r.cost) },
    ],
  }
}
