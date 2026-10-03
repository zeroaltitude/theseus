// The speed wall (theseus-logs, round two): "fast is a contract", made visible. The README's budgets, each a brass
// dial with its red line, read from the daemon's own record:
//
// - the start: health's phases for the last one, and every start since from its `server.serving` row;
// - each turn's harness overhead, from its trace (the turn's time less its model and tool time), split into the
//   disk's commits (the write path, one fsync a frame) and the rest;
// - the stops, restarts, swaps, and frames per turn: the gates' benches, from the history they append
//   (`bench.history`), with every gate's numbers over time;
// - frames per turn live too: each turn's trace counts its own (theseus-wz4y), and the dial reads the plain turns'
//   (one loop, no tool call) beside the bench's.
//
// Without the bench history the wall shows the live numbers alone, and says so.
import { useMemo, useState } from 'react'
import { useNavigate } from 'react-router'
import { Gauge as GaugeIcon, HardDrive, History, Rocket, Timer } from 'lucide-react'
import type { BenchHistoryResult, BenchRun, Health, LedgerEntry, SessionInfo, Span, StartupPhase } from '@protocol'
import { useRpc } from '@/lib/rpc'
import { useHistoryRows } from '@/lib/history'
import { quantile } from '@/lib/derive'
import { axisStyle, ink, type EChartsOption } from '@/lib/chart'
import { cn, ms, short, stamp } from '@/lib/format'
import { Echart } from '@/components/Echart'
import { Empty, Panel, Segmented } from '@/components/ui'
import { Dial, Engraved, Needle, Ticks, arc, polar } from '@/ship/instruments'

type D = Record<string, any>

/** The README's table ("Fast is a contract"): each budget, its unit, and where the wall reads it. */
const CONTRACTS = [
  { key: 'cold', word: 'Start to answering', budget: 50, unit: 'ms', bench: 'cold' },
  { key: 'shutdown', word: 'Clean stop, work in flight', budget: 100, unit: 'ms', bench: 'shutdown' },
  { key: 'kill', word: 'Crash, restart, answer', budget: 150, unit: 'ms', bench: 'kill' },
  { key: 'swap', word: 'Swap the binary under load', budget: 200, unit: 'ms', bench: 'swap' },
  { key: 'overhead', word: 'Harness overhead a turn', budget: 5, unit: 'ms', bench: undefined },
  { key: 'frames', word: 'Frames a plain turn', budget: 5, unit: 'frames', bench: 'frames_plain' },
] as const

// ---------------------------------------------------------------- reading the record

interface TurnCost { at: number; session: string | null; turn: string | null; total: number; model: number; tools: number; commits: number; compile: number; admission: number; rest: number; harness: number; storeSpans: number[]; loops: number; stop: string; frames?: number }

/** One turn's trace, split: the model's calls and the tools' runs are the turn's own waits; the rest is the harness,
 *  and of that, the store's commits are the disk's (each frame one fsync). */
function costOf(r: LedgerEntry): TurnCost | null {
  const root = r.data as Span | null
  if (!root || typeof root.start_us !== 'number' || typeof root.end_us !== 'number') return null
  const c = { model: 0, tools: 0, commits: 0, compile: 0, admission: 0, storeSpans: [] as number[] }
  const walk = (s: Span) => {
    const d = Math.max(0, (s.end_us ?? s.start_us) - s.start_us) / 1000
    if (s.kind === 'provider') { c.model += d; return }
    if (s.kind === 'tool' && s.name.startsWith('tool ')) { c.tools += d; return }
    if (s.kind === 'store') { c.commits += d; c.storeSpans.push(d) }
    if (s.kind === 'compile') c.compile += d
    if (s.kind === 'lock') c.admission += d
    for (const ch of s.children ?? []) walk(ch)
  }
  walk(root)
  const total = (root.end_us - root.start_us) / 1000
  const harness = Math.max(0, total - c.model - c.tools)
  const attrs = (root.attrs ?? {}) as D
  return {
    at: r.at_unix_ms, session: r.session_id, turn: r.turn_id, total, harness, ...c,
    rest: Math.max(0, harness - c.commits - c.compile - c.admission), loops: Number(attrs.loops ?? 0),
    // The turn's own frame count (theseus-wz4y); absent from a daemon before it, and from a failed turn.
    stop: String(attrs.stop_reason ?? ''), frames: typeof attrs.frames === 'number' ? attrs.frames : undefined,
  }
}

/** Every start's serving time, from its `server.serving` row (µs → ms). */
function startsOf(rows: LedgerEntry[]): { at: number; ms: number; phases: StartupPhase[] }[] {
  const out = []
  for (const r of rows) {
    if (r.kind !== 'server.serving') continue
    const d = (r.data ?? {}) as D
    if (typeof d.serving_us === 'number') out.push({ at: r.at_unix_ms, ms: d.serving_us / 1000, phases: (d.phases ?? []) as StartupPhase[] })
  }
  return out
}

const phaseOf = (run: BenchRun, name: string) => run.phases.find((p) => p.phase === name)

export default function Speed() {
  const nav = useNavigate()
  const { data: h } = useRpc<Health>('health', undefined, 5000)
  const { data: bh } = useRpc<BenchHistoryResult>('bench.history', { last: 400 }, 60_000)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 10_000)
  const { rows } = useHistoryRows()
  const [onlyMain, setOnlyMain] = useState<'main' | 'every branch'>('main')
  const title = useMemo(() => new Map((sl?.sessions ?? []).map((s) => [s.session_id, s.title || s.label || short(s.session_id)])), [sl])

  const turns = useMemo(() => rows.filter((r) => r.kind === 'turn.trace').map(costOf).filter((x): x is TurnCost => !!x), [rows])
  const starts = useMemo(() => startsOf(rows), [rows])
  const serving = h?.startup.find((p) => p.name === 'socket')?.end_us
  const runs = useMemo(() => (bh?.runs ?? []).filter((r) => onlyMain === 'every branch' || r.label.startsWith('main ')), [bh, onlyMain])
  const lastWith = (phase: string) => [...runs].reverse().find((r) => phaseOf(r, phase))
  const recent = turns.slice(-50)
  const p = (xs: number[], q: number) => quantile(xs, q)

  const dials: DialData[] = CONTRACTS.map((c) => {
    if (c.key === 'cold') {
      const g = lastWith('cold')
      return {
        value: serving !== undefined && serving !== null ? serving / 1000 : undefined, gate: g ? phaseOf(g, 'cold')!.p95 : undefined,
        limit: g ? phaseOf(g, 'cold')!.limit : undefined, source: 'this daemon’s last start; the pointer is the last gate’s p95',
        note: g ? `this daemon · gate p95 ${ms(phaseOf(g, 'cold')!.p95)}` : 'this daemon’s last start', spark: starts.slice(-40).map((s) => s.ms),
      }
    }
    if (c.key === 'overhead') {
      const disk = p(recent.map((t) => t.commits), 0.5)
      const rest = p(recent.map((t) => t.rest + t.compile + t.admission), 0.5)
      return {
        value: p(recent.map((t) => t.harness), 0.5), gate: rest, source: 'the median of the last 50 turns’ traces; the pointer leaves out the disk’s commits',
        note: disk !== undefined ? `the disk ${ms(disk)} · the rest ${ms(rest)}` : 'no traced turns', spark: recent.map((t) => t.harness),
      }
    }
    const g = lastWith(c.bench!)
    const ph = g ? phaseOf(g, c.bench!) : undefined
    if (c.key === 'frames') {
      // This daemon's plain turns (one loop, no tool call), each counting its own frames, beside the gate's bench. A
      // session's first turn is left out, as the bench leaves out its warm-ups: it also writes what opens the session.
      const seen = new Set<string | null>()
      const warm = turns.filter((t) => { const first = !seen.has(t.session); seen.add(t.session); return !first })
      const live = warm.filter((t) => t.loops === 1 && t.stop === 'no_tool_calls' && t.frames !== undefined).slice(-50).map((t) => t.frames!)
      if (live.length) {
        return {
          value: p(live, 0.95), gate: ph?.p95, limit: ph?.limit,
          source: `this daemon’s last ${live.length} plain turns, each counting its own frames (p95; a session’s first turn left out)${g ? `; the pointer is the gate’s bench, ${g.label} (p95)` : ''}`,
          note: `live p95 ${p(live, 0.95)} of ${live.length} turns${ph ? ` · gate p95 ${ph.p95}` : ' · no bench history'}`,
          spark: live,
        }
      }
    }
    return {
      value: ph?.p95, gate: ph?.p50, limit: ph?.limit, source: g ? `the gate’s bench, ${g.label} (p95; the pointer is p50)` : 'no bench history on this machine',
      note: g ? `gate ${g.label.replace(/^main /, 'main ')}${ph?.limit ? ` · limit ${c.unit === 'frames' ? ph.limit : ms(ph.limit)}` : ''}` : 'no bench history',
      spark: runs.filter((r) => phaseOf(r, c.bench!)).slice(-40).map((r) => phaseOf(r, c.bench!)!.p95),
    }
  })

  return (
    <div className="flex flex-col gap-3">
      <div className="panel px-4 pb-3 pt-3">
        <div className="flex flex-wrap items-baseline gap-x-4">
          <h1 className="ship-title !text-[26px]">The speed wall</h1>
          <span className="text-[12px] text-ink-dim">fast is a contract: every budget the README promises, against what the record says</span>
          <span className="num ml-auto text-[11px] text-ink-faint">
            {bh?.exists ? `${bh.total} bench runs in ${bh.path?.replace(/^.*\/\.cache\//, '~/.cache/')}` : bh ? 'no bench history on this machine: the wall shows the live numbers alone' : 'reading…'}
          </span>
        </div>
        <div className="speed-wall mt-2 grid grid-cols-2 gap-x-2 gap-y-3 md:grid-cols-3 xl:grid-cols-6">
          {CONTRACTS.map((c, i) => <ContractDial key={c.key} c={c} d={dials[i]} />)}
        </div>
      </div>

      <div className="grid grid-cols-1 gap-3 2xl:grid-cols-[1fr_1fr]">
        <Panel title="The last start · phases from process start" icon={<Rocket size={13} />} bodyClassName="h-[300px] p-2"
          actions={<span className="num text-[11px] text-ink-faint">serving at {serving ? ms(serving / 1000) : '—'} · budget 50 ms</span>}>
          {h ? <Waterfall phases={h.startup} /> : <Empty>reading health…</Empty>}
        </Panel>
        <Panel title={<>Every start · serving time, {starts.length} in the record</>} icon={<History size={13} />} bodyClassName="h-[300px] p-2">
          {starts.length ? <Starts starts={starts} /> : <Empty>no start rows in the record</Empty>}
        </Panel>
      </div>

      <div className="grid grid-cols-1 gap-3 2xl:grid-cols-[1.4fr_1fr]">
        <Panel title={<>Each turn · harness overhead, the turn less its model and tools · {turns.length} traced</>} icon={<Timer size={13} />} bodyClassName="h-[320px] p-2">
          {turns.length ? <Overheads turns={turns} title={title} onPick={(sid) => nav(`/session/${sid}`)} /> : <Empty>no turn traces in the record</Empty>}
        </Panel>
        <Panel title="The write path · commit latency, one fsync a frame" icon={<HardDrive size={13} />} bodyClassName="h-[320px] p-2"
          actions={<span className="num text-[11px] text-ink-faint">{commitLine(turns)}</span>}>
          {turns.length ? <Commits turns={turns} /> : <Empty>no commits traced</Empty>}
        </Panel>
      </div>

      <Panel title={<>The gates&rsquo; benches · every run&rsquo;s p95 against its limit</>} icon={<GaugeIcon size={13} />} bodyClassName="p-2"
        actions={<Segmented value={onlyMain} options={['main', 'every branch'] as const} onChange={setOnlyMain} />}>
        {bh?.exists && runs.length ? <BenchGrid runs={runs} /> : <Empty>{bh?.exists ? 'no run on this filter' : 'No bench history on this machine (the gate appends ~/.cache/theseus/bench-history.csv): the wall shows the live numbers alone.'}</Empty>}
        {!!bh?.skipped.length && <div className="num px-2 pt-1 text-[10.5px] text-ink-faint">{bh.skipped.length} line{bh.skipped.length === 1 ? '' : 's'} left out: {bh.skipped[0]}</div>}
      </Panel>
    </div>
  )
}

// ---------------------------------------------------------------- the dials

interface DialData { value?: number; gate?: number; limit?: number; source: string; note: string; spark: number[] }

function ContractDial({ c, d }: { c: (typeof CONTRACTS)[number]; d: DialData }) {
  const max = Math.max(c.budget * 2, (d.value ?? 0) * 1.1, (d.gate ?? 0) * 1.1)
  const at = (v: number) => -120 + (Math.min(v, max) / max) * 240
  const over = d.value !== undefined && d.value > c.budget
  const tone = d.value === undefined ? '#9c907a' : over ? '#fb7185' : d.value > c.budget * 0.8 ? '#fbbf24' : '#22d3ee'
  const fmt = (v?: number) => (v === undefined ? '—' : c.unit === 'frames' ? `${v % 1 ? v.toFixed(1) : v}` : ms(v))
  return (
    <div className="flex flex-col items-center">
      <Dial title={`${c.word}: budget ${c.budget} ${c.unit}. ${d.source}.${d.limit ? ` The gate judges it against ${fmt(d.limit)} (the budget plus this machine’s margin).` : ''}`}
        label={c.word} sub={<span style={{ color: tone }}>{fmt(d.value)}{d.value !== undefined ? ` of ${c.budget}${c.unit === 'ms' ? ' ms' : ''}` : ''}</span>} glow={over ? '#fb7185' : undefined}>
        {() => (
          <g>
            <path d={arc(-120, 120, 44)} fill="none" stroke="#2b3d52" strokeWidth="4" />
            <path d={arc(at(c.budget), 120, 44)} fill="none" stroke="#fb7185" strokeOpacity="0.6" strokeWidth="4" />
            {d.limit !== undefined && d.limit > c.budget && <path d={arc(at(c.budget), at(d.limit), 44)} fill="none" stroke="#fbbf24" strokeOpacity="0.5" strokeWidth="4" />}
            <Ticks a0={-120} a1={120} n={8} major={2} r={41} />
            {[0, 0.5, 1].map((f) => {
              const [x, y] = polar(-120 + f * 240, 29)
              return <Engraved key={f} x={x} y={y + 2.3} size={6.4}>{c.unit === 'frames' ? (max * f).toFixed(0) : `${(max * f).toFixed(max * f < 10 ? 1 : 0)}`}</Engraved>
            })}
            {(() => { const [x, y] = polar(at(c.budget), 52); return <circle cx={x} cy={y} r="2.2" fill="#fb7185" /> })()}
            <Engraved x={60} y={86} size={5.6} color="#b8a77f">{c.unit === 'frames' ? 'FRAMES' : 'MS'}</Engraved>
            {d.gate !== undefined && (
              <g transform={`rotate(${at(d.gate)} 60 60)`} opacity="0.85"><path d="M 58.2 22 L 60 14 L 61.8 22 Z" fill="#d6a548" /></g>
            )}
            {d.value !== undefined && <Needle deg={at(d.value)} color={tone} len={38} />}
          </g>
        )}
      </Dial>
      <div className="num -mt-px max-w-[190px] truncate text-center text-[10px] text-ink-faint" title={d.note}>{d.note}</div>
      <div className="mt-0.5 h-[26px] w-[150px]"><Spark values={d.spark} budget={c.budget} /></div>
    </div>
  )
}

/** A dial's history under it: each value against the budget's red line. */
function Spark({ values, budget }: { values: number[]; budget: number }) {
  if (values.length < 2) return <div className="mt-3 h-px w-full bg-line" />
  const max = Math.max(budget * 1.2, ...values)
  const pts = values.map((v, i) => `${(i / (values.length - 1)) * 150},${24 - (Math.min(v, max) / max) * 22}`).join(' ')
  const by = 24 - (budget / max) * 22
  return (
    <svg width="150" height="26" viewBox="0 0 150 26" aria-hidden>
      <line x1="0" x2="150" y1={by} y2={by} stroke="#fb7185" strokeOpacity="0.55" strokeDasharray="3 3" strokeWidth="1" />
      <polyline points={pts} fill="none" stroke="#d6a548" strokeWidth="1.3" />
    </svg>
  )
}

// ---------------------------------------------------------------- the start

function Waterfall({ phases }: { phases: StartupPhase[] }) {
  const option = useMemo<EChartsOption>(() => {
    const ps = [...phases].sort((a, b) => Number(a.background) - Number(b.background) || a.start_us - b.start_us)
    const names = ps.map((p) => `${p.background ? '↳ ' : ''}${p.name}`)
    const serving = phases.find((p) => p.name === 'socket')?.end_us
    const end = (p: StartupPhase) => (p.end_us ?? p.start_us) / 1000
    return {
      grid: { left: 108, right: 24, top: 10, bottom: 26 },
      tooltip: { trigger: 'item', formatter: (x: any) => { const p = ps[x.dataIndex]; return `<b>${p.name}</b>${p.background ? ' (after serving)' : ''}<br/>${ms(p.start_us / 1000)} → ${p.end_us === null ? 'running' : ms(end(p))}<br/>${ms(end(p) - p.start_us / 1000)}${p.detail ? `<br/><span style="color:${ink.faint}">${JSON.stringify(p.detail).slice(0, 160)}</span>` : ''}` } },
      xAxis: { type: 'log', logBase: 10, min: 1, ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => ms(v) } },
      yAxis: { type: 'category', data: names, inverse: true, ...axisStyle, axisLabel: { ...axisStyle.axisLabel, fontFamily: "'JetBrains Mono Variable', monospace", fontSize: 10.5 } },
      series: [
        { type: 'bar', stack: 'w', silent: true, itemStyle: { color: 'transparent' }, data: ps.map((p) => Math.max(1, p.start_us / 1000)) },
        {
          type: 'bar', stack: 'w', barWidth: 10,
          data: ps.map((p) => ({ value: Math.max(0.05, end(p) - Math.max(1, p.start_us / 1000)), itemStyle: { color: p.background ? '#5b6b80' : '#22d3ee', borderRadius: 3 } })),
          markLine: {
            silent: true, symbol: 'none',
            data: [
              { xAxis: 50, lineStyle: { color: '#fb7185', type: 'dashed', width: 1 }, label: { formatter: 'budget 50 ms', color: '#fb7185', fontSize: 10 } },
              ...(serving ? [{ xAxis: serving / 1000, lineStyle: { color: '#d6a548', width: 1 }, label: { formatter: `serving ${ms(serving / 1000)}`, color: '#d6a548', fontSize: 10, position: 'insideEndTop' as const } }] : []),
            ],
          },
        },
      ],
    }
  }, [phases])
  return <Echart option={option} />
}

function Starts({ starts }: { starts: { at: number; ms: number }[] }) {
  const option = useMemo<EChartsOption>(() => ({
    grid: { left: 46, right: 16, top: 14, bottom: 26 },
    tooltip: { trigger: 'item', formatter: (x: any) => `${stamp(x.value[0])}<br/>serving at <b>${ms(x.value[1])}</b>` },
    xAxis: { type: 'time', ...axisStyle, splitLine: { show: false } },
    yAxis: { type: 'log', logBase: 10, ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => ms(v) } },
    series: [{
      type: 'scatter', symbolSize: 9,
      data: starts.map((s) => ({ value: [s.at, Math.max(1, s.ms)], itemStyle: { color: s.ms > 50 ? '#fb7185' : '#22d3ee', borderColor: '#030912', borderWidth: 1 } })),
      markLine: { silent: true, symbol: 'none', data: [{ yAxis: 50, lineStyle: { color: '#fb7185', type: 'dashed' }, label: { formatter: '50 ms', color: '#fb7185', fontSize: 10 } }] },
    }],
  }), [starts])
  return <Echart option={option} />
}

// ---------------------------------------------------------------- turns and commits

const PART_COLORS = { commits: '#8b6cf0', compile: '#3b7fdb', admission: '#b8862c', rest: '#0ea5c6' } as const

function Overheads({ turns, title, onPick }: { turns: TurnCost[]; title: Map<string, string>; onPick: (sid: string) => void }) {
  const option = useMemo<EChartsOption>(() => {
    const parts = ['commits', 'compile', 'admission', 'rest'] as const
    const words = { commits: 'the disk’s commits', compile: 'compiles', admission: 'admission', rest: 'the rest' }
    const xs = turns.map((t) => t.at)
    return {
      grid: { left: 52, right: 16, top: 30, bottom: 26 },
      legend: { top: 2, right: 8, textStyle: { color: ink.text, fontSize: 10.5 }, itemWidth: 10, itemHeight: 8, data: parts.map((p) => words[p]) },
      tooltip: {
        trigger: 'axis', axisPointer: { type: 'shadow' },
        formatter: (ps: any) => {
          const t = turns[ps[0]?.dataIndex ?? 0]
          if (!t) return ''
          return `<b>${title.get(t.session ?? '') ?? short(t.session)}</b> · ${stamp(t.at)}<br/>turn ${ms(t.total)}: model ${ms(t.model)}, tools ${ms(t.tools)}<br/>harness <b>${ms(t.harness)}</b>: commits ${ms(t.commits)} (${t.storeSpans.length}), compile ${ms(t.compile)}, admission ${ms(t.admission)}, rest ${ms(t.rest)}`
        },
      },
      xAxis: { type: 'category', data: xs.map((x) => stamp(x)), ...axisStyle, axisLabel: { ...axisStyle.axisLabel, showMaxLabel: true, interval: Math.max(0, Math.floor(xs.length / 6)) } },
      yAxis: { type: 'value', ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => ms(v) } },
      series: parts.map((pt, i) => ({
        name: words[pt], type: 'bar' as const, stack: 'h', barMaxWidth: 18,
        itemStyle: { color: PART_COLORS[pt], borderColor: '#0a1828', borderWidth: 1, ...(i === parts.length - 1 ? { borderRadius: [3, 3, 0, 0] as [number, number, number, number] } : {}) },
        data: turns.map((t) => +t[pt].toFixed(2)),
        ...(i === 0 ? { markLine: { silent: true, symbol: 'none', data: [{ yAxis: 5, lineStyle: { color: '#fb7185', type: 'dashed' as const }, label: { formatter: 'budget 5 ms', color: '#fb7185', fontSize: 10 } }] } } : {}),
      })),
    }
  }, [turns, title])
  return <Echart option={option} onClick={(x: any) => { const t = turns[x?.dataIndex]; if (t?.session) onPick(t.session) }} />
}

function commitLine(turns: TurnCost[]): string {
  const all = turns.flatMap((t) => t.storeSpans)
  if (!all.length) return ''
  return `${all.length} commits · p50 ${ms(quantile(all, 0.5))} · p95 ${ms(quantile(all, 0.95))}`
}

function Commits({ turns }: { turns: TurnCost[] }) {
  const option = useMemo<EChartsOption>(() => {
    const all = turns.flatMap((t) => t.storeSpans)
    const edges = [0.5, 1, 2, 5, 10, 20, 50, 100, 200, 500]
    const counts = new Array(edges.length + 1).fill(0)
    for (const v of all) counts[edges.findIndex((e) => v < e) === -1 ? edges.length : edges.findIndex((e) => v < e)]++
    const labels = ['<0.5 ms', ...edges.slice(1).map((e, i) => `${edges[i]}–${e} ms`), '≥500 ms']
    return {
      grid: { left: 40, right: 12, top: 14, bottom: 46 },
      tooltip: { trigger: 'item', formatter: (x: any) => `${labels[x.dataIndex]}: <b>${x.value}</b> commit${x.value === 1 ? '' : 's'}` },
      xAxis: { type: 'category', data: labels, ...axisStyle, axisLabel: { ...axisStyle.axisLabel, rotate: 35, fontSize: 9.5 } },
      yAxis: { type: 'value', minInterval: 1, ...axisStyle },
      series: [{ type: 'bar', barMaxWidth: 26, data: counts.map((n) => ({ value: n, itemStyle: { color: '#8b6cf0', borderRadius: [3, 3, 0, 0] } })) }],
    }
  }, [turns])
  return <Echart option={option} />
}

// ---------------------------------------------------------------- the benches

const BENCH_PANELS = [
  { phase: 'cold', word: 'start', budget: 50 }, { phase: 'shutdown', word: 'clean stop', budget: 100 },
  { phase: 'kill', word: 'crash and restart', budget: 150 }, { phase: 'swap', word: 'binary swap', budget: 200 },
  { phase: 'turn_plain', word: 'a plain turn, stand-in model', budget: undefined }, { phase: 'frames_plain', word: 'frames a plain turn', budget: 5 },
] as const

function BenchGrid({ runs }: { runs: BenchRun[] }) {
  return (
    <div className="grid grid-cols-1 gap-2 md:grid-cols-2 2xl:grid-cols-3">
      {BENCH_PANELS.map((b) => <BenchChart key={b.phase} runs={runs} phase={b.phase} word={b.word} budget={b.budget} />)}
    </div>
  )
}

function BenchChart({ runs, phase, word, budget }: { runs: BenchRun[]; phase: string; word: string; budget?: number }) {
  const pts = runs.map((r) => ({ r, p: phaseOf(r, phase) })).filter((x) => x.p)
  const option = useMemo<EChartsOption>(() => ({
    grid: { left: 44, right: 10, top: 22, bottom: 22 },
    title: { text: word, left: 6, top: 0, textStyle: { color: '#d6a548', fontSize: 10.5, fontFamily: "'Cinzel Variable', serif", fontWeight: 700 } },
    tooltip: { trigger: 'axis', formatter: (ps: any) => { const x = pts[ps[0]?.dataIndex ?? 0]; return x ? `${x.r.label}<br/>${x.r.time}<br/>p50 ${x.p!.p50} · p95 <b>${x.p!.p95}</b>${x.p!.limit ? ` · limit ${x.p!.limit}` : ''}${x.r.passed ? '' : '<br/><span style="color:#fb7185">failed</span>'}` : '' } },
    xAxis: { type: 'category', data: pts.map((x) => x.r.time.slice(5, 16).replace('T', ' ')), ...axisStyle, axisLabel: { ...axisStyle.axisLabel, fontSize: 9, interval: Math.max(0, Math.floor(pts.length / 4)) } },
    yAxis: { type: 'value', ...axisStyle, axisLabel: { ...axisStyle.axisLabel, fontSize: 9.5 } },
    series: [
      { type: 'line', name: 'p95', data: pts.map((x) => x.p!.p95), symbol: 'none', lineStyle: { color: '#22d3ee', width: 1.6 } },
      { type: 'line', name: 'limit', data: pts.map((x) => x.p!.limit ?? null), symbol: 'none', step: 'end', lineStyle: { color: '#fbbf24', width: 1, type: 'dashed' } },
      ...(budget ? [{ type: 'line' as const, name: 'budget', data: pts.map(() => budget), symbol: 'none', lineStyle: { color: '#fb7185', width: 1, type: 'dotted' as const } }] : []),
      { type: 'scatter', name: 'missed', data: pts.map((x) => (x.r.passed ? null : x.p!.p95)), symbolSize: 6, itemStyle: { color: '#fb7185' } },
    ],
  }), [pts, word, budget])
  if (!pts.length) return <div className={cn('flex h-[150px] items-center justify-center rounded-md text-[11px] text-ink-faint ring-1 ring-line')}>{word}: no runs</div>
  return <div className="h-[150px] rounded-md ring-1 ring-line"><Echart option={option} /></div>
}
