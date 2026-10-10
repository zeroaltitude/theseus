// The speed wall (theseus-logs, round two): "fast is a contract", made visible. The README's budgets, each a brass
// dial with its red line, read from the daemon's own record:
//
// - the start: health's phases for the last one, and every start since from its `server.serving` row;
// - each turn's harness overhead, from its trace: the turn's time less its model's calls and its tools' runs, the
//   disk's commits included (theseus-4w1h: the README's definition, and `theseus-sim bench turn`'s), split into the
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
import type { EChartsOption } from '@/lib/chart'
import { cn, ms, pct, short, stamp } from '@/lib/format'
import {
  CATEGORICAL, CHROME, FONTS, MARK, TIME_LABELS, TIP_FRAME, TONE_MARK, barRadius, baseAxis, budgetLine, countTick, msLogTick, msTick,
  niceScale, numTick, stackTop, valueAxis,
} from '@/lib/viz'
import { tip } from '@/lib/viztip'
import { Echart } from '@/components/Echart'
import { ChartPanel, type LegendItem } from '@/components/ChartPanel'
import { Startup } from '@/components/instruments'
import { STARTUP_LEGEND, startupTable } from '@/components/instrumentTables'
import { Segmented } from '@/components/ui'
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
        value: p(recent.map((t) => t.harness), 0.5), gate: rest, source: 'the median of the last 50 turns’ traces, each the turn less its model and tools, the disk’s commits included; the pointer leaves the commits out',
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
        <ChartPanel id="start" title="The last start · phases from process start" icon={<Rocket size={13} />} height={300}
          actions={<span className="num text-[11px] text-ink-faint">serving at {serving ? ms(serving / 1000) : '—'} · budget 50 ms</span>}
          legend={STARTUP_LEGEND} empty={h ? undefined : 'reading health…'} table={startupTable(h?.startup ?? [])}>
          {h && <Startup phases={h.startup} />}
        </ChartPanel>
        <ChartPanel id="starts" title={`Every start · serving time, ${starts.length} in the record`} icon={<History size={13} />} height={300}
          legend={[{ key: 'in', label: 'within the 50 ms budget', color: TONE_MARK.ok, mark: 'dot' }, { key: 'over', label: 'over the budget', color: TONE_MARK.fault, mark: 'triangle' }]}
          empty={starts.length ? undefined : 'no start rows in the record'} table={startsTable(starts)}>
          <Starts starts={starts} />
        </ChartPanel>
      </div>

      <div className="grid grid-cols-1 gap-3 2xl:grid-cols-[1.4fr_1fr]">
        <ChartPanel id="overhead" title={`Each turn · harness overhead, the turn less its model and tools · ${turns.length} traced`} icon={<Timer size={13} />} height={320}
          legend={PARTS.map((pt) => ({ key: pt, label: PART_WORDS[pt], color: PART_COLORS[pt], mark: 'rect' as const }))}
          empty={turns.length ? undefined : 'no turn traces in the record'} table={overheadTable(turns, title)}>
          <Overheads turns={turns} title={title} onPick={(sid) => nav(`/session/${sid}`)} />
        </ChartPanel>
        <ChartPanel id="commits" title="The write path · commit latency, one fsync a frame" icon={<HardDrive size={13} />} height={320}
          empty={turns.length ? undefined : 'no commits traced'} table={commitsTable(turns)}>
          <div className="flex h-full flex-col">
            <div className="num px-2 pt-0.5 text-[11px] text-ink-dim">{commitLine(turns)}</div>
            <div className="min-h-0 flex-auto"><Commits turns={turns} /></div>
          </div>
        </ChartPanel>
      </div>

      <ChartPanel id="benches" title={<>The gates&rsquo; benches · every run&rsquo;s p95 against its limit</>} icon={<GaugeIcon size={13} />}
        actions={<Segmented value={onlyMain} options={['main', 'every branch'] as const} onChange={setOnlyMain} />}
        legend={BENCH_LEGEND} table={benchTable(runs)}
        empty={bh?.exists && runs.length ? undefined : bh?.exists ? 'no run on this filter' : 'No bench history on this machine (the gate appends ~/.cache/theseus/bench-history.csv): the wall shows the live numbers alone.'}>
        <BenchGrid runs={runs} />
        {!!bh?.skipped.length && <div className="num px-2 pt-1 text-[10.5px] text-ink-faint">{bh.skipped.length} line{bh.skipped.length === 1 ? '' : 's'} left out: {bh.skipped[0]}</div>}
      </ChartPanel>
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

// ---------------------------------------------------------------- the charts, after the chart method (lib/viz.ts)
//
// Below the dials each chart follows the method (theseus-hnof): one scale a plot, clean ticks, thin marks, a status colour
// only for state and always with a shape and a word, the budget lines solid with their words in the ink, a legend for two
// series or more, and every chart with its table view.

const C = CHROME.dark
/** A single series' colour: the first categorical slot. */
const ACCENT = CATEGORICAL.dark[0]
/** A turn's harness overhead, part by part, in the categorical slots' order: the disk's commits take the first slot,
 *  as they do in the commits' histogram beside it. */
const PARTS = ['commits', 'compile', 'admission', 'rest'] as const
const PART_WORDS = { commits: 'the disk’s commits', compile: 'compiles', admission: 'admission', rest: 'the rest' } as const
const PART_COLORS = { commits: CATEGORICAL.dark[0], compile: CATEGORICAL.dark[1], admission: CATEGORICAL.dark[2], rest: CATEGORICAL.dark[3] } as const

// ---------------------------------------------------------------- the starts

/** Every start's serving time over time, on a log axis: within the budget a dot in the ok tone, over it a triangle in the
 *  fault tone (shape and colour, and the legend's words). */
function Starts({ starts }: { starts: { at: number; ms: number }[] }) {
  const option = useMemo<EChartsOption>(() => {
    const vaxis = valueAxis(), axis = baseAxis()
    return {
      grid: { left: 12, right: 28, top: 16, bottom: 22, containLabel: true },
      tooltip: {
        ...TIP_FRAME, trigger: 'item',
        formatter: (x: any) => {
          const s = starts[x.dataIndex]
          if (!s) return ''
          const over = s.ms > 50
          return tip(stamp(s.at), [{ value: ms(s.ms), label: over ? 'serving, over the 50 ms budget' : 'serving, within the 50 ms budget', color: over ? TONE_MARK.fault : TONE_MARK.ok, mark: over ? 'triangle' : 'dot' }])
        },
      },
      xAxis: { type: 'time', ...axis, axisLabel: { ...axis.axisLabel, formatter: TIME_LABELS } },
      yAxis: { ...vaxis, type: 'log', logBase: 10, axisLabel: { ...vaxis.axisLabel, formatter: msLogTick } },
      series: [{
        type: 'scatter', symbolSize: MARK.marker + MARK.ring,
        data: starts.map((s) => {
          const over = s.ms > 50
          return { value: [s.at, Math.max(1, s.ms)], symbol: over ? 'triangle' : 'circle', itemStyle: { color: over ? TONE_MARK.fault : TONE_MARK.ok, borderColor: C.surface, borderWidth: MARK.ring } }
        }),
        markLine: { silent: true, symbol: 'none', data: [budgetLine({ yAxis: 50 }, 'budget 50 ms', 'insideEndTop')] },
      }],
    }
  }, [starts])
  return <Echart option={option} />
}

function startsTable(starts: { at: number; ms: number }[]) {
  type R = { at: number; ms: number }
  return {
    caption: 'every start in the record and its serving time', rows: [...starts].reverse(), rowKey: (r: R) => String(r.at),
    columns: [
      { key: 'at', label: 'started', cell: (r: R) => stamp(r.at) },
      { key: 'ms', label: 'serving at', num: true, cell: (r: R) => ms(r.ms) },
      { key: 'budget', label: 'the 50 ms budget', cell: (r: R) => (r.ms > 50 ? `over, by ${ms(r.ms - 50)}` : 'within') },
    ],
  }
}

// ---------------------------------------------------------------- turns and commits

/** Each traced turn's harness overhead, a column stacked part by part (a 2 px gap between parts, the top part's end
 *  rounded), on one scale with the 5 ms budget; a click opens the turn's session. */
function Overheads({ turns, title, onPick }: { turns: TurnCost[]; title: Map<string, string>; onPick: (sid: string) => void }) {
  const option = useMemo<EChartsOption>(() => {
    const xs = turns.map((t) => t.at)
    const vals = PARTS.map((pt) => turns.map((t) => +t[pt].toFixed(2)))
    const tops = stackTop(vals)
    const sc = niceScale(Math.max(5, ...turns.map((t) => PARTS.reduce((a, pt) => a + t[pt], 0))))
    const vaxis = valueAxis(), axis = baseAxis()
    return {
      // The right margin holds the budget's words past the line's end: on this scale the 5 ms line lies on the baseline,
      // and words inside the plot sat on the last columns.
      grid: { left: 12, right: 80, top: 14, bottom: 22, containLabel: true },
      tooltip: {
        ...TIP_FRAME, trigger: 'axis', axisPointer: { type: 'shadow', shadowStyle: { color: 'rgba(176,141,87,0.08)' } },
        formatter: (ps: any) => {
          const t = turns[(Array.isArray(ps) ? ps[0] : ps)?.dataIndex ?? -1]
          if (!t) return ''
          return tip(`${title.get(t.session ?? '') ?? short(t.session)} · ${stamp(t.at)}`, [
            ...PARTS.map((pt) => ({ value: ms(t[pt]), label: pt === 'commits' ? `${PART_WORDS[pt]} (${t.storeSpans.length})` : PART_WORDS[pt], color: PART_COLORS[pt], mark: 'rect' as const })),
            { value: ms(t.harness), label: 'the harness, in all' },
            { value: ms(t.total), label: `the turn: model ${ms(t.model)}, tools ${ms(t.tools)}`, strong: false },
          ], t.session ? 'a click opens the session' : undefined)
        },
      },
      xAxis: { type: 'category', data: xs.map((x) => stamp(x)), ...axis, axisLabel: { ...axis.axisLabel, showMaxLabel: true, interval: Math.max(0, Math.floor(xs.length / 6)) } },
      yAxis: { ...vaxis, min: 0, max: sc.max, interval: sc.interval, axisLabel: { ...vaxis.axisLabel, formatter: msTick(sc.interval) } },
      series: PARTS.map((pt, i) => ({
        name: PART_WORDS[pt], type: 'bar' as const, stack: 'h', barMaxWidth: 18,
        itemStyle: { color: PART_COLORS[pt], borderColor: C.surface, borderWidth: MARK.gap / 2 },
        data: vals[i].map((v, j) => ({ value: v, itemStyle: { borderRadius: tops[j] === i ? barRadius(false) : 0 } })),
        ...(i === 0 ? { markLine: { silent: true, symbol: 'none' as const, data: [budgetLine({ yAxis: 5 }, 'budget 5 ms', 'end')] } } : {}),
      })),
    }
  }, [turns, title])
  return <Echart option={option} onClick={(x: any) => { const t = turns[x?.dataIndex]; if (t?.session) onPick(t.session) }} />
}

function overheadTable(turns: TurnCost[], title: Map<string, string>) {
  type R = TurnCost
  return {
    caption: 'each traced turn, its time and its harness overhead part by part', rows: [...turns].reverse(), rowKey: (r: R) => `${r.at}:${r.turn ?? ''}`,
    columns: [
      { key: 'at', label: 'when', cell: (r: R) => stamp(r.at) },
      { key: 'session', label: 'session', cell: (r: R) => title.get(r.session ?? '') ?? short(r.session), title: (r: R) => title.get(r.session ?? '') ?? undefined },
      { key: 'total', label: 'turn', num: true, cell: (r: R) => ms(r.total) },
      { key: 'model', label: 'model', num: true, cell: (r: R) => ms(r.model) },
      { key: 'tools', label: 'tools', num: true, cell: (r: R) => ms(r.tools) },
      { key: 'harness', label: 'harness', num: true, cell: (r: R) => ms(r.harness) },
      { key: 'commits', label: 'commits', num: true, cell: (r: R) => `${ms(r.commits)} (${r.storeSpans.length})` },
      { key: 'compile', label: 'compiles', num: true, cell: (r: R) => ms(r.compile) },
      { key: 'admission', label: 'admission', num: true, cell: (r: R) => ms(r.admission) },
      { key: 'rest', label: 'the rest', num: true, cell: (r: R) => ms(r.rest) },
    ],
  }
}

function commitLine(turns: TurnCost[]): string {
  const all = turns.flatMap((t) => t.storeSpans)
  if (!all.length) return ''
  return `${all.length} commits · p50 ${ms(quantile(all, 0.5))} · p95 ${ms(quantile(all, 0.95))}`
}

/** The commit latencies' bins, in ms: under 0.5, then each edge to the next, then 500 and over. */
const EDGES = [0.5, 1, 2, 5, 10, 20, 50, 100, 200, 500]
const BINS = ['<0.5', ...EDGES.slice(1).map((e, i) => `${EDGES[i]}–${e}`), '≥500']

function commitBins(turns: TurnCost[]): { counts: number[]; n: number } {
  const all = turns.flatMap((t) => t.storeSpans)
  const counts = new Array<number>(EDGES.length + 1).fill(0)
  for (const v of all) { const i = EDGES.findIndex((e) => v < e); counts[i === -1 ? EDGES.length : i]++ }
  return { counts, n: all.length }
}

/** The write path's commit latencies as a histogram: one series, the commits' colour, ms bins in order, no rotated words. */
function Commits({ turns }: { turns: TurnCost[] }) {
  const option = useMemo<EChartsOption>(() => {
    const { counts, n } = commitBins(turns)
    const sc = niceScale(Math.max(1, ...counts), 4, 1)
    const vaxis = valueAxis(), axis = baseAxis()
    return {
      grid: { left: 12, right: 24, top: 14, bottom: 22, containLabel: true },
      tooltip: {
        ...TIP_FRAME, trigger: 'item',
        formatter: (x: any) => tip(`${BINS[x.dataIndex]} ms`, [{ value: countTick(x.value), label: `commit${x.value === 1 ? '' : 's'} · ${pct(n ? x.value / n : 0, 1)} of ${n}`, color: PART_COLORS.commits, mark: 'rect' }]),
      },
      xAxis: { type: 'category', data: BINS, name: 'ms', nameGap: 6, nameTextStyle: { color: C.muted, fontSize: 10, fontFamily: FONTS.mono }, ...axis, axisLabel: { ...axis.axisLabel, interval: 0, fontSize: 9.5 } },
      yAxis: { ...vaxis, min: 0, max: sc.max, interval: sc.interval, axisLabel: { ...vaxis.axisLabel, formatter: countTick } },
      series: [{ type: 'bar', barMaxWidth: MARK.bar, data: counts.map((c) => ({ value: c, itemStyle: { color: PART_COLORS.commits, borderRadius: barRadius(false) } })) }],
    }
  }, [turns])
  return <Echart option={option} />
}

function commitsTable(turns: TurnCost[]) {
  const { counts, n } = commitBins(turns)
  type R = { bin: string; count: number }
  return {
    caption: 'the commits by latency', rows: BINS.map((bin, i) => ({ bin, count: counts[i] })), rowKey: (r: R) => r.bin,
    columns: [
      { key: 'bin', label: 'latency, ms', cell: (r: R) => r.bin },
      { key: 'count', label: 'commits', num: true, cell: (r: R) => countTick(r.count) },
      { key: 'share', label: 'share', num: true, cell: (r: R) => pct(n ? r.count / n : 0, 1) },
    ],
  }
}

// ---------------------------------------------------------------- the benches

const BENCH_PANELS = [
  { phase: 'cold', word: 'start', budget: 50 }, { phase: 'shutdown', word: 'clean stop', budget: 100 },
  { phase: 'kill', word: 'crash and restart', budget: 150 }, { phase: 'swap', word: 'binary swap', budget: 200 },
  { phase: 'turn_plain', word: 'a plain turn, stand-in model', budget: undefined }, { phase: 'frames_plain', word: 'frames a plain turn', budget: 5 },
] as const

/** The small multiples' one key: the run's p95 is the series; the gate's limit and the README's budget are lines in their
 *  status tones' steps for marks (the bright tones sit above the band a mark keeps to); a failed run is a triangle. */
const BENCH_LEGEND: LegendItem[] = [
  { key: 'p95', label: 'the run’s p95', color: ACCENT, mark: 'line' },
  { key: 'limit', label: 'the gate’s limit', color: TONE_MARK.wait, mark: 'line' },
  { key: 'budget', label: 'the README’s budget', color: TONE_MARK.fault, mark: 'line' },
  { key: 'missed', label: 'a run that failed', color: TONE_MARK.fault, mark: 'triangle' },
]

function BenchGrid({ runs }: { runs: BenchRun[] }) {
  return (
    <div className="grid grid-cols-1 gap-2 md:grid-cols-2 2xl:grid-cols-3">
      {BENCH_PANELS.map((b) => <BenchChart key={b.phase} runs={runs} phase={b.phase} word={b.word} budget={b.budget} />)}
    </div>
  )
}

function BenchChart({ runs, phase, word, budget }: { runs: BenchRun[]; phase: string; word: string; budget?: number }) {
  const pts = useMemo(() => runs.map((r) => ({ r, p: phaseOf(r, phase) })).filter((x): x is { r: BenchRun; p: NonNullable<ReturnType<typeof phaseOf>> } => !!x.p), [runs, phase])
  const option = useMemo<EChartsOption>(() => {
    const frames = phase.startsWith('frames')
    const sc = niceScale(Math.max(budget ?? 0, ...pts.map((x) => Math.max(x.p.p95, x.p.limit ?? 0))), 3)
    const fmt = frames ? numTick(sc.interval) : msTick(sc.interval)
    const vaxis = valueAxis(), axis = baseAxis()
    return {
      grid: { left: 8, right: 12, top: 24, bottom: 18, containLabel: true },
      title: { text: word, left: 8, top: 4, textStyle: { color: C.secondary, fontSize: 11, fontWeight: 500, fontFamily: FONTS.sans } },
      tooltip: {
        ...TIP_FRAME, trigger: 'axis', axisPointer: { type: 'line', lineStyle: { color: C.axis, width: 1, type: 'solid' } },
        formatter: (ps: any) => {
          const x = pts[(Array.isArray(ps) ? ps[0] : ps)?.dataIndex ?? -1]
          if (!x) return ''
          return tip(x.r.label, [
            { value: fmt(x.p.p95), label: 'p95', color: ACCENT, mark: 'line' },
            { value: fmt(x.p.p50), label: 'p50', strong: false },
            ...(x.p.limit ? [{ value: fmt(x.p.limit), label: 'the gate’s limit', color: TONE_MARK.wait, mark: 'line' as const, strong: false }] : []),
            ...(x.r.passed ? [] : [{ value: 'failed', label: 'this run', color: TONE_MARK.fault, mark: 'triangle' as const }]),
          ], x.r.time)
        },
      },
      xAxis: { type: 'category', data: pts.map((x) => x.r.time.slice(5, 16).replace('T', ' ')), ...axis, axisLabel: { ...axis.axisLabel, fontSize: 9, interval: Math.max(0, Math.floor(pts.length / 4)) } },
      yAxis: { ...vaxis, min: 0, max: sc.max, interval: sc.interval, axisLabel: { ...vaxis.axisLabel, fontSize: 9.5, formatter: fmt } },
      series: [
        { type: 'line', name: 'p95', data: pts.map((x) => x.p.p95), symbol: 'none', lineStyle: { color: ACCENT, width: MARK.line, cap: 'round', join: 'round' } },
        { type: 'line', name: 'limit', data: pts.map((x) => x.p.limit ?? null), symbol: 'none', step: 'end', lineStyle: { color: TONE_MARK.wait, width: 1 } },
        ...(budget ? [{ type: 'line' as const, name: 'budget', data: pts.map(() => budget), symbol: 'none', lineStyle: { color: TONE_MARK.fault, width: 1 } }] : []),
        { type: 'scatter', name: 'missed', data: pts.map((x) => (x.r.passed ? null : x.p.p95)), symbol: 'triangle', symbolSize: MARK.marker + MARK.ring, itemStyle: { color: TONE_MARK.fault, borderColor: C.surface, borderWidth: MARK.ring } },
      ],
    }
  }, [pts, phase, word, budget])
  if (!pts.length) return <div className={cn('flex h-[150px] items-center justify-center rounded-md text-[11px] text-ink-faint ring-1 ring-line')}>{word}: no runs</div>
  return <div className="h-[150px] rounded-md ring-1 ring-line"><Echart option={option} /></div>
}

function benchTable(runs: BenchRun[]) {
  type R = BenchRun
  const cell = (r: R, phase: string) => { const p = phaseOf(r, phase); return p ? (phase.startsWith('frames') ? String(p.p95) : ms(p.p95)) : '—' }
  return {
    caption: 'every bench run, newest first, with each phase\'s p95', rows: [...runs].reverse(), rowKey: (r: R) => `${r.time}:${r.label}`,
    columns: [
      { key: 'time', label: 'run', cell: (r: R) => r.time.slice(0, 16).replace('T', ' ') },
      { key: 'label', label: 'gate', cell: (r: R) => r.label, title: (r: R) => r.label },
      { key: 'passed', label: 'passed', cell: (r: R) => (r.passed ? 'yes' : 'failed') },
      ...BENCH_PANELS.map((b) => ({ key: b.phase, label: `${b.word} p95`, num: true, cell: (r: R) => cell(r, b.phase) })),
    ],
  }
}
