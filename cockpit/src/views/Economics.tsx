// Economics: where the money and the tokens go. Every figure comes from the ledger's provider.call rows, priced with
// the catalog's own per-million rates, so "saved by caching" is what the cache read would have cost as input.
//
// The charts follow the chart method (`lib/viz.ts`, theseus-hnof): each one's form by its job, one axis a chart, a
// model in the same colour in every chart here (its categorical slot, in the order the record first names it), the
// figures in the ink, and every chart with a hover tip and a table view.
import { useCallback, useMemo } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { AlertTriangle, Bot, Brain, CalendarClock, Coins, PiggyBank, Receipt, Timer, TrendingUp } from 'lucide-react'
import type { CatalogList, Health, SessionInfo } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { providerCalls, totalIn, type ProviderCall } from '@/lib/derive'
import { useHistoryRows } from '@/lib/history'
import { cacheBy, pricing, split } from '@/lib/money'
import { ms, pct, short, tokens, usd } from '@/lib/format'
import type { EChartsOption } from '@/lib/chart'
import {
  BUCKETS, CHROME, FONTS, MARK, OTHER, TIME_LABELS, TIP_FRAME, TOKEN_KINDS, barRadius, baseAxis, bucketFor, jitter, kindTokens, latencyByModel,
  msLogTick, niceScale, shares, slotColor, slots, spendByBucket, spendBySession, spendTree, stackTop, usdTick, valueAxis,
  type Bucket, type Latency, type SessionSpend, type SpendNode, type SpendSeries, type TokenKind,
} from '@/lib/viz'
import { tip, type TipRow } from '@/lib/viztip'
import { Echart } from '@/components/Echart'
import { ChartPanel, StatTile, Swatch, TipArea, TipBody, TipTarget, type LegendItem } from '@/components/ChartPanel'
import { Empty, Panel, Segmented } from '@/components/ui'

/** What the models past the eighth slot fold into. */
const OTHER_KEY = 'other models'
const C = CHROME.dark
/** The sessions the cost chart draws; its table lists them all. */
const TOP_SESSIONS = 14

interface Palette { keyOf: (model: string) => string; colorOf: (key: string) => string; keys: string[] }

export default function Economics() {
  const nav = useNavigate()
  // Every billed call in the record: the whole ledger, not its newest rows (a read caps at 1,000).
  const { rows } = useHistoryRows()
  const calls = useMemo(() => providerCalls(rows), [rows])
  const { data: cat } = useRpc<CatalogList>('catalog.list', undefined, 60_000)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const { data: h } = useRpc<Health>('health', undefined, 5000)
  // The bucket is the view's state, so it lives in the address (?bucket=hour). Unset, it follows the record's span, so a
  // short record is never one column.
  const [params, setParams] = useSearchParams()
  const asked = BUCKETS.find((b) => b === params.get('bucket'))
  const bucket: Bucket = asked ?? bucketFor(calls.length ? calls[calls.length - 1].at - calls[0].at : 0)
  const setBucket = (b: Bucket) => setParams((p) => { p.set('bucket', b); return p }, { replace: true })
  const prices = useMemo(() => pricing(cat), [cat])
  // A session's profile, as the session list says it now: every call of a session counts under it.
  const profileOf = useMemo(() => new Map((sl?.sessions ?? []).map((s) => [s.session_id, s.profile ?? '—'])), [sl])
  const title = useMemo(() => new Map((sl?.sessions ?? []).map((s) => [s.session_id, s.title || s.label || short(s.session_id)])), [sl])
  const nameOf = useCallback((sid: string) => (sid === '—' ? 'no session' : title.get(sid) ?? short(sid)), [title])
  const pick = useCallback((sid: string) => { if (sid !== '—') nav(`/session/${sid}`) }, [nav])

  // A model's colour is its slot, in the order the record first names it: the same in every chart here, and the same
  // as the record grows. Past eight models, the eighth slot on folds into "other models", in the de-emphasis gray.
  const palette = useMemo<Palette>(() => {
    const { slot, folded } = slots(calls.map((c) => c.model))
    return {
      keyOf: (m) => (slot.has(m) ? m : OTHER_KEY),
      colorOf: (k) => (k === OTHER_KEY ? OTHER : slotColor(slot, k)),
      keys: [...slot.keys(), ...(folded.length ? [OTHER_KEY] : [])],
    }
  }, [calls])

  const totals = useMemo(() => {
    let cost = 0, saved = 0, out = 0, inTok = 0, cached = 0
    const parts: Record<TokenKind, number> = { input: 0, cacheRead: 0, cacheWrite: 0, cacheWrite1h: 0, output: 0 }
    for (const c of calls) {
      cost += c.cost
      const s = split(c, prices.get(c.model))
      saved += s.saved
      parts.input += s.input; parts.cacheRead += s.cacheRead; parts.cacheWrite += s.cacheWrite; parts.cacheWrite1h += s.cacheWrite1h; parts.output += s.output
      out += c.usage.output_tokens; inTok += totalIn(c.usage); cached += c.usage.cache_read_input_tokens
    }
    return { cost, saved, out, inTok, cached, parts, tok: kindTokens(calls) }
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

  const spend = useMemo(() => spendByBucket(calls, bucket, palette.keyOf), [calls, bucket, palette])
  // Every model on its own, unfolded: the colours stop at eight, but the tip and the table name each model past them.
  const spendAll = useMemo(() => spendByBucket(calls, bucket), [calls, bucket])
  const tree = useMemo(() => spendTree(calls), [calls])
  const sessions = useMemo(() => spendBySession(calls, palette.keys), [calls, palette])
  const latency = useMemo(() => latencyByModel(calls, [...new Set(calls.map((c) => c.model))]), [calls])
  const modelLegend: LegendItem[] = spend.series.map((s) => ({ key: s.key, label: s.key, color: palette.colorOf(s.key), mark: 'rect' }))
  const partsTotal = TOKEN_KINDS.reduce((a, k) => a + totals.parts[k.key], 0)

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 2xl:grid-cols-6">
        <StatTile label="Spent · billed model calls" icon={<Coins size={12} />} value={totals.cost} format={(n) => usd(n)}
          hint={h?.cost_usd_total !== undefined && Math.abs(h.cost_usd_total - totals.cost) > 0.0005
            ? `sessions' totals ${usd(h.cost_usd_total)}: the calls are the record. Builds before theseus-hco (2026-09-28) missed a failed turn's cost.`
            : `${calls.length} model calls`} />
        <StatTile label="Saved by caching" icon={<PiggyBank size={12} />} value={totals.saved} format={(n) => usd(n)} hint={totals.cost ? `${pct(totals.saved / (totals.cost + totals.saved))} of the uncached price` : undefined} />
        <StatTile label="Cache hit" icon={<Brain size={12} />} value={totals.inTok ? totals.cached / totals.inTok : 0} format={(n) => pct(n, 1)} hint={`${tokens(totals.cached)} of ${tokens(totals.inTok)} input tokens`} />
        <StatTile label="Per turn" icon={<Receipt size={12} />} value={turnsCount ? totals.cost / turnsCount : 0} format={(n) => usd(n)}
          hint={`${turnsCount} turns · ${usd(calls.length ? totals.cost / calls.length : 0)} a model call`} />
        <StatTile label="Last 24 hours" icon={<CalendarClock size={12} />} value={recent.d1} format={(n) => usd(n)}
          hint={`${usd(recent.d7)} in 7 days · ${usd(recent.d7 / 7)} a day on average`} />
        <StatTile label="Output tokens" icon={<TrendingUp size={12} />} value={totals.out} format={tokens} hint={totals.cost ? `${pct(totals.parts.output / totals.cost)} of spend` : undefined} />
      </div>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <ChartPanel id="spend" title="Spend over time, by model" icon={<TrendingUp size={13} />} className="xl:col-span-2" height={300}
          actions={<Segmented value={bucket} options={BUCKETS} onChange={setBucket} />}
          legend={modelLegend} empty={calls.length ? undefined : 'no model calls yet'}
          table={spendTable(spendAll, bucket, palette.colorOf, palette.keyOf)}>
          <SpendOverTime s={spend} all={spendAll} bucket={bucket} colorOf={palette.colorOf} keyOf={palette.keyOf} />
        </ChartPanel>
        <ChartPanel id="kinds" title="Where the money goes · by token kind" icon={<Coins size={13} />} height={300}
          empty={partsTotal > 0 ? undefined : 'no priced calls (the catalog has no rates for these models)'}
          table={kindsTable(totals.parts, totals.tok)}>
          <WhereItGoes parts={totals.parts} tok={totals.tok} saved={totals.saved} />
        </ChartPanel>
      </div>

      <div className="grid grid-cols-1 gap-3 xl:grid-cols-3">
        <ChartPanel id="tree" title="Provider → model → session" icon={<Bot size={13} />} height={340}
          empty={calls.some((c) => c.cost > 0) ? undefined : 'no spend yet'} table={treeTable(tree, nameOf)}>
          <SpendTreeBars tree={tree} palette={palette} nameOf={nameOf} onPick={pick} />
        </ChartPanel>
        <ChartPanel id="sessions" title={sessions.length > TOP_SESSIONS ? `Cost per session · the top ${TOP_SESSIONS} of ${sessions.length}` : 'Cost per session'}
          icon={<Receipt size={13} />} height={340} legend={modelLegend}
          empty={sessions.length ? undefined : 'no spend yet'} table={sessionsTable(sessions, nameOf)}>
          <SessionBars rows={sessions.slice(0, TOP_SESSIONS)} palette={palette} nameOf={nameOf} onPick={pick} />
        </ChartPanel>
        <ChartPanel id="latency" title="Latency per model · first token and total" icon={<Timer size={13} />} height={340}
          legend={[{ key: 'first', label: 'first token', color: C.secondary, mark: 'ring' }, { key: 'total', label: 'total', color: C.secondary, mark: 'dot' }]}
          empty={latency.length ? undefined : 'no timed model calls yet'} table={latencyTable(latency)}>
          <LatencyStrip lat={latency} palette={palette} />
        </ChartPanel>
      </div>

      <Panel title="Caching · by profile" icon={<PiggyBank size={13} />} bodyClassName="p-2">
        <CacheByProfile calls={calls} prices={prices} profileOf={profileOf} />
      </Panel>
    </div>
  )
}

// ---------------------------------------------------------------- spend over time

/** Two small multiples on one time axis, each with its own single scale (never a second y axis on one plot): the
 *  running total, a line, and each bucket's spend, columns stacked by model. Past eight models the tail is one column of
 *  "other models", and the tip names each of them under it, from the unfolded series `all`. */
function SpendOverTime({ s, all, bucket, colorOf, keyOf }: { s: SpendSeries; all: SpendSeries; bucket: Bucket; colorOf: (k: string) => string; keyOf: (m: string) => string }) {
  const option = useMemo<EChartsOption>(() => {
    const mids = s.starts.map((t, i) => (t + s.ends[i]) / 2)
    const per = niceScale(Math.max(0, ...s.totals))
    const run = niceScale(s.cumulative.at(-1) ?? 0, 2)
    const tops = stackTop(s.series.map((x) => x.values))
    const axis = baseAxis(), vaxis = valueAxis()
    const heading = (text: string, top: number) => ({ text, left: 64, top, textStyle: { color: C.secondary, fontSize: 11, fontWeight: 500 as const, fontFamily: FONTS.sans } })
    // Both plots span the buckets exactly. ECharts widens a time axis that carries bars (`containShape`), which slid the
    // columns away from the running total's points above them; a column sits mid-bucket, so it fits without it.
    const x = (gridIndex: number, labels: boolean) => ({ type: 'time' as const, gridIndex, min: s.starts[0], max: s.ends.at(-1), containShape: false, ...axis, axisLabel: { ...axis.axisLabel, show: labels, formatter: TIME_LABELS } })
    return {
      grid: [{ left: 64, right: 18, top: 24, height: 58 }, { left: 64, right: 18, top: 120, bottom: 24 }],
      title: [
        heading(`running total · ${usd(s.cumulative.at(-1) ?? 0)}`, 2),
        heading(`spend per ${bucket === '5 min' ? 'five minutes' : bucket}${s.series.length === 1 ? ` · ${s.series[0].key}` : ''}`, 98),
      ],
      axisPointer: { link: [{ xAxisIndex: 'all' }] },
      tooltip: {
        ...TIP_FRAME, trigger: 'axis', axisPointer: { type: 'line', lineStyle: { color: C.axis, width: 1, type: 'solid' } },
        formatter: (ps: any) => {
          const i = (Array.isArray(ps) ? ps[0] : ps)?.dataIndex
          if (i === undefined || s.starts[i] === undefined) return ''
          const rows: TipRow[] = s.series.filter((x) => x.values[i] > 0).flatMap((x): TipRow[] => [
            { value: usd(x.values[i]), label: x.key, color: colorOf(x.key), mark: 'rect' },
            // The models folded into "other models", each with its own dollars in this bucket.
            ...(x.key === OTHER_KEY ? all.series.filter((m) => keyOf(m.key) === OTHER_KEY && m.values[i] > 0).map((m): TipRow => ({ value: usd(m.values[i]), label: `  ${m.key}`, strong: false })) : []),
          ])
          rows.push({ value: usd(s.totals[i]), label: inBucket(bucket) }, { value: usd(s.cumulative[i]), label: 'running total', color: C.secondary, mark: 'line', strong: false })
          return tip(bucketWords(s.starts[i], s.ends[i], bucket), rows)
        },
      },
      xAxis: [x(0, false), x(1, true)],
      yAxis: [
        { ...vaxis, gridIndex: 0, min: 0, max: run.max, interval: run.interval, axisLabel: { ...vaxis.axisLabel, formatter: usdTick(run.interval) } },
        { ...vaxis, gridIndex: 1, min: 0, max: per.max, interval: per.interval, axisLabel: { ...vaxis.axisLabel, formatter: usdTick(per.interval) } },
      ],
      series: [
        {
          type: 'line', name: 'running total', xAxisIndex: 0, yAxisIndex: 0, data: mids.map((m, i) => [m, s.cumulative[i]]),
          symbol: 'circle', symbolSize: MARK.marker + 2 * MARK.ring, showSymbol: mids.length === 1,
          lineStyle: { width: MARK.line, color: C.secondary, cap: 'round', join: 'round' },
          itemStyle: { color: C.secondary, borderColor: C.surface, borderWidth: MARK.ring },
          areaStyle: { color: C.secondary, opacity: MARK.wash },
        },
        ...s.series.map((x, i) => ({
          type: 'bar' as const, name: x.key, stack: 'spend', xAxisIndex: 1, yAxisIndex: 1, barMaxWidth: MARK.bar,
          itemStyle: { color: colorOf(x.key), borderColor: C.surface, borderWidth: MARK.gap / 2 },
          data: mids.map((m, j) => ({ value: [m, x.values[j]], itemStyle: { borderRadius: tops[j] === i ? barRadius(false) : 0 } })),
        })),
      ],
    }
  }, [s, all, bucket, colorOf, keyOf])
  // A new set of models is a new chart: ECharts merges an option into the last one, and would keep a dropped series.
  return <Echart key={s.series.map((x) => x.key).join('|')} option={option} />
}

const inBucket = (b: Bucket) => (b === '5 min' ? 'in these five minutes' : `this ${b}`)

function bucketWords(start: number, end: number, bucket: Bucket): string {
  const day = new Date(start).toLocaleDateString([], { weekday: 'short', month: 'short', day: 'numeric' })
  if (bucket === 'day') return day
  const t = (at: number) => new Date(at).toLocaleTimeString([], { hour12: false, hour: '2-digit', minute: '2-digit' })
  return `${day} · ${t(start)}–${t(end)}`
}

/** The spend over time as rows: every model its own column, unfolded (the chart's colours stop at eight; this does not),
 *  each bucket's total, and the running total. A model past the eighth says so in its column's head. */
function spendTable(s: SpendSeries, bucket: Bucket, colorOf: (k: string) => string, keyOf: (m: string) => string) {
  const rows = s.starts.map((start, i) => ({ i, start }))
  return {
    caption: `spend per ${bucket}, by model (every model), with the running total`,
    rows, rowKey: (r: { start: number }) => String(r.start),
    columns: [
      { key: 'when', label: bucket, cell: (r: { i: number; start: number }) => bucketWords(r.start, s.ends[r.i], bucket) },
      ...s.series.map((x) => ({
        key: `m:${x.key}`, label: keyOf(x.key) === OTHER_KEY ? `${x.key} (in other models)` : x.key, num: true,
        cell: (r: { i: number }) => usd(x.values[r.i]), title: () => (colorOf(keyOf(x.key)) === OTHER ? 'drawn in the chart as other models' : undefined),
      })),
      { key: 'total', label: inBucket(bucket), num: true, cell: (r: { i: number }) => usd(s.totals[r.i]) },
      { key: 'run', label: 'running total', num: true, cell: (r: { i: number }) => usd(s.cumulative[r.i]) },
    ],
  }
}

// ---------------------------------------------------------------- where the money goes

/** Part to whole: the total, then one bar of the token kinds' shares (a 2 px gap between them, the data end rounded),
 *  then each kind with its dollars and share: the key and the direct labels at once. */
function WhereItGoes({ parts, tok, saved }: { parts: Record<TokenKind, number>; tok: Record<TokenKind, number>; saved: number }) {
  const total = TOKEN_KINDS.reduce((a, k) => a + parts[k.key], 0)
  const sh = shares(TOKEN_KINDS.map((k) => parts[k.key]))
  const kinds = TOKEN_KINDS.map((k, i) => ({ ...k, cost: parts[k.key], share: sh[i], tokens: tok[k.key] }))
  const drawn = kinds.filter((k) => k.cost > 0)
  return (
    <TipArea className="flex h-full flex-col px-2 pt-1">
      <div className="flex flex-wrap items-baseline gap-x-3">
        <span className="viz-figure text-[30px] font-semibold leading-none text-ink">{usd(total)}</span>
        <span className="text-[12px] text-ink-dim">spent · caching saved {usd(saved)}</span>
      </div>
      <div className="mt-4 flex h-6 w-full gap-[2px]" role="group" aria-label="the token kinds' shares of the spend">
        {drawn.map((k, i) => (
          <TipTarget key={k.key} label={`${k.word}: ${usd(k.cost)}, ${pct(k.share, 1)}`} className="viz-mark h-full min-w-[2px]"
            style={{ flex: `${k.share * 1000} 1 0`, background: k.color, borderRadius: i === drawn.length - 1 ? '0 4px 4px 0' : 0 }}
            tip={<TipBody rows={[{ value: usd(k.cost), label: k.word, color: k.color, mark: 'rect' }]} foot={`${pct(k.share, 1)} of the spend · ${tokens(k.tokens)} tokens`} />} />
        ))}
      </div>
      <ul className="mt-4 grid grid-cols-[auto_1fr_auto_auto] items-center gap-x-3 gap-y-1.5 text-[12px]">
        {kinds.map((k) => (
          <li key={k.key} className="contents">
            <Swatch color={k.color} />
            <span className="truncate text-ink-dim">{k.word}</span>
            <span className="num text-right text-ink">{usd(k.cost)}</span>
            <span className="num w-[5ch] text-right text-ink-faint">{pct(k.share)}</span>
          </li>
        ))}
      </ul>
    </TipArea>
  )
}

function kindsTable(parts: Record<TokenKind, number>, tok: Record<TokenKind, number>) {
  const total = TOKEN_KINDS.reduce((a, k) => a + parts[k.key], 0)
  type R = { key: string; word: string; tokens: number; cost: number }
  const rows: R[] = [
    ...TOKEN_KINDS.map((k) => ({ key: k.key, word: k.word, tokens: tok[k.key], cost: parts[k.key] })),
    { key: 'writes', word: 'cache writes, both', tokens: tok.cacheWrite + tok.cacheWrite1h, cost: parts.cacheWrite + parts.cacheWrite1h },
    { key: 'all', word: 'every kind', tokens: TOKEN_KINDS.reduce((a, k) => a + tok[k.key], 0), cost: total },
  ]
  return {
    caption: 'the spend by token kind', rows, rowKey: (r: R) => r.key,
    columns: [
      { key: 'kind', label: 'token kind', cell: (r: R) => r.word },
      { key: 'tokens', label: 'tokens', num: true, cell: (r: R) => tokens(r.tokens) },
      { key: 'cost', label: 'spent', num: true, cell: (r: R) => usd(r.cost) },
      { key: 'share', label: 'share', num: true, cell: (r: R) => pct(total > 0 ? r.cost / total : 0, 1) },
    ],
  }
}

// ---------------------------------------------------------------- provider → model → session

/** The hierarchy as bars on one scale: each provider's total, its models' bars in their colours, and under each model
 *  its three costliest sessions as thinner bars of the same hue (the rest folded, all of them in the table). */
function SpendTreeBars({ tree, palette, nameOf, onPick }: { tree: SpendNode[]; palette: Palette; nameOf: (sid: string) => string; onPick: (sid: string) => void }) {
  const max = Math.max(0, ...tree.flatMap((p) => p.children.map((m) => m.cost)))
  const w = (v: number) => `${max > 0 ? (v / max) * 100 : 0}%`
  return (
    <TipArea className="h-full overflow-y-auto overflow-x-hidden px-1">
      {tree.map((p) => (
        <section key={p.key} className="mb-2.5">
          <div className="flex items-baseline justify-between border-b border-line px-1 pb-0.5 text-[11px]">
            <span className="font-semibold uppercase tracking-wider text-ink-dim">{p.key}</span>
            <span className="num text-ink-dim">{usd(p.cost)} · {p.calls} calls</span>
          </div>
          {p.children.map((m) => {
            const color = palette.colorOf(palette.keyOf(m.key))
            const rest = m.children.slice(3)
            return (
              <div key={m.key} className="mt-1">
                <TipTarget className="viz-row grid w-full grid-cols-[minmax(0,38%)_1fr_auto] items-center gap-x-2 px-1 py-1"
                  label={`${m.key}: ${usd(m.cost)}`}
                  tip={<TipBody head={p.key} rows={[{ value: usd(m.cost), label: m.key, color, mark: 'rect' }]} foot={`${m.calls} model calls · ${m.children.length} session${m.children.length === 1 ? '' : 's'}`} />}>
                  <span className="truncate text-left text-[12px] text-ink">{m.key}</span>
                  <span className="h-3"><span className="block h-full" style={{ width: w(m.cost), minWidth: 2, background: color, borderRadius: '0 4px 4px 0' }} /></span>
                  <span className="num w-[8ch] text-right text-[11px] text-ink">{usd(m.cost)}</span>
                </TipTarget>
                {m.children.slice(0, 3).map((s) => (
                  <TipTarget key={s.key} onClick={s.key === '—' ? undefined : () => onPick(s.key)} label={`${nameOf(s.key)}: ${usd(s.cost)}`}
                    className="viz-row grid w-full grid-cols-[minmax(0,38%)_1fr_auto] items-center gap-x-2 py-1 pl-4 pr-1"
                    tip={<TipBody head={`${m.key} · a session`} rows={[{ value: usd(s.cost), label: nameOf(s.key), color, mark: 'rect' }]} foot={`${s.calls} model calls${s.key === '—' ? '' : ' · open it with a click'}`} />}>
                    <span className="truncate text-left text-[11px] text-ink-dim">{nameOf(s.key)}</span>
                    <span className="h-1.5"><span className="block h-full" style={{ width: w(s.cost), minWidth: 2, background: color, borderRadius: '0 3px 3px 0' }} /></span>
                    <span className="num w-[8ch] text-right text-[11px] text-ink-dim">{usd(s.cost)}</span>
                  </TipTarget>
                ))}
                {rest.length > 0 && (
                  <div className="py-[2px] pl-4 text-[10.5px] text-ink-faint">
                    and {rest.length} more session{rest.length === 1 ? '' : 's'}, {usd(rest.reduce((a, s) => a + s.cost, 0))} (the table lists them)
                  </div>
                )}
              </div>
            )
          })}
        </section>
      ))}
    </TipArea>
  )
}

function treeTable(tree: SpendNode[], nameOf: (sid: string) => string) {
  type R = { key: string; provider: string; model: string; session: string; calls: number; cost: number }
  const rows: R[] = tree.flatMap((p) => p.children.flatMap((m) => m.children.map((s) => ({
    key: `${p.key}/${m.key}/${s.key}`, provider: p.key, model: m.key, session: nameOf(s.key), calls: s.calls, cost: s.cost,
  }))))
  return {
    caption: 'spend by provider, model, and session', rows, rowKey: (r: R) => r.key,
    columns: [
      { key: 'provider', label: 'provider', cell: (r: R) => r.provider },
      { key: 'model', label: 'model', cell: (r: R) => r.model },
      { key: 'session', label: 'session', cell: (r: R) => r.session, title: (r: R) => r.session },
      { key: 'calls', label: 'calls', num: true, cell: (r: R) => r.calls },
      { key: 'cost', label: 'spent', num: true, cell: (r: R) => usd(r.cost) },
    ],
  }
}

// ---------------------------------------------------------------- cost per session

/** Each session's cost as a horizontal bar on one scale, stacked by model in the models' colours (a 2 px gap between
 *  models), its title beside it and its dollars at the end; a click opens the session. */
function SessionBars({ rows, palette, nameOf, onPick }: { rows: SessionSpend[]; palette: Palette; nameOf: (sid: string) => string; onPick: (sid: string) => void }) {
  const max = rows[0]?.cost ?? 0
  return (
    <TipArea className="h-full overflow-y-auto overflow-x-hidden px-1">
      {rows.map((r) => {
        // Models folded into "other" are one segment.
        const segs = new Map<string, number>()
        for (const b of r.byModel) segs.set(palette.keyOf(b.model), (segs.get(palette.keyOf(b.model)) ?? 0) + b.cost)
        const parts = [...segs].filter(([, v]) => v > 0)
        return (
          <TipTarget key={r.session} onClick={() => onPick(r.session)} label={`${nameOf(r.session)}: ${usd(r.cost)}`}
            className="viz-row grid w-full grid-cols-[minmax(0,40%)_1fr_auto] items-center gap-x-2 px-1 py-[5px]"
            tip={<TipBody head={nameOf(r.session)} rows={parts.map(([k, v]) => ({ value: usd(v), label: k, color: palette.colorOf(k), mark: 'rect' }))} foot={`${r.calls} model calls · open it with a click`} />}>
            <span className="truncate text-left text-[12px] text-ink-dim">{nameOf(r.session)}</span>
            <span className="flex h-3 gap-[2px]" style={{ width: `${max > 0 ? (r.cost / max) * 100 : 0}%`, minWidth: 2 }}>
              {parts.map(([k, v], i) => (
                <span key={k} className="h-full min-w-[2px]" style={{ flex: `${(v / r.cost) * 1000} 1 0`, background: palette.colorOf(k), borderRadius: i === parts.length - 1 ? '0 4px 4px 0' : 0 }} />
              ))}
            </span>
            <span className="num w-[8ch] text-right text-[11px] text-ink">{usd(r.cost)}</span>
          </TipTarget>
        )
      })}
    </TipArea>
  )
}

function sessionsTable(rows: SessionSpend[], nameOf: (sid: string) => string) {
  return {
    caption: 'every session\'s spend, by model', rows, rowKey: (r: SessionSpend) => r.session,
    columns: [
      { key: 'session', label: 'session', cell: (r: SessionSpend) => nameOf(r.session), title: (r: SessionSpend) => nameOf(r.session) },
      { key: 'models', label: 'models', cell: (r: SessionSpend) => r.byModel.map((b) => `${b.model} ${usd(b.cost)}`).join(' · ') },
      { key: 'calls', label: 'calls', num: true, cell: (r: SessionSpend) => r.calls },
      { key: 'cost', label: 'spent', num: true, cell: (r: SessionSpend) => usd(r.cost) },
    ],
  }
}

// ---------------------------------------------------------------- latency

/** A strip per model on one log axis of time: each call's first token (a ring, above the line) and its total (a dot,
 *  below), spread so they never stack, in the model's colour; each strip's p50 a tick with its value. Hovering a model's
 *  band shows its numbers. */
function LatencyStrip({ lat, palette }: { lat: Latency[]; palette: Palette }) {
  const option = useMemo<EChartsOption>(() => {
    const names = lat.map((l) => l.model)
    const vaxis = valueAxis(), axis = baseAxis()
    const dots = (which: 'first' | 'total') => lat.flatMap((l, i) => {
      const color = palette.colorOf(palette.keyOf(l.model))
      return l[which].map((p, k) => ({
        value: [Math.max(p.ms, 0.01), i], symbolOffset: [0, (which === 'first' ? -11 : 11) + jitter(k) * 12],
        itemStyle: which === 'first' ? { color: C.surface, borderColor: color, borderWidth: MARK.ring } : { color, borderColor: C.surface, borderWidth: MARK.ring },
      }))
    })
    const p50s = lat.flatMap((l, i) => [
      ...(l.firstP50 !== undefined ? [{ value: [Math.max(l.firstP50, 0.01), i], symbolOffset: [0, -11] }] : []),
      ...(l.totalP50 !== undefined ? [{ value: [Math.max(l.totalP50, 0.01), i], symbolOffset: [0, 11] }] : []),
    ])
    // The p50s' words go under the model's name, beside its strip, so they never cover a dot. Rich text reads braces and
    // bars as markup: a name loses them.
    const plain = (t: string) => t.replace(/[{}|]/g, '')
    const label = (name: string) => {
      const l = lat.find((x) => x.model === name)
      return `{name|${plain(name)}}\n{p50|first token p50 ${ms(l?.firstP50)}}\n{p50|total p50 ${ms(l?.totalP50)}}`
    }
    return {
      grid: { left: 12, right: 24, top: 8, bottom: 24, containLabel: true },
      tooltip: {
        ...TIP_FRAME, trigger: 'axis', axisPointer: { type: 'shadow', axis: 'y', shadowStyle: { color: 'rgba(176,141,87,0.06)' } },
        formatter: (ps: any) => {
          const p = Array.isArray(ps) ? ps[0] : ps
          const l = lat[names.indexOf(p?.name ?? p?.axisValue)] ?? lat[p?.value?.[1]]
          if (!l) return ''
          const color = palette.colorOf(palette.keyOf(l.model))
          return tip(l.model, [
            { value: `${ms(l.firstP50)} · ${ms(l.firstP95)}`, label: 'first token, p50 · p95', color, mark: 'ring' },
            { value: `${ms(l.totalP50)} · ${ms(l.totalP95)}`, label: 'total, p50 · p95', color, mark: 'dot' },
          ], `${l.total.length || l.first.length} timed calls`)
        },
      },
      xAxis: { ...vaxis, type: 'log', logBase: 10, axisLabel: { ...vaxis.axisLabel, formatter: msLogTick } },
      yAxis: {
        type: 'category', data: names, inverse: true, ...axis, axisLine: { show: false },
        splitLine: { show: true, lineStyle: { color: C.grid, width: 1, type: 'solid' } },
        axisLabel: {
          ...axis.axisLabel, formatter: label, lineHeight: 15,
          rich: { name: { color: C.secondary, fontFamily: FONTS.sans, fontSize: 11, width: 130, overflow: 'truncate' }, p50: { color: C.muted, fontFamily: FONTS.mono, fontSize: 9.5 } },
        },
      },
      series: [
        { type: 'scatter', name: 'first token', symbolSize: MARK.marker + MARK.ring, data: dots('first') },
        { type: 'scatter', name: 'total', symbolSize: MARK.marker + MARK.ring, data: dots('total') },
        { type: 'scatter', name: 'p50', symbol: 'rect', symbolSize: [2, 16], silent: true, z: 3, itemStyle: { color: C.text }, data: p50s },
      ],
    }
  }, [lat, palette])
  return <Echart option={option} />
}

function latencyTable(lat: Latency[]) {
  return {
    caption: 'each model\'s latency, first token and total, p50 and p95', rows: lat, rowKey: (l: Latency) => l.model,
    columns: [
      { key: 'model', label: 'model', cell: (l: Latency) => l.model },
      { key: 'calls', label: 'timed calls', num: true, cell: (l: Latency) => Math.max(l.first.length, l.total.length) },
      { key: 'f50', label: 'first token p50', num: true, cell: (l: Latency) => ms(l.firstP50) },
      { key: 'f95', label: 'p95', num: true, cell: (l: Latency) => ms(l.firstP95) },
      { key: 't50', label: 'total p50', num: true, cell: (l: Latency) => ms(l.totalP50) },
      { key: 't95', label: 'p95', num: true, cell: (l: Latency) => ms(l.totalP95) },
    ],
  }
}

// ---------------------------------------------------------------- caching

/** The provider's prompt cache, by profile: of each input, the share read from cache, the tokens written to it, and the
 *  dollars it saved at the catalog's prices, net of what its writes cost over plain input. A read costs the cache-read
 *  price instead of the input price; a write costs the cache-write price. A profile whose writes cost more than its reads
 *  saved so far says so with a mark and a word, never with colour alone. */
function CacheByProfile({ calls, prices, profileOf }: { calls: ProviderCall[]; prices: ReturnType<typeof pricing>; profileOf: Map<string, string> }) {
  const rows = useMemo(() => cacheBy(calls, (c) => profileOf.get(c.session_id ?? '') ?? '—', prices), [calls, prices, profileOf])
  if (!rows.length) return <Empty>no model call has read or written the cache yet</Empty>
  return (
    <table className="viz-table w-full text-[12px]" title="the provider's prompt cache: of each input, the share read from cache, and the dollars that saved at the catalog's prices. A read costs the cache-read price instead of the input price; a write costs the cache-write price, and its premium over the input price counts against the saving">
      <caption className="sr-only">the prompt cache by profile</caption>
      <thead>
        <tr><th className="text-left">profile</th><th className="text-right">sessions</th><th className="text-right">input</th><th className="text-right">read from cache</th><th className="text-right">written</th><th className="text-right">saved</th></tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={r.key}>
            <td className="text-ink">{r.key}</td>
            <td className="num text-right text-ink-dim">{r.sessions}</td>
            <td className="num text-right text-ink-dim">{tokens(r.input)}</td>
            <td className="num text-right text-ink">{pct(r.read / r.input, 1)}</td>
            <td className="num text-right text-ink-dim">{tokens(r.written)}</td>
            <td className="num text-right text-ink">
              {r.saved < 0
                ? <span className="inline-flex items-center gap-1" title="the cache's writes have cost more than its reads saved, so far"><AlertTriangle size={11} className="text-wait" aria-hidden />net cost −{usd(-r.saved)}</span>
                : usd(r.saved)}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}
