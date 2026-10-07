// The money river (theseus-logs, round two): where every dollar went, as a river. From the sessions, through the
// profiles and models they ran on, into the token kinds they bought (input, cache reads, cache writes at five minutes
// and at an hour, output), out to the sea: the dollars spent. Beside it, the spend's pace on a brass dial, every
// budget against its limit with the calls in flight held as pools, and what the cache saved.
//
// Every figure is the ledger's: each `provider.call` row's usage and recorded cost. A call's dollars are split by
// kind at the catalog's rates, then scaled to the cost the call recorded, so the river's sea is exactly what was
// spent. The range ends at the time machine's moment when it is set, so the river shows the money as it stood then.
import { useDeferredValue, useEffect, useMemo } from 'react'
import { useNavigate, useSearchParams } from 'react-router'
import { Coins, Landmark, PiggyBank, Timer, Waves } from 'lucide-react'
import type { ActionInfo, CatalogList, Health, LedgerEntry, SessionInfo } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { useHistoryRows } from '@/lib/history'
import { useWorld } from '@/lib/world'
import { providerCalls, type ProviderCall } from '@/lib/derive'
import { pricing, split } from '@/lib/money'
import type { EChartsOption } from '@/lib/chart'
import { clock, pct, short, stamp, tokens, usd } from '@/lib/format'
import { CHROME, FONTS, OTHER, TIP_FRAME, TOKEN_KINDS, slots, slotColor } from '@/lib/viz'
import { tip } from '@/lib/viztip'
import { useWidth } from '@/lib/chartview'
import { Echart } from '@/components/Echart'
import { ChartPanel, StatTile, TipArea, TipBody, TipTarget, type LegendItem, type TableSpec } from '@/components/ChartPanel'
import { Segmented } from '@/components/ui'
import { Budgets } from '@/components/Budgets'
import { Dial, Engraved, Needle, Ticks, arc, polar } from '@/ship/instruments'

// The token kinds in their fixed order and validated colours (`lib/viz.ts`: they pass the method's checks on both panel
// faces), Economics' and the Bridge's too.
const KINDS = TOKEN_KINDS.map((k) => ({ key: k.key, word: k.word, c: k.color }))
const C = CHROME.dark
/** The sea: the dollars spent, the theme's gold. */
const SEA = '#d6a548'
type Kind = (typeof TOKEN_KINDS)[number]['key']

const RANGES = ['1h', '6h', '1d', '7d', 'all'] as const
type Range = (typeof RANGES)[number]
const RANGE_MS: Record<Range, number> = { '1h': 3_600_000, '6h': 21_600_000, '1d': 86_400_000, '7d': 604_800_000, all: Infinity }
const MEASURES = ['dollars', 'tokens'] as const
type Measure = (typeof MEASURES)[number]

/** Sessions beyond this many fold into one "others" stream, so the river stays legible. */
const TOP = 10

/** A call's dollars and tokens of one kind; `model` is the river's stream (its profile and model), `name` the model. */
interface Part { session: string; model: string; name: string; kind: Kind; usd: number; tokens: number }

/** A call into its five parts: dollars at the catalog's rates, scaled to the cost the call recorded. */
function partsOf(c: ProviderCall, profile: string | undefined, prices: ReturnType<typeof pricing>): Part[] {
  const u = c.usage
  const w1h = Math.min(u.cache_creation_1h_input_tokens ?? 0, u.cache_creation_input_tokens)
  const tok: Record<Kind, number> = {
    input: u.input_tokens, cacheRead: u.cache_read_input_tokens, cacheWrite: u.cache_creation_input_tokens - w1h, cacheWrite1h: w1h, output: u.output_tokens,
  }
  const s = split(c, prices.get(c.model))
  const at: Record<Kind, number> = { input: s.input, cacheRead: s.cacheRead, cacheWrite: s.cacheWrite, cacheWrite1h: s.cacheWrite1h, output: s.output }
  const sum = KINDS.reduce((a, k) => a + at[k.key], 0)
  const allTok = KINDS.reduce((a, k) => a + tok[k.key], 0)
  // Scaled to the recorded cost; a model the catalog no longer prices shares its cost by tokens.
  const scale = sum > 0 ? c.cost / sum : 0
  const model = `${profile ? `${profile} · ` : ''}${c.model}`
  return KINDS.map((k) => ({
    session: c.session_id ?? '—', model, name: c.model, kind: k.key, tokens: tok[k.key],
    usd: sum > 0 ? at[k.key] * scale : allTok > 0 ? (c.cost * tok[k.key]) / allTok : 0,
  }))
}

export default function Money() {
  const nav = useNavigate()
  const tick = useTick(10_000)
  // Deferred: the river lays itself out again for each moment, and a scrub's needle never waits for it.
  const world = useDeferredValue(useWorld())
  const end = world?.t ?? tick
  const { rows } = useHistoryRows()
  const { data: cat } = useRpc<CatalogList>('catalog.list', undefined, 60_000)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 5000)
  const { data: al } = useRpc<{ actions: ActionInfo[] }>('action.list', { n: 500 }, 3000)
  const { data: h } = useRpc<Health>('health', undefined, 10_000)
  // The river's range, measure, and table live in the address (?range=1h&measure=tokens&table=river).
  const [params, setParams] = useSearchParams()
  const range: Range = (RANGES as readonly string[]).includes(params.get('range') ?? '') ? (params.get('range') as Range) : 'all'
  const measure: Measure = params.get('measure') === 'tokens' ? 'tokens' : 'dollars'
  const put = (k: string, v: string | null) => setParams((p) => { if (v) p.set(k, v); else p.delete(k); return p }, { replace: true })
  const setRange = (r: Range) => put('range', r === 'all' ? null : r)
  const setMeasure = (m: Measure) => put('measure', m === 'dollars' ? null : m)
  // An address from before the chart method said `table=1` for the river's table: it still opens it.
  useEffect(() => { if (params.get('table') === '1') put('table', 'river') })
  const prices = useMemo(() => pricing(cat), [cat])
  const title = useMemo(() => new Map((sl?.sessions ?? []).map((s) => [s.session_id, s.title || s.label || short(s.session_id)])), [sl])

  const calls = useMemo(() => providerCalls(rows), [rows])
  // A model's colour is its slot over the whole record, in the order the record first names it: Economics' colours.
  const modelSlot = useMemo(() => slots(calls.map((c) => c.model)).slot, [calls])
  const profileOf = useMemo(() => profilesByTurn(rows), [rows])
  const start = range === 'all' ? 0 : end - RANGE_MS[range]
  const inRange = useMemo(() => calls.filter((c) => c.at > start && c.at <= end), [calls, start, end])

  const parts = useMemo(() => inRange.flatMap((c) => partsOf(c, c.turn_id ? profileOf.get(c.turn_id) : undefined, prices)), [inRange, profileOf, prices])
  const totals = useMemo(() => {
    let spent = 0, saved = 0, tok = 0
    for (const c of inRange) {
      spent += c.cost
      saved += split(c, prices.get(c.model)).saved
    }
    for (const p of parts) tok += p.tokens
    return { spent, saved, tok }
  }, [inRange, parts, prices])
  // The pace: dollars an hour over the 15 minutes before the range's end.
  const pace = useMemo(() => {
    let last15 = 0, lastHour = 0
    for (const c of calls) {
      if (c.at > end || c.at <= end - 3_600_000) continue
      lastHour += c.cost
      if (c.at > end - 900_000) last15 += c.cost
    }
    return { perHour: last15 * 4, lastHour }
  }, [calls, end])

  const reservedBy = useMemo(() => {
    if (world) return world.reserved
    const m = new Map<string, number>()
    for (const a of al?.actions ?? []) if (!a.settled_at_ms && a.reserved_usd > 0) m.set(a.execution_id, (m.get(a.execution_id) ?? 0) + a.reserved_usd)
    return m
  }, [world, al])
  const held = [...reservedBy.values()].reduce((a, b) => a + b, 0)
  const callsHeld = world ? world.actions.filter((a) => !a.settled_at_ms && a.reserved_usd > 0).length : (al?.actions ?? []).filter((a) => !a.settled_at_ms && a.reserved_usd > 0).length

  return (
    <div className="flex flex-col gap-3">
      <div className="grid grid-cols-2 gap-3 md:grid-cols-3 xl:grid-cols-5">
        <StatTile label={`Spent · ${range === 'all' ? 'all time' : `last ${range}`}`} icon={<Coins size={12} />} value={totals.spent} format={(n) => usd(n)} tone="money"
          hint={range === 'all' && !world && h && Math.abs(h.cost_usd_total - totals.spent) > 0.0005
            ? `${inRange.length} calls · the sessions' totals say ${usd(h.cost_usd_total)}: the calls are the record (theseus-lluv)`
            : `${inRange.length} model calls${world ? ` · as of ${stamp(end)}` : ''}`} />
        <StatTile label="Saved by the cache" icon={<PiggyBank size={12} />} value={totals.saved} format={(n) => usd(n)} tone="ok"
          hint={totals.spent + totals.saved > 0 ? `${pct(totals.saved / (totals.spent + totals.saved))} off the uncached price` : 'no cache reads in range'} />
        <StatTile label="Pace · dollars an hour" icon={<Timer size={12} />} value={pace.perHour} format={(n) => usd(n)} tone="live"
          hint={`over the 15 minutes before ${world ? stamp(end) : 'now'} · ${usd(pace.lastHour)} in the last hour`} />
        <StatTile label="Held by calls in flight" icon={<Landmark size={12} />} value={held} format={(n) => usd(n)} tone={held > 0 ? 'wait' : undefined}
          hint={callsHeld ? `${callsHeld} call${callsHeld === 1 ? '' : 's'} reserved and not settled` : 'nothing reserved now'} />
        <StatTile label="Tokens in range" icon={<Waves size={12} />} value={totals.tok} format={tokens} tone="model"
          hint={inRange.length ? `${tokens(totals.tok / inRange.length)} a call` : undefined} />
      </div>

      <div className="grid grid-cols-1 gap-3 2xl:grid-cols-[1fr_360px]">
        <ChartPanel id="river" title={<>The money river · sessions → profiles and models → token kinds → the sea</>} icon={<Waves size={13} />} height={560}
          actions={<>
            <Segmented value={measure} options={MEASURES} onChange={setMeasure} />
            <Segmented value={range} options={RANGES} onChange={setRange} />
          </>}
          legend={RIVER_LEGEND} table={riverTable(parts, title, measure)}
          empty={parts.length ? undefined : rows.length ? 'no model calls in this range' : 'reading the ledger…'}>
          <River parts={parts} title={title} measure={measure} slot={modelSlot} onPick={(sid) => nav(`/session/${sid}`)} />
        </ChartPanel>
        <ChartPanel id="pace" title="The pace" icon={<Timer size={13} />} height={560} table={lastHourTable(calls, end)}>
          <div className="speed-wall flex h-full flex-col items-center justify-center gap-2 px-3 py-4">
            <PaceDial perHour={pace.perHour} />
            <div className="max-w-[34ch] text-center text-[12px] leading-snug text-ink-dim">
              {usd(pace.perHour)} an hour, from the model calls of the 15 minutes before {world ? stamp(end) : 'now'}; {usd(pace.lastHour)} in the last hour
            </div>
            <LastHour calls={calls} end={end} now={!world} />
          </div>
        </ChartPanel>
      </div>

      <Budgets rows={rows} past={world ? world.t : null} />
    </div>
  )
}

/** turn id → the profile it ran on (`turn.started` rows). */
function profilesByTurn(rows: LedgerEntry[]): Map<string, string> {
  const m = new Map<string, string>()
  for (const r of rows) {
    if (r.kind !== 'turn.started' || !r.turn_id) continue
    const p = ((r.data ?? {}) as { profile?: string }).profile
    if (p) m.set(r.turn_id, p)
  }
  return m
}

interface Folded { sessions: string[]; others: number; flows: Map<string, number> }

/** The streams to draw: the top sessions by the measure, the rest as one, and every link's value. */
function fold(parts: Part[], measure: Measure): Folded {
  const v = (p: Part) => (measure === 'dollars' ? p.usd : p.tokens)
  const bySession = new Map<string, number>()
  for (const p of parts) bySession.set(p.session, (bySession.get(p.session) ?? 0) + v(p))
  const ranked = [...bySession.entries()].filter(([, x]) => x > 0).sort((a, b) => b[1] - a[1])
  // The biggest streams by themselves (at least three); a session under 3% of the river joins "others".
  const all = ranked.reduce((a, [, x]) => a + x, 0)
  const top = ranked.filter(([, x], i) => i < 3 || (i < TOP && x >= all * 0.03)).map(([s]) => s)
  const keep = new Set(top)
  const flows = new Map<string, number>()
  const add = (a: string, b: string, x: number) => { if (x > 0) flows.set(`${a}\u0000${b}`, (flows.get(`${a}\u0000${b}`) ?? 0) + x) }
  for (const p of parts) {
    const x = v(p)
    if (x <= 0) continue
    const s = keep.has(p.session) ? `s:${p.session}` : 's:others'
    add(s, `m:${p.model}`, x)
    add(`m:${p.model}`, `k:${p.kind}`, x)
    add(`k:${p.kind}`, 'sea', x)
  }
  return { sessions: top, others: ranked.length - top.length, flows }
}

const fmt = (measure: Measure, x: number) => (measure === 'dollars' ? usd(x) : `${tokens(x)} tokens`)

/** The river's key: the five token kinds, and the sea. */
const RIVER_LEGEND: LegendItem[] = [
  ...KINDS.map((k) => ({ key: k.key, label: k.word, color: k.c, mark: 'rect' as const })),
  { key: 'sea', label: 'the sea: the dollars spent', color: SEA, mark: 'rect' },
]

/** The river after the chart method: the sessions in the de-emphasis gray, each profile and model in its model's slot
 *  (Economics' colours), the token kinds in theirs, the sea in the theme's gold; every name in the ink, in the sans; a
 *  stream too thin to hold its name keeps it in the tip and the table, and a session's name ends in an ellipsis where it
 *  would run into the next column, whole in both. */
function River({ parts, title, measure, slot, onPick }: { parts: Part[]; title: Map<string, string>; measure: Measure; slot: Map<string, number>; onPick: (sid: string) => void }) {
  const [ref, width] = useWidth<HTMLDivElement>()
  const option = useMemo<EChartsOption>(() => {
    const f = fold(parts, measure)
    const total = [...f.flows.entries()].filter(([k]) => k.endsWith('\u0000sea')).reduce((a, [, x]) => a + x, 0)
    const kindOf = new Map<string, (typeof KINDS)[number]>(KINDS.map((k) => [`k:${k.key}`, k]))
    const modelName = new Map(parts.map((p) => [`m:${p.model}`, p.name]))
    const names = new Set<string>()
    for (const k of f.flows.keys()) { const [a, b] = k.split('\u0000'); names.add(a); names.add(b) }
    const label = (n: string) => {
      if (n === 'sea') return `the sea · ${fmt(measure, total)}`
      if (n === 's:others') return `${f.others} other session${f.others === 1 ? '' : 's'}`
      if (n.startsWith('s:')) return title.get(n.slice(2)) ?? short(n.slice(2))
      if (n.startsWith('m:')) return n.slice(2)
      return kindOf.get(n)?.word ?? n
    }
    const depth = (n: string) => (n.startsWith('s:') ? 0 : n.startsWith('m:') ? 1 : n.startsWith('k:') ? 2 : 3)
    const modelColor = (n: string) => { const m = modelName.get(n); return m === undefined ? OTHER : slotColor(slot, m) }
    const color = (n: string) => (n === 'sea' ? SEA : n.startsWith('k:') ? kindOf.get(n)!.c : n.startsWith('m:') ? modelColor(n) : OTHER)
    const valueOf = new Map<string, number>()
    for (const [k, x] of f.flows) { const [a, b] = k.split('\u0000'); valueOf.set(b, (valueOf.get(b) ?? 0) + x); if (depth(a) === 0) valueOf.set(a, (valueOf.get(a) ?? 0) + x) }
    // A session's words stop before the models' column: about a third of the river's width.
    const room = Math.max(90, ((width || 1200) - 150) / 3 - 24)
    const share = (v: number) => (total > 0 ? `${pct(v / total, 1)} of the sea` : undefined)
    return {
      tooltip: {
        ...TIP_FRAME, trigger: 'item',
        formatter: (p: any) => {
          if (p.dataType === 'edge') {
            return tip(`${label(p.data.source)} → ${label(p.data.target)}`, [{ value: fmt(measure, p.data.value), label: 'along this stream', color: p.data.lineStyle?.color, mark: 'line' }], share(p.data.value))
          }
          const kind = p.name === 'sea' ? 'the sea' : depth(p.name) === 0 ? 'a session' : depth(p.name) === 1 ? 'a profile and its model' : 'a token kind'
          return tip(kind, [{ value: fmt(measure, valueOf.get(p.name) ?? 0), label: label(p.name), color: color(p.name), mark: 'rect' }],
            `${share(valueOf.get(p.name) ?? 0) ?? ''}${p.name.startsWith('s:') && p.name !== 's:others' ? ' · a click opens the session' : ''}`)
        },
      },
      series: [{
        type: 'sankey',
        left: 8, right: 150, top: 10, bottom: 10,
        nodeWidth: 12, nodeGap: 9, nodeAlign: 'justify', layoutIterations: 48, draggable: false,
        emphasis: { focus: 'adjacency' },
        data: [...names].map((n) => ({
          name: n, depth: depth(n),
          itemStyle: { color: color(n), borderColor: C.surface, borderWidth: 1 },
          label: {
            // A stream too thin to hold its name keeps it in the tooltip, so names never pile up.
            show: n === 'sea' || (valueOf.get(n) ?? 0) >= total * 0.012,
            formatter: () => label(n), color: n === 'sea' ? C.text : depth(n) === 2 ? C.text : C.secondary,
            fontFamily: FONTS.sans, fontSize: n === 'sea' ? 13 : 11, fontWeight: n === 'sea' || depth(n) === 2 ? 600 : 400,
            ...(depth(n) === 0 ? { width: room, overflow: 'truncate' as const, ellipsis: '…' } : {}),
          },
        })),
        links: [...f.flows.entries()].map(([k, value]) => {
          const [source, target] = k.split('\u0000')
          const kind = kindOf.get(target) ?? kindOf.get(source)
          // Into the kinds and out to the sea, each stream wears its kind; from a session to its model, the model's colour.
          return { source, target, value, lineStyle: { color: kind ? kind.c : modelColor(target), opacity: kind ? 0.42 : 0.3, curveness: 0.5 } }
        }),
      }],
    }
  }, [parts, title, measure, slot, width])
  return <div ref={ref} className="h-full w-full"><Echart option={option} onClick={(p: any) => { if (p?.dataType === 'node' && typeof p.name === 'string' && p.name.startsWith('s:') && p.name !== 's:others') onPick(p.name.slice(2)) }} /></div>
}

/** The same streams as a table: each session's spend (or tokens) by kind, and its total, the most first. */
function riverTable(parts: Part[], title: Map<string, string>, measure: Measure): TableSpec<[string, Record<string, number>]> {
  const m = new Map<string, Record<string, number>>()
  for (const p of parts) {
    const r = m.get(p.session) ?? {}
    const x = measure === 'dollars' ? p.usd : p.tokens
    r[p.kind] = (r[p.kind] ?? 0) + x
    r.total = (r.total ?? 0) + x
    m.set(p.session, r)
  }
  type R = [string, Record<string, number>]
  const name = (sid: string) => (sid === '—' ? 'no session' : title.get(sid) ?? short(sid))
  return {
    caption: `each session's ${measure} by token kind, the most first`, rows: [...m.entries()].sort((a, b) => (b[1].total ?? 0) - (a[1].total ?? 0)), rowKey: (r: R) => r[0],
    columns: [
      { key: 'session', label: 'session', cell: (r: R) => name(r[0]), title: (r: R) => name(r[0]) },
      ...KINDS.map((k) => ({ key: k.key, label: k.word, num: true, cell: (r: R) => (r[1][k.key] ? fmt(measure, r[1][k.key]) : '—') })),
      { key: 'total', label: 'total', num: true, cell: (r: R) => fmt(measure, r[1].total ?? 0) },
    ],
  }
}

/** The last hour's twelve five-minute bins, oldest first. */
function lastHourBins(calls: ProviderCall[], end: number): number[] {
  const b = new Array<number>(12).fill(0)
  for (const c of calls) if (c.at <= end && c.at > end - 3_600_000) b[Math.min(11, Math.floor((c.at - (end - 3_600_000)) / 300_000))] += c.cost
  return b
}

function lastHourTable(calls: ProviderCall[], end: number): TableSpec<number> {
  const bins = lastHourBins(calls, end)
  const hm = (t: number) => clock(t).slice(0, 5)
  return {
    caption: 'the last hour\'s spend, five minutes a row, the newest first', rows: bins.map((_, i) => i).reverse(), rowKey: (i: number) => String(i),
    columns: [
      { key: 'from', label: 'from', cell: (i: number) => hm(end - 3_600_000 + i * 300_000) },
      { key: 'to', label: 'to', cell: (i: number) => hm(end - 3_600_000 + (i + 1) * 300_000) },
      { key: 'spent', label: 'spent', num: true, cell: (i: number) => usd(bins[i]) },
    ],
  }
}

/** The last hour's spend, five minutes a column, in the sea's gold: the pace's own history, each column's dollars on
 *  hover and focus. */
function LastHour({ calls, end, now }: { calls: ProviderCall[]; end: number; now: boolean }) {
  const bins = useMemo(() => lastHourBins(calls, end), [calls, end])
  const max = Math.max(...bins)
  const hm = (t: number) => clock(t).slice(0, 5)
  return (
    <TipArea className="mt-1 w-full px-2">
      <div className="mb-1.5 text-center text-[11px] text-ink-faint">the last hour, five minutes a column</div>
      <div className="flex h-16 items-end gap-[2px]" role="group" aria-label="the last hour's spend, five minutes a column">
        {bins.map((v, i) => {
          const from = end - 3_600_000 + i * 300_000
          return (
            <TipTarget key={i} className="viz-mark flex h-full flex-1 items-end" label={`${hm(from)} to ${hm(from + 300_000)}: ${usd(v)}`}
              tip={<TipBody head={`${hm(from)} – ${hm(from + 300_000)}`} rows={[{ value: usd(v), label: 'spent', color: SEA, mark: 'rect' }]} />}>
              <span className="block w-full" style={{ height: `${max > 0 ? (v / max) * 100 : 0}%`, minHeight: v > 0 ? 2 : 0, background: SEA, borderRadius: '3px 3px 0 0' }} />
            </TipTarget>
          )
        })}
      </div>
      <div className="num mt-0.5 flex justify-between border-t border-line pt-0.5 text-[10px] text-ink-faint">
        <span>{hm(end - 3_600_000)}</span><span>{max > 0 ? `the tallest ${usd(max)}` : 'nothing spent'}</span><span>{now ? 'now' : hm(end)}</span>
      </div>
    </TipArea>
  )
}

/** The pace on a brass dial: dollars an hour, on a scale that grows with it (a dollar, ten, a hundred). */
function PaceDial({ perHour }: { perHour: number }) {
  const max = perHour <= 1 ? 1 : perHour <= 10 ? 10 : perHour <= 100 ? 100 : Math.ceil(perHour / 100) * 100
  const deg = -120 + (Math.min(perHour, max) / max) * 240
  return (
    <Dial title={`The spend's pace: ${usd(perHour)} an hour, from the model calls of the last 15 minutes.`} label="Pace" sub={`${usd(perHour)} / hour`} glow={perHour > 0 ? '#22d3ee' : undefined}>
      {() => (
        <g>
          <path d={arc(-120, 120, 44)} fill="none" stroke="#2b3d52" strokeWidth="4" />
          <path d={arc(-120, -120 + Math.max(0.5, (Math.min(perHour, max) / max) * 240), 44)} fill="none" stroke="#d6a548" strokeOpacity="0.8" strokeWidth="4" />
          <Ticks a0={-120} a1={120} n={10} major={5} r={41} />
          {[0, 0.5, 1].map((f) => {
            const [x, y] = polar(-120 + f * 240, 29)
            return <Engraved key={f} x={x} y={y + 2.3} size={6.6}>{`$${(max * f).toFixed(max < 10 ? 1 : 0)}`}</Engraved>
          })}
          <Engraved x={60} y={86} size={5.6} color="#b8a77f">PER HOUR</Engraved>
          <Needle deg={deg} color="#22d3ee" len={38} />
        </g>
      )}
    </Dial>
  )
}
