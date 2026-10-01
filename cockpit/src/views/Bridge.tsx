// The Bridge: the whole harness at a glance. Everything here is live; everything clicks through.
import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router'
import {
  Activity, Bot, Brain, Clock, Coins, Flame, Gauge, Hourglass, Radar, Rocket, ShieldCheck, Siren, Timer, Wrench,
} from 'lucide-react'
import { ContextGrowth, Startup, TokenMix, ToolBoard, TurnsChart } from '@/components/instruments'
import { NowStrip } from '@/components/NowStrip'
import type { ConfirmRequest, ExecutionInfo, Health, SessionInfo } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useDerived, pulse, spendCurve, totalIn, type ProviderCall } from '@/lib/derive'
import { ago, ms, pct, short, tokens, usd, clock } from '@/lib/format'
import { stateTone, toneHex, type Tone } from '@/lib/taxonomy'
import { Echart } from '@/components/Echart'
import { axisStyle, type EChartsOption } from '@/lib/chart'
import { Empty, Kpi, Meter, Panel, Pill, Segmented, StatePill } from '@/components/ui'
import { useHistory, useTick } from '@/lib/hooks'

export function Bridge() {
  const nav = useNavigate()
  const { data: h } = useRpc<Health>('health', undefined, 2000)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 2000)
  const { data: el } = useRpc<{ executions: ExecutionInfo[] }>('execution.list', undefined, 2000)
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 2000)
  const { rows, calls } = useDerived(2000)

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
  const lastHour = calls.filter((c) => c.at > now - 3600_000)
  const tokHour = lastHour.reduce((a, c) => a + totalIn(c.usage) + c.usage.output_tokens, 0)

  const spendHist = useMemo(() => spendCurve(calls).map(([, v]) => v), [calls])
  const runHist = useHistory(running)
  const flightHist = useHistory(inFlight)

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-3 md:grid-cols-4 2xl:grid-cols-8">
        <Kpi label="Executions running" icon={<Rocket size={12} />} value={running} format={(n) => n.toFixed(0)} tone="live"
          hint={Object.entries(exStates).filter(([s]) => s !== 'running').map(([s, n]) => `${n} ${s}`).join(' · ') || 'none other'}
          spark={runHist} onClick={() => nav('/fleet')} />
        <Kpi label="Tools in flight" icon={<Wrench size={12} />} value={inFlight} format={(n) => n.toFixed(0)} tone="tool"
          hint={`${k?.actions_by_state?.succeeded ?? 0} succeeded · ${k?.actions_by_state?.failed ?? 0} failed`} spark={flightHist} onClick={() => nav('/actions')} />
        <Kpi label="Spent" icon={<Coins size={12} />} value={h?.cost_usd_total ?? 0} format={(n) => usd(n)} tone="money"
          hint={`limit per session ${usd(k?.spend_limit_usd)}`} spark={spendHist} onClick={() => nav('/economics')} />
        <Kpi label="Tokens · last hour" icon={<Bot size={12} />} value={tokHour} format={tokens} tone="model"
          hint={`${lastHour.length} model calls`} onClick={() => nav('/economics')} />
        <Kpi label="Cache hit" icon={<Brain size={12} />} value={cacheRate} format={(n) => pct(n, 1)} tone="think"
          hint={usage ? `${tokens(usage.cache_read_input_tokens)} read from cache` : undefined} onClick={() => nav('/economics')} />
        <Kpi label="Approvals waiting" icon={<ShieldCheck size={12} />} value={confirms.length} format={(n) => n.toFixed(0)}
          tone={confirms.length ? 'wait' : 'ok'} hint={confirms[0] ? `${confirms[0].tool} · ${ago(confirms[0].requested_at_ms)}` : 'nothing waits'} onClick={() => nav('/actions')} />
        <Kpi label="External-text holds" icon={<Siren size={12} />} value={h?.external_text?.length ?? 0} format={(n) => n.toFixed(0)}
          tone={h?.external_text?.length ? 'wait' : 'ok'} hint={h?.external_text?.[0] ? `${h.external_text[0].held.tool} ${ago(h.external_text[0].held.since_ms)}` : 'every session trusted'} />
        <Kpi label="Wakes due" icon={<Hourglass size={12} />} value={h?.wakes?.length ?? 0} format={(n) => n.toFixed(0)} tone="live"
          hint={h?.wakes?.[0] ? `next ${h.wakes[0].due_local}` : 'none set'} />
      </div>

      <NowStrip sessions={sl?.sessions ?? []} executions={el?.executions ?? []} />

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <PulsePanel rows={rows} />
        <Panel title="Fleet" icon={<Radar size={13} />} bodyClassName="h-[230px] flex flex-col">
          <FleetGlance sessions={sl?.sessions ?? []} executions={el?.executions ?? []} />
        </Panel>
      </div>

      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2 2xl:grid-cols-4">
        <Panel title="Fuel · provider rate-limit headroom" icon={<Gauge size={13} />} bodyClassName="h-[250px] p-2">
          <Fuel calls={calls} />
        </Panel>
        <Panel title="Latency · first token and total, per model call" icon={<Clock size={13} />} bodyClassName="h-[250px] p-2">
          <Latency calls={calls} />
        </Panel>
        <Panel title="Spend flow · provider → model → session" icon={<Flame size={13} />} bodyClassName="h-[250px] p-2">
          <SpendFlow calls={calls} sessions={sl?.sessions ?? []} />
        </Panel>
        <Panel title="Last start · phases from process start" icon={<Rocket size={13} />} bodyClassName="h-[250px] p-2">
          <Startup phases={h?.startup ?? []} />
        </Panel>
      </div>

      <div className="grid grid-cols-1 gap-3 lg:grid-cols-2 2xl:grid-cols-4">
        <Panel title="Turns · duration, loops, and cost" icon={<Timer size={13} />} bodyClassName="h-[260px] p-2">
          <TurnsChart rows={rows} sessions={sl?.sessions ?? []} onPick={(sid) => nav(`/session/${sid}`)} />
        </Panel>
        <Panel title="Tools · calls, failures, and time" icon={<Wrench size={13} />} bodyClassName="h-[260px]">
          <ToolBoard rows={rows} />
        </Panel>
        <Panel title="Context · estimated prompt size per compile" icon={<Brain size={13} />} bodyClassName="h-[260px] p-2">
          <ContextGrowth rows={rows} sessions={sl?.sessions ?? []} />
        </Panel>
        <Panel title="Token mix · per model call" icon={<Bot size={13} />} bodyClassName="h-[260px] p-2">
          <TokenMix calls={calls} />
        </Panel>
      </div>
    </div>
  )
}

const RANGES = ['30m', '6h', '24h', '7d', 'all'] as const
type Range = (typeof RANGES)[number]
const RANGE_MS: Record<Exclude<Range, 'all'>, number> = { '30m': 30 * 60_000, '6h': 6 * 3600_000, '24h': 24 * 3600_000, '7d': 7 * 86400_000 }

/** The smallest window with enough to see: a busy daemon opens on its last half hour, an idle one on its history. */
function autoRange(rows: Parameters<typeof pulse>[0], now: number): Range {
  for (const r of ['30m', '6h', '24h', '7d'] as const) {
    if ((rows ?? []).filter((x) => x.at_unix_ms > now - RANGE_MS[r]).length >= 25) return r
  }
  return 'all'
}

function PulsePanel({ rows }: { rows: Parameters<typeof pulse>[0] }) {
  const now = useTick(5000)
  const [picked, setPicked] = useState<Range | null>(null)
  const range = picked ?? (rows ? autoRange(rows, now) : '30m')
  const option = useMemo<EChartsOption>(() => {
    const oldest = Math.min(now - 60_000, ...(rows ?? []).map((r) => r.at_unix_ms))
    const span = range === 'all' ? now - oldest : RANGE_MS[range]
    const p = pulse(rows, span, 72, now)
    const long = span > 26 * 3600_000
    const names: Partial<Record<Tone, string>> = {
      live: 'turns & executions', model: 'model', tool: 'tools', think: 'context', wait: 'policy & sessions', ok: 'discord', money: 'budget', fault: 'faults', idle: 'other',
    }
    return {
      grid: { left: 36, right: 12, top: 28, bottom: 22 },
      legend: { top: 0, left: 0, itemWidth: 10, itemHeight: 6, textStyle: { color: '#94a3b8', fontSize: 10 } },
      tooltip: { trigger: 'axis', axisPointer: { type: 'shadow', shadowStyle: { color: 'rgba(34,211,238,.06)' } } },
      xAxis: {
        type: 'category', ...axisStyle, splitLine: { show: false },
        data: p.times.map((t) => long ? `${new Date(t).toLocaleDateString([], { month: 'short', day: 'numeric' })} ${clock(t).slice(0, 2)}h` : clock(t).slice(0, 5)),
      },
      yAxis: { type: 'value', minInterval: 1, ...axisStyle },
      series: p.tones.filter((t) => p.series[t].some((v) => v > 0)).map((t) => ({
        name: names[t], type: 'bar', stack: 'events', barWidth: '72%', data: p.series[t],
        itemStyle: { color: toneHex[t], borderRadius: [2, 2, 0, 0], opacity: 0.88 },
        emphasis: { focus: 'series' },
      })),
    }
  }, [rows, now, range])
  return (
    <Panel title="Pulse · ledger events" icon={<Activity size={13} />} className="xl:col-span-2" bodyClassName="h-[230px] p-2"
      actions={<>
        <span className="num mr-1 text-[11px] text-ink-faint">{rows?.length ?? 0} rows</span>
        <Segmented value={range} options={RANGES} onChange={setPicked} />
      </>}>
      {rows ? <Echart option={option} /> : <Empty>reading the ledger…</Empty>}
    </Panel>
  )
}

function FleetGlance({ sessions, executions }: { sessions: SessionInfo[]; executions: ExecutionInfo[] }) {
  const nav = useNavigate()
  const now = useTick(5000)
  const byState = useMemo(() => {
    const m = new Map<string, number>()
    for (const e of executions) m.set(e.state, (m.get(e.state) ?? 0) + 1)
    return [...m.entries()]
  }, [executions])
  const option = useMemo<EChartsOption>(() => ({
    tooltip: { trigger: 'item' },
    series: [{
      type: 'pie', radius: ['58%', '82%'], center: ['50%', '50%'], padAngle: 3, itemStyle: { borderRadius: 4 },
      label: { show: false },
      data: byState.map(([s, n]) => ({ name: s, value: n, itemStyle: { color: toneHex[stateTone(s)] } })),
    }],
  }), [byState])
  const recent = [...sessions].sort((a, b) => (b.last_active_ms ?? 0) - (a.last_active_ms ?? 0)).slice(0, 6)
  return (
    <>
      <div className="flex items-center gap-3 border-b border-line px-3 py-2">
        <div className="h-[78px] w-[78px] shrink-0"><Echart option={option} /></div>
        <div className="flex flex-wrap gap-1.5">
          {byState.map(([s, n]) => (
            <Pill key={s} tone={stateTone(s)}>{s} <span className="num font-semibold">{n}</span></Pill>
          ))}
          <span className="num w-full text-[11px] text-ink-faint">{executions.length} executions · {sessions.length} sessions</span>
        </div>
      </div>
      <div className="min-h-0 flex-1 overflow-auto">
        {recent.map((s) => (
          <button key={s.session_id} onClick={() => nav(`/session/${s.session_id}`)}
            className="flex w-full items-center gap-2 border-b border-line/60 px-3 py-1.5 text-left text-[12px] hover:bg-white/[0.03]">
            <StatePill state={s.execution_state ?? 'idle'} />
            <span className="min-w-0 flex-1 truncate text-ink">{s.title || s.label || short(s.session_id)}</span>
            <span className="num text-[11px] text-ink-faint">{s.turns}t</span>
            <span className="num w-14 text-right text-[11px] text-money">{usd(s.cost_usd)}</span>
            <span className="num w-14 text-right text-[11px] text-ink-faint">{ago(s.last_active_ms, now)}</span>
          </button>
        ))}
        {recent.length === 0 && <Empty>no sessions yet</Empty>}
      </div>
    </>
  )
}

function Fuel({ calls }: { calls: ProviderCall[] }) {
  const latest = useMemo(() => {
    const m = new Map<string, ProviderCall>()
    for (const c of calls) if (c.rate) m.set(c.provider, c)
    return [...m.values()]
  }, [calls])
  if (!latest.length) return <Empty>no model call has reported its limits yet</Empty>
  return (
    <div className="flex h-full flex-col gap-3 overflow-auto px-2 py-1">
      {latest.map((c) => {
        const r = c.rate!
        const gauges: [string, number | undefined, number | undefined][] = [
          ['requests', r.requests_remaining, r.requests_limit],
          ['tokens', r.tokens_remaining, r.tokens_limit],
        ]
        return (
          <div key={c.provider}>
            <div className="mb-1 flex items-baseline justify-between">
              <span className="text-[12px] font-semibold text-model">{c.provider}</span>
              <span className="num text-[10px] text-ink-faint">as of {ago(c.at)}</span>
            </div>
            {gauges.map(([label, rem, lim]) => rem !== undefined && lim ? (
              <div key={label} className="mb-2">
                <div className="mb-1 flex justify-between text-[11px]">
                  <span className="text-ink-faint">{label}</span>
                  <span className="num text-ink">{tokens(rem)} / {tokens(lim)} <span className="text-ink-faint">({pct(rem / lim, 1)})</span></span>
                </div>
                <Meter value={rem} max={lim} tone={rem / lim > 0.5 ? 'ok' : rem / lim > 0.15 ? 'wait' : 'fault'} />
              </div>
            ) : null)}
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
  )
}

function Latency({ calls }: { calls: ProviderCall[] }) {
  const option = useMemo<EChartsOption>(() => ({
    grid: { left: 44, right: 12, top: 26, bottom: 24 },
    legend: { top: 0, right: 0, itemWidth: 10, itemHeight: 6, textStyle: { color: '#94a3b8', fontSize: 10 } },
    tooltip: { trigger: 'item', formatter: (p: any) => `${p.seriesName}<br/>${clock(p.value[0])} · <b>${ms(p.value[1])}</b>` },
    xAxis: { type: 'time', ...axisStyle, splitLine: { show: false } },
    yAxis: { type: 'log', logBase: 10, ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => ms(v) } },
    series: [
      { name: 'first token', type: 'scatter', symbolSize: 7, data: calls.filter((c) => c.first_token_ms).map((c) => [c.at, c.first_token_ms!]), itemStyle: { color: toneHex.live, opacity: 0.85 } },
      { name: 'total', type: 'scatter', symbolSize: 7, data: calls.filter((c) => c.total_ms).map((c) => [c.at, c.total_ms!]), itemStyle: { color: toneHex.model, opacity: 0.75 } },
    ],
  }), [calls])
  if (!calls.length) return <Empty>no model calls yet</Empty>
  return <Echart option={option} />
}

function SpendFlow({ calls, sessions }: { calls: ProviderCall[]; sessions: SessionInfo[] }) {
  const option = useMemo<EChartsOption>(() => {
    const title = new Map(sessions.map((s) => [s.session_id, s.title || s.label || short(s.session_id)]))
    const links = new Map<string, number>()
    const nodes = new Set<string>()
    const add = (a: string, b: string, v: number) => { nodes.add(a); nodes.add(b); links.set(`${a}\u0000${b}`, (links.get(`${a}\u0000${b}`) ?? 0) + v) }
    for (const c of calls) {
      if (c.cost <= 0) continue
      const s = `◦ ${title.get(c.session_id ?? '') ?? short(c.session_id)}`
      add(c.provider, c.model, c.cost)
      add(c.model, s, c.cost)
    }
    return {
      tooltip: { trigger: 'item', formatter: (p: any) => p.dataType === 'edge' ? `${p.data.source} → ${p.data.target}<br/><b>${usd(p.data.value)}</b>` : `${p.name}` },
      series: [{
        type: 'sankey', left: 4, right: 110, top: 8, bottom: 8, nodeWidth: 10, nodeGap: 8, draggable: false,
        emphasis: { focus: 'adjacency' },
        lineStyle: { color: 'gradient', opacity: 0.35, curveness: 0.5 },
        label: { color: '#cbd5e1', fontSize: 11 },
        itemStyle: { borderWidth: 0 },
        data: [...nodes].map((n) => ({ name: n, itemStyle: { color: n.startsWith('◦') ? toneHex.money : calls.some((c) => c.provider === n) ? toneHex.model : toneHex.live } })),
        links: [...links.entries()].map(([k, v]) => { const [source, target] = k.split('\u0000'); return { source, target, value: Number(v.toFixed(6)) } }),
      }],
    }
  }, [calls, sessions])
  if (!calls.some((c) => c.cost > 0)) return <Empty>no spend yet</Empty>
  return <Echart option={option} />
}
