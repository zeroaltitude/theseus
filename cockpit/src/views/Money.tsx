// The money river (theseus-logs, round two): where every dollar went, as a river. From the sessions, through the
// profiles and models they ran on, into the token kinds they bought (input, cache reads, cache writes at five minutes
// and at an hour, output), out to the sea: the dollars spent. Beside it, the spend's pace on a brass dial, every
// budget against its limit with the calls in flight held as pools, and what the cache saved.
//
// Every figure is the ledger's: each `provider.call` row's usage and recorded cost. A call's dollars are split by
// kind at the catalog's rates, then scaled to the cost the call recorded, so the river's sea is exactly what was
// spent. The range ends at the time machine's moment when it is set, so the river shows the money as it stood then.
import { useDeferredValue, useMemo, useState } from 'react'
import { useNavigate } from 'react-router'
import { Coins, Landmark, PiggyBank, Table2, Timer, Waves } from 'lucide-react'
import type { ActionInfo, CatalogList, ExecutionInfo, Health, LedgerEntry, SessionInfo } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useTick } from '@/lib/hooks'
import { useHistoryRows } from '@/lib/history'
import { useWorld } from '@/lib/world'
import { providerCalls, type ProviderCall } from '@/lib/derive'
import { pricing, split } from '@/lib/money'
import { ink, type EChartsOption } from '@/lib/chart'
import { cn, pct, short, stamp, tokens, usd } from '@/lib/format'
import { Echart } from '@/components/Echart'
import { Empty, Kpi, Panel, Segmented } from '@/components/ui'
import { Dial, Engraved, Needle, Ticks, arc, polar } from '@/ship/instruments'

// The token kinds, in a fixed order with their validated colours (dark surface; the dataviz validator passes all
// six checks): cool and warm alternate, so neighbours stay apart for every reader.
const KINDS = [
  { key: 'input', word: 'input', c: '#3b7fdb' },
  { key: 'cacheRead', word: 'cache reads', c: '#b8862c' },
  { key: 'cacheWrite', word: 'cache writes, 5 min', c: '#8b6cf0' },
  { key: 'cacheWrite1h', word: 'cache writes, 1 hour', c: '#c2410c' },
  { key: 'output', word: 'output', c: '#0ea5c6' },
] as const
type Kind = (typeof KINDS)[number]['key']

const RANGES = ['1h', '6h', '1d', '7d', 'all'] as const
type Range = (typeof RANGES)[number]
const RANGE_MS: Record<Range, number> = { '1h': 3_600_000, '6h': 21_600_000, '1d': 86_400_000, '7d': 604_800_000, all: Infinity }
const MEASURES = ['dollars', 'tokens'] as const
type Measure = (typeof MEASURES)[number]

/** Sessions beyond this many fold into one "others" stream, so the river stays legible. */
const TOP = 10

interface Part { session: string; model: string; kind: Kind; usd: number; tokens: number }

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
    session: c.session_id ?? '—', model, kind: k.key, tokens: tok[k.key],
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
  const { data: el } = useRpc<{ executions: ExecutionInfo[] }>('execution.list', undefined, 3000)
  const { data: al } = useRpc<{ actions: ActionInfo[] }>('action.list', { n: 500 }, 3000)
  const { data: h } = useRpc<Health>('health', undefined, 10_000)
  const [range, setRange] = useState<Range>('all')
  const [measure, setMeasure] = useState<Measure>('dollars')
  const [table, setTable] = useState(false)
  const prices = useMemo(() => pricing(cat), [cat])
  const title = useMemo(() => new Map((sl?.sessions ?? []).map((s) => [s.session_id, s.title || s.label || short(s.session_id)])), [sl])

  const calls = useMemo(() => providerCalls(rows), [rows])
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

  const executions = world?.executions ?? el?.executions ?? []
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
        <Kpi label={`Spent · ${range === 'all' ? 'all time' : `last ${range}`}`} icon={<Coins size={12} />} value={totals.spent} format={(n) => usd(n)} tone="money"
          hint={range === 'all' && !world && h && Math.abs(h.cost_usd_total - totals.spent) > 0.0005
            ? `${inRange.length} calls · the sessions' totals say ${usd(h.cost_usd_total)}: the calls are the record (theseus-lluv)`
            : `${inRange.length} model calls${world ? ` · as of ${stamp(end)}` : ''}`} />
        <Kpi label="Saved by the cache" icon={<PiggyBank size={12} />} value={totals.saved} format={(n) => usd(n)} tone="ok"
          hint={totals.spent + totals.saved > 0 ? `${pct(totals.saved / (totals.spent + totals.saved))} off the uncached price` : 'no cache reads in range'} />
        <Kpi label="Pace · dollars an hour" icon={<Timer size={12} />} value={pace.perHour} format={(n) => usd(n)} tone="live"
          hint={`over the 15 minutes before ${world ? stamp(end) : 'now'} · ${usd(pace.lastHour)} in the last hour`} />
        <Kpi label="Held by calls in flight" icon={<Landmark size={12} />} value={held} format={(n) => usd(n)} tone="wait"
          hint={callsHeld ? `${callsHeld} call${callsHeld === 1 ? '' : 's'} reserved and not settled` : 'nothing reserved now'} />
        <Kpi label="Tokens in range" icon={<Waves size={12} />} value={totals.tok} format={tokens} tone="model"
          hint={inRange.length ? `${tokens(totals.tok / inRange.length)} a call` : undefined} />
      </div>

      <div className="grid grid-cols-1 gap-3 2xl:grid-cols-[1fr_400px]">
        <Panel title={<>The money river · sessions → profiles and models → token kinds → the sea</>} icon={<Waves size={13} />}
          bodyClassName="h-[560px] p-2"
          actions={<>
            <Segmented value={measure} options={MEASURES} onChange={setMeasure} />
            <Segmented value={range} options={RANGES} onChange={setRange} />
            <button type="button" onClick={() => setTable((v) => !v)} title="The river as a table" className={cn('rounded p-1 hover:bg-gold/10', table ? 'text-live' : 'text-ink-faint')}><Table2 size={14} /></button>
          </>}>
          {parts.length
            ? (table ? <RiverTable parts={parts} title={title} measure={measure} /> : <River parts={parts} title={title} measure={measure} onPick={(sid) => nav(`/session/${sid}`)} />)
            : <Empty>{rows.length ? 'no model calls in this range' : 'reading the ledger…'}</Empty>}
        </Panel>
        <div className="flex min-w-0 flex-col gap-3">
          <Panel title="The pace" icon={<Timer size={13} />} bodyClassName="speed-wall flex items-center justify-around px-2 py-3">
            <PaceDial perHour={pace.perHour} />
            <div className="num flex flex-col gap-1 text-[12px]">
              <KindKey />
            </div>
          </Panel>
          <Panel title={<>Budgets · spent, held, and left</>} icon={<Landmark size={13} />} bodyClassName="max-h-[372px] overflow-auto p-2">
            <Budgets executions={executions} reserved={reservedBy} title={title} onPick={(sid) => nav(`/session/${sid}`)} />
          </Panel>
        </div>
      </div>
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

function River({ parts, title, measure, onPick }: { parts: Part[]; title: Map<string, string>; measure: Measure; onPick: (sid: string) => void }) {
  const option = useMemo<EChartsOption>(() => {
    const f = fold(parts, measure)
    const total = [...f.flows.entries()].filter(([k]) => k.endsWith('\u0000sea')).reduce((a, [, x]) => a + x, 0)
    const kindOf = new Map<string, (typeof KINDS)[number]>(KINDS.map((k) => [`k:${k.key}`, k]))
    const names = new Set<string>()
    for (const k of f.flows.keys()) { const [a, b] = k.split('\u0000'); names.add(a); names.add(b) }
    const label = (n: string) => {
      if (n === 'sea') return `the sea · ${fmt(measure, total)}`
      if (n === 's:others') return `${f.others} other session${f.others === 1 ? '' : 's'}`
      if (n.startsWith('s:')) return (title.get(n.slice(2)) ?? short(n.slice(2))).slice(0, 34)
      if (n.startsWith('m:')) return n.slice(2)
      return kindOf.get(n)?.word ?? n
    }
    const depth = (n: string) => (n.startsWith('s:') ? 0 : n.startsWith('m:') ? 1 : n.startsWith('k:') ? 2 : 3)
    const color = (n: string) => (n === 'sea' ? '#d6a548' : n.startsWith('k:') ? kindOf.get(n)!.c : n.startsWith('m:') ? '#d9cba8' : '#c9a467')
    const valueOf = new Map<string, number>()
    for (const [k, x] of f.flows) { const [a, b] = k.split('\u0000'); valueOf.set(b, (valueOf.get(b) ?? 0) + x); if (depth(a) === 0) valueOf.set(a, (valueOf.get(a) ?? 0) + x) }
    return {
      tooltip: {
        trigger: 'item',
        formatter: (p: any) => {
          if (p.dataType === 'edge') {
            const share = total > 0 ? ` · ${pct(p.data.value / total, 1)} of the sea` : ''
            return `${label(p.data.source)} → ${label(p.data.target)}<br/><b>${fmt(measure, p.data.value)}</b>${share}`
          }
          return `<b>${label(p.name)}</b><br/>${fmt(measure, valueOf.get(p.name) ?? 0)}`
        },
      },
      series: [{
        type: 'sankey',
        left: 8, right: 150, top: 10, bottom: 10,
        nodeWidth: 12, nodeGap: 9, nodeAlign: 'justify', layoutIterations: 48, draggable: false,
        emphasis: { focus: 'adjacency' },
        data: [...names].map((n) => ({
          name: n, depth: depth(n),
          itemStyle: { color: color(n), borderColor: 'rgba(3,9,18,0.9)', borderWidth: 1 },
          label: {
            // A stream too thin to hold its name keeps it in the tooltip, so names never pile up.
            show: n === 'sea' || (valueOf.get(n) ?? 0) >= total * 0.012,
            formatter: () => label(n), color: n === 'sea' ? '#f3d9a4' : depth(n) === 2 ? ink.bright : ink.text,
            fontFamily: depth(n) === 3 ? "'Cinzel Variable', serif" : "'Inter Variable', sans-serif",
            fontSize: n === 'sea' ? 13 : 11, fontWeight: n === 'sea' || depth(n) === 2 ? 600 : 400,
          },
        })),
        links: [...f.flows.entries()].map(([k, value]) => {
          const [source, target] = k.split('\u0000')
          const kind = kindOf.get(target) ?? kindOf.get(source)
          // Into the kinds and out to the sea, each stream wears its kind; from a session to its model, brass turning ivory.
          return { source, target, value, lineStyle: { color: kind ? kind.c : 'gradient', opacity: kind ? 0.42 : 0.36, curveness: 0.5 } }
        }),
      }],
    }
  }, [parts, title, measure])
  return <Echart option={option} onClick={(p: any) => { if (p?.dataType === 'node' && typeof p.name === 'string' && p.name.startsWith('s:') && p.name !== 's:others') onPick(p.name.slice(2)) }} />
}

/** The same streams as a table: each session's spend by kind (the river's accessible twin). */
function RiverTable({ parts, title, measure }: { parts: Part[]; title: Map<string, string>; measure: Measure }) {
  const rows = useMemo(() => {
    const m = new Map<string, Record<string, number>>()
    for (const p of parts) {
      const r = m.get(p.session) ?? {}
      const x = measure === 'dollars' ? p.usd : p.tokens
      r[p.kind] = (r[p.kind] ?? 0) + x
      r.total = (r.total ?? 0) + x
      m.set(p.session, r)
    }
    return [...m.entries()].sort((a, b) => (b[1].total ?? 0) - (a[1].total ?? 0))
  }, [parts, measure])
  return (
    <div className="h-full overflow-auto">
      <table className="w-full whitespace-nowrap text-[12px]">
        <thead className="sticky top-0 bg-hull/95 text-[10px] uppercase tracking-wider text-ink-faint">
          <tr><th className="px-2 py-1.5 text-left">session</th>{KINDS.map((k) => <th key={k.key} className="px-2 py-1.5 text-right"><span className="mr-1 inline-block h-2 w-2 rounded-sm" style={{ background: k.c }} />{k.word}</th>)}<th className="px-2 py-1.5 text-right">total</th></tr>
        </thead>
        <tbody>
          {rows.map(([sid, r]) => (
            <tr key={sid} className="border-t border-line/50">
              <td className="max-w-[260px] truncate px-2 py-1 text-ink">{title.get(sid) ?? short(sid)}</td>
              {KINDS.map((k) => <td key={k.key} className="num px-2 py-1 text-right text-ink-dim">{r[k.key] ? fmt(measure, r[k.key]) : '—'}</td>)}
              <td className="num px-2 py-1 text-right text-ink">{fmt(measure, r.total ?? 0)}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  )
}

function KindKey() {
  return (
    <ul className="space-y-1">
      {KINDS.map((k) => (
        <li key={k.key} className="flex items-center gap-2 text-ink-dim">
          <span className="inline-block h-2.5 w-5 rounded-sm" style={{ background: k.c }} />{k.word}
        </li>
      ))}
      <li className="flex items-center gap-2 pt-0.5 text-ink-dim"><span className="inline-block h-2.5 w-5 rounded-sm bg-gold" />the sea: dollars spent</li>
    </ul>
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

/** Each execution's budget: spent in gold, held by calls in flight as an amber pool, and what is left. */
function Budgets({ executions, reserved, title, onPick }: { executions: ExecutionInfo[]; reserved: Map<string, number>; title: Map<string, string>; onPick: (sid: string) => void }) {
  const list = useMemo(() => executions
    .map((e) => ({ e, held: reserved.get(e.execution_id) ?? e.budget.reserved_usd }))
    .filter(({ e, held }) => e.budget.spent_usd > 0 || held > 0)
    .sort((a, b) => b.e.budget.spent_usd + b.held - (a.e.budget.spent_usd + a.held)), [executions, reserved])
  if (!list.length) return <Empty>nothing spent against a budget</Empty>
  return (
    <div className="flex flex-col gap-2">
      {list.map(({ e, held }) => {
        const lim = Math.max(e.budget.limit_usd, 1e-9)
        const spentF = Math.min(1, e.budget.spent_usd / lim)
        const heldF = Math.min(1 - spentF, held / lim)
        return (
          <button type="button" key={e.execution_id} onClick={() => onPick(e.session_id)} className="rounded-md px-1.5 py-1 text-left hover:bg-gold/[0.06]">
            <div className="flex items-baseline gap-2 text-[12px]">
              <span className="min-w-0 flex-1 truncate text-ink">{title.get(e.session_id) ?? short(e.session_id)}</span>
              {e.kind === 'task' && <span className="text-[10px] text-ink-faint">task</span>}
              <span className="num text-[11px] text-ink-dim">{usd(e.budget.spent_usd)} of {usd(e.budget.limit_usd)}</span>
            </div>
            <div className="relative mt-1 h-2.5 overflow-hidden rounded-full bg-black/45 shadow-[inset_0_0_0_1px_rgba(176,141,87,0.28)]" role="img"
              aria-label={`spent ${usd(e.budget.spent_usd)}, held ${usd(held)}, limit ${usd(e.budget.limit_usd)}`}>
              <div className="absolute inset-y-0 left-0 rounded-l-full" style={{ width: `${spentF * 100}%`, background: 'linear-gradient(90deg,#9c6f23,#d6a548)', boxShadow: '0 0 8px rgba(214,165,72,0.45)' }} />
              {heldF > 0 && (
                <div className="absolute inset-y-0" title={`${usd(held)} held by calls in flight`}
                  style={{ left: `${spentF * 100}%`, width: `${Math.max(heldF * 100, 1.5)}%`, background: 'repeating-linear-gradient(135deg, rgba(251,191,36,0.85) 0 3px, rgba(251,191,36,0.35) 3px 6px)' }} />
              )}
            </div>
            <div className="num mt-0.5 flex gap-3 text-[10.5px] text-ink-faint">
              {held > 0 && <span className="text-wait">{usd(held)} held</span>}
              <span>{usd(Math.max(0, e.budget.limit_usd - e.budget.spent_usd - held))} left</span>
              {e.budget.resets > 0 && <span>{e.budget.resets} reset{e.budget.resets === 1 ? '' : 's'}</span>}
            </div>
          </button>
        )
      })}
    </div>
  )
}
