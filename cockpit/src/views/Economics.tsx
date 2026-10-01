// Economics: where the money and the tokens go. Every figure comes from the ledger's provider.call rows, priced with
// the catalog's own per-million rates, so "saved by caching" is what the cache read would have cost as input.
import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router'
import { Bot, Brain, CalendarClock, Coins, PiggyBank, Receipt, Timer, TrendingUp } from 'lucide-react'
import type { CatalogList, Health, SessionInfo } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { useDerived, totalIn, type ProviderCall } from '@/lib/derive'
import { ms, pct, short, tokens, usd } from '@/lib/format'
import { toneHex } from '@/lib/taxonomy'
import { axisStyle, type EChartsOption } from '@/lib/chart'
import { Echart } from '@/components/Echart'
import { Empty, Kpi, Panel, Segmented } from '@/components/ui'

interface Price { input: number; output: number; cacheRead: number; cacheWrite: number; cacheWrite1h: number }

function pricing(cat?: CatalogList): Map<string, Price> {
  const m = new Map<string, Price>()
  for (const x of cat?.models ?? []) {
    const e = x.entry as Record<string, number>
    const input = e.input_per_mtok ?? 0
    // A catalog from before 13c has no 1-hour write price: Anthropic's is 2 × input (theseus-ev1).
    m.set(x.model, { input, output: e.output_per_mtok ?? 0, cacheRead: e.cache_read_per_mtok ?? 0, cacheWrite: e.cache_write_per_mtok ?? 0, cacheWrite1h: e.cache_write_1h_per_mtok ?? 2 * input })
  }
  return m
}

/** A call's cost split by token kind, from the catalog's rates; 1-hour cache writes at their own rate. */
function split(c: ProviderCall, p?: Price) {
  if (!p) return { input: 0, cacheRead: 0, cacheWrite: 0, output: 0, saved: 0 }
  const u = c.usage
  const w1h = Math.min(u.cache_creation_1h_input_tokens ?? 0, u.cache_creation_input_tokens)
  return {
    input: (u.input_tokens * p.input) / 1e6,
    cacheRead: (u.cache_read_input_tokens * p.cacheRead) / 1e6,
    cacheWrite: ((u.cache_creation_input_tokens - w1h) * p.cacheWrite + w1h * p.cacheWrite1h) / 1e6,
    output: (u.output_tokens * p.output) / 1e6,
    saved: (u.cache_read_input_tokens * (p.input - p.cacheRead)) / 1e6,
  }
}

const BUCKETS = ['hour', 'day'] as const

export default function Economics() {
  const nav = useNavigate()
  const { calls } = useDerived(5000)
  const { data: cat } = useRpc<CatalogList>('catalog.list', undefined, 60_000)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const { data: h } = useRpc<Health>('health', undefined, 5000)
  const [bucket, setBucket] = useState<(typeof BUCKETS)[number]>('day')
  const prices = useMemo(() => pricing(cat), [cat])
  const title = useMemo(() => new Map((sl?.sessions ?? []).map((s) => [s.session_id, s.title || s.label || short(s.session_id)])), [sl])

  const totals = useMemo(() => {
    let cost = 0, saved = 0, out = 0, inTok = 0, cached = 0
    const parts = { input: 0, cacheRead: 0, cacheWrite: 0, output: 0 }
    for (const c of calls) {
      cost += c.cost
      const s = split(c, prices.get(c.model))
      saved += s.saved; parts.input += s.input; parts.cacheRead += s.cacheRead; parts.cacheWrite += s.cacheWrite; parts.output += s.output
      out += c.usage.output_tokens; inTok += totalIn(c.usage); cached += c.usage.cache_read_input_tokens
    }
    return { cost, saved, out, inTok, cached, parts }
  }, [calls, prices])

  const turnsCount = useMemo(() => new Set(calls.map((c) => c.turn_id)).size, [calls])
  // The recent pace: what the last 24 hours and the last 7 days cost, from the billed calls' own times.
  const now = useTick(60_000)
  const recent = useMemo(() => {
    const day = 86_400_000
    let d1 = 0, d7 = 0
    for (const c of calls) {
      if (now - c.at <= day) d1 += c.cost
      if (now - c.at <= 7 * day) d7 += c.cost
    }
    return { d1, d7 }
  }, [calls, now])

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 2xl:grid-cols-6">
        <Kpi label="Spent · billed model calls" icon={<Coins size={12} />} value={totals.cost} format={(n) => usd(n)} tone="money"
          hint={h?.cost_usd_total !== undefined && Math.abs(h.cost_usd_total - totals.cost) > 0.0005
            ? `sessions' totals ${usd(h.cost_usd_total)}: the calls are the record. Builds before theseus-hco (2026-09-28) missed a failed turn's cost.`
            : `${calls.length} model calls`} />
        <Kpi label="Saved by caching" icon={<PiggyBank size={12} />} value={totals.saved} format={(n) => usd(n)} tone="ok" hint={totals.cost ? `${pct(totals.saved / (totals.cost + totals.saved))} of the uncached price` : undefined} />
        <Kpi label="Cache hit" icon={<Brain size={12} />} value={totals.inTok ? totals.cached / totals.inTok : 0} format={(n) => pct(n, 1)} tone="think" hint={`${tokens(totals.cached)} of ${tokens(totals.inTok)} input tokens`} />
        <Kpi label="Per turn" icon={<Receipt size={12} />} value={turnsCount ? totals.cost / turnsCount : 0} format={(n) => usd(n)} tone="money"
          hint={`${turnsCount} turns · ${usd(calls.length ? totals.cost / calls.length : 0)} a model call`} />
        <Kpi label="Last 24 hours" icon={<CalendarClock size={12} />} value={recent.d1} format={(n) => usd(n)} tone="money"
          hint={`${usd(recent.d7)} in 7 days · ${usd(recent.d7 / 7)} a day on average`} />
        <Kpi label="Output tokens" icon={<TrendingUp size={12} />} value={totals.out} format={tokens} tone="live" hint={totals.cost ? `${pct(totals.parts.output / totals.cost)} of spend` : undefined} />
      </div>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <Panel title="Spend over time, by model" icon={<TrendingUp size={13} />} className="xl:col-span-2" bodyClassName="h-[300px] p-2"
          actions={<Segmented value={bucket} options={BUCKETS} onChange={setBucket} />}>
          <SpendOverTime calls={calls} bucket={bucket} />
        </Panel>
        <Panel title="Where the money goes · by token kind" icon={<Coins size={13} />} bodyClassName="h-[300px] p-2">
          <WhereItGoes parts={totals.parts} saved={totals.saved} />
        </Panel>
      </div>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <Panel title="Provider → model → session" icon={<Bot size={13} />} bodyClassName="h-[340px] p-2">
          <Sunburst calls={calls} title={title} />
        </Panel>
        <Panel title="Cost per session" icon={<Receipt size={13} />} bodyClassName="h-[340px] p-2">
          <PerSession calls={calls} title={title} onPick={(sid) => nav(`/session/${sid}`)} />
        </Panel>
        <Panel title="Latency per model · first token and total" icon={<Timer size={13} />} bodyClassName="h-[340px] p-2">
          <LatencyByModel calls={calls} />
        </Panel>
      </div>
    </div>
  )
}

const MODEL_COLORS = [toneHex.model, toneHex.live, toneHex.think, toneHex.tool, toneHex.ok, toneHex.money, toneHex.wait]

function SpendOverTime({ calls, bucket }: { calls: ProviderCall[]; bucket: 'hour' | 'day' }) {
  const option = useMemo<EChartsOption>(() => {
    const size = bucket === 'hour' ? 3600_000 : 86400_000
    const models = [...new Set(calls.map((c) => c.model))]
    const by = new Map<number, Map<string, number>>()
    for (const c of calls) {
      const k = Math.floor(c.at / size) * size
      const m = by.get(k) ?? new Map<string, number>()
      m.set(c.model, (m.get(c.model) ?? 0) + c.cost)
      by.set(k, m)
    }
    const keys = [...by.keys()].sort((a, b) => a - b)
    const perKey = keys.map((k) => [...by.get(k)!.values()].reduce((a, b) => a + b, 0))
    const cum = keys.map((k, i) => [k, Number(perKey.slice(0, i + 1).reduce((a, b) => a + b, 0).toFixed(6))])
    return {
      grid: { left: 52, right: 52, top: 30, bottom: 26 },
      legend: { top: 0, left: 0, itemWidth: 10, itemHeight: 6, textStyle: { color: '#94a3b8', fontSize: 10 } },
      tooltip: { trigger: 'axis', valueFormatter: (v: any) => usd(Number(v)) },
      xAxis: { type: 'time', ...axisStyle, splitLine: { show: false } },
      yAxis: [
        { type: 'value', ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => usd(v, 2) } },
        { type: 'value', ...axisStyle, splitLine: { show: false }, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => usd(v, 2) } },
      ],
      series: [
        ...models.map((m, i) => ({
          name: m, type: 'bar' as const, stack: 'spend', barMaxWidth: 28,
          data: keys.map((k) => [k, by.get(k)!.get(m) ?? 0]),
          itemStyle: { color: MODEL_COLORS[i % MODEL_COLORS.length], opacity: 0.85 },
        })),
        { name: 'cumulative', type: 'line' as const, yAxisIndex: 1, data: cum, smooth: 0.3, showSymbol: false, lineStyle: { color: toneHex.money, width: 2 },
          areaStyle: { color: { type: 'linear', x: 0, y: 0, x2: 0, y2: 1, colorStops: [{ offset: 0, color: `${toneHex.money}33` }, { offset: 1, color: `${toneHex.money}00` }] } } },
      ],
    }
  }, [calls, bucket])
  if (!calls.length) return <Empty>no model calls yet</Empty>
  return <Echart option={option} />
}

function WhereItGoes({ parts, saved }: { parts: { input: number; cacheRead: number; cacheWrite: number; output: number }; saved: number }) {
  const option = useMemo<EChartsOption>(() => ({
    tooltip: { trigger: 'item', formatter: (p: any) => `${p.name}<br/><b>${usd(p.value)}</b> · ${p.percent}%` },
    legend: { bottom: 0, itemWidth: 10, itemHeight: 6, textStyle: { color: '#94a3b8', fontSize: 10 } },
    series: [{
      type: 'pie', radius: ['46%', '72%'], center: ['50%', '45%'], padAngle: 2, itemStyle: { borderRadius: 5 },
      label: { color: '#cbd5e1', fontSize: 11, formatter: (p: any) => `${p.name}\n${usd(p.value)}` },
      data: [
        { name: 'output', value: Number(parts.output.toFixed(6)), itemStyle: { color: toneHex.money } },
        { name: 'cache write', value: Number(parts.cacheWrite.toFixed(6)), itemStyle: { color: toneHex.model } },
        { name: 'cache read', value: Number(parts.cacheRead.toFixed(6)), itemStyle: { color: toneHex.think } },
        { name: 'input', value: Number(parts.input.toFixed(6)), itemStyle: { color: toneHex.live } },
      ],
    }],
  }), [parts])
  const total = parts.input + parts.cacheRead + parts.cacheWrite + parts.output
  if (total <= 0) return <Empty>no priced calls (the catalog has no rates for these models)</Empty>
  return (
    <div className="relative h-full">
      <Echart option={option} />
      <div className="pointer-events-none absolute left-1/2 top-[45%] -translate-x-1/2 -translate-y-1/2 text-center">
        <div className="num text-[15px] font-semibold text-money">{usd(total)}</div>
        <div className="num text-[10px] text-ok">saved {usd(saved)}</div>
      </div>
    </div>
  )
}

function Sunburst({ calls, title }: { calls: ProviderCall[]; title: Map<string, string> }) {
  const option = useMemo<EChartsOption>(() => {
    const tree = new Map<string, Map<string, Map<string, number>>>()
    for (const c of calls) {
      const p = tree.get(c.provider) ?? new Map(); tree.set(c.provider, p)
      const m = p.get(c.model) ?? new Map(); p.set(c.model, m)
      const s = title.get(c.session_id ?? '') ?? short(c.session_id)
      m.set(s, (m.get(s) ?? 0) + c.cost)
    }
    const data = [...tree.entries()].map(([prov, models], i) => ({
      name: prov, itemStyle: { color: MODEL_COLORS[i % MODEL_COLORS.length] },
      children: [...models.entries()].map(([model, sessions]) => ({
        name: model,
        children: [...sessions.entries()].map(([s, v]) => ({ name: s, value: Number(v.toFixed(6)) })),
      })),
    }))
    return {
      tooltip: { trigger: 'item', formatter: (p: any) => `${p.name}<br/><b>${usd(p.value)}</b>` },
      series: [{
        type: 'sunburst', radius: ['12%', '92%'], data, sort: undefined, nodeClick: 'rootToNode',
        itemStyle: { borderColor: '#070a10', borderWidth: 2 },
        label: { color: '#e2e8f0', fontSize: 10, minAngle: 12 },
        levels: [{}, { r0: '12%', r: '38%', label: { rotate: 0 } }, { r0: '38%', r: '66%', itemStyle: { opacity: 0.85 } }, { r0: '66%', r: '92%', label: { position: 'outside', fontSize: 9 }, itemStyle: { opacity: 0.7 } }],
      }],
    }
  }, [calls, title])
  if (!calls.some((c) => c.cost > 0)) return <Empty>no spend yet</Empty>
  return <Echart option={option} />
}

function PerSession({ calls, title, onPick }: { calls: ProviderCall[]; title: Map<string, string>; onPick: (sid: string) => void }) {
  const rows = useMemo(() => {
    const m = new Map<string, number>()
    for (const c of calls) if (c.session_id) m.set(c.session_id, (m.get(c.session_id) ?? 0) + c.cost)
    return [...m.entries()].sort((a, b) => b[1] - a[1]).slice(0, 14)
  }, [calls])
  const option = useMemo<EChartsOption>(() => ({
    grid: { left: 150, right: 56, top: 8, bottom: 8 },
    tooltip: { trigger: 'item', formatter: (p: any) => `${p.name}<br/><b>${usd(p.value)}</b>` },
    xAxis: { type: 'value', show: false },
    yAxis: { type: 'category', inverse: true, data: rows.map(([sid]) => title.get(sid) ?? short(sid)), ...axisStyle, axisLabel: { color: '#cbd5e1', fontSize: 11, width: 140, overflow: 'truncate' } },
    series: [{
      type: 'bar', data: rows.map(([, v]) => Number(v.toFixed(6))), barMaxWidth: 16,
      itemStyle: { color: { type: 'linear', x: 0, y: 0, x2: 1, y2: 0, colorStops: [{ offset: 0, color: `${toneHex.money}55` }, { offset: 1, color: toneHex.money }] }, borderRadius: [0, 4, 4, 0] },
      label: { show: true, position: 'right', color: '#facc15', fontSize: 10, fontFamily: 'JetBrains Mono Variable', formatter: (p: any) => usd(p.value) },
    }],
  }), [rows, title])
  if (!rows.length) return <Empty>no spend yet</Empty>
  return <Echart option={option} onClick={(p: any) => { const r = rows[p.dataIndex]; if (r) onPick(r[0]) }} />
}

function LatencyByModel({ calls }: { calls: ProviderCall[] }) {
  const option = useMemo<EChartsOption>(() => {
    const models = [...new Set(calls.map((c) => c.model))]
    return {
      grid: { left: 52, right: 12, top: 28, bottom: 40 },
      legend: { top: 0, left: 0, itemWidth: 10, itemHeight: 6, textStyle: { color: '#94a3b8', fontSize: 10 } },
      tooltip: { trigger: 'item', formatter: (p: any) => `${p.seriesName} · ${p.value[0]}<br/><b>${ms(p.value[1])}</b>` },
      xAxis: { type: 'category', data: models, ...axisStyle, axisLabel: { ...axisStyle.axisLabel, interval: 0, rotate: models.length > 3 ? 20 : 0 } },
      yAxis: { type: 'log', logBase: 10, ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => ms(v) } },
      series: [
        { name: 'first token', type: 'scatter', symbolSize: 8, data: calls.filter((c) => c.first_token_ms).map((c) => [c.model, c.first_token_ms!]), itemStyle: { color: toneHex.live, opacity: 0.7 } },
        { name: 'total', type: 'scatter', symbolSize: 8, symbol: 'diamond', data: calls.filter((c) => c.total_ms).map((c) => [c.model, c.total_ms!]), itemStyle: { color: toneHex.model, opacity: 0.7 } },
      ],
    }
  }, [calls])
  if (!calls.length) return <Empty>no model calls yet</Empty>
  return <Echart option={option} />
}
