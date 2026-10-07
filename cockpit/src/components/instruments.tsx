// Instruments shared by the Bridge, the session deck, Speed, and Systems: the last start, the turns, the tools, context
// growth, and the token mix. Each follows the chart method (`lib/viz.ts`, theseus-hnof): its form by its job, the
// method's axes and marks, colour from a validated palette, a state's colour only with a shape and a word, the figures in
// the ink, a tooltip built as DOM, and a table of the same numbers for its panel's table view (`instrumentTables.ts`).
import { useMemo } from 'react'
import { AlertTriangle, X } from 'lucide-react'
import type { LedgerEntry, SessionInfo, StartupPhase } from '@protocol'
import { contextSeries, quantile, toolStats, type ProviderCall, type TurnRow } from '@/lib/derive'
import { clock, ms, pct, stamp, tokens, usd } from '@/lib/format'
import type { EChartsOption } from '@/lib/chart'
import {
  CATEGORICAL, CHROME, FONTS, MARK, OTHER, TIME_LABELS, TIP_FRAME, TOKEN_KINDS, TONE_MARK, barRadius, baseAxis, budgetLine,
  growthBySession, kindTokens, msLogTick, niceScale, stackTop, tokenTick, valueAxis, type Growth,
} from '@/lib/viz'
import { tip, type TipRow } from '@/lib/viztip'
import { Echart } from './Echart'
import { useWidth } from '@/lib/chartview'
import { MiniSpark, TipArea, TipBody, TipTarget } from './ChartPanel'
import { phaseOrder, titleOf } from './instrumentTables'
import { Empty } from './ui'

const C = CHROME.dark
/** A single series' colour: the first categorical slot. */
const ACCENT = CATEGORICAL.dark[0]

/** A time axis over `span` when it is given (the page's range), else over the data. */
function timeAxis(span?: readonly [number, number]) {
  const axis = baseAxis()
  return { type: 'time' as const, ...axis, ...(span ? { min: span[0], max: span[1] } : {}), axisLabel: { ...axis.axisLabel, formatter: TIME_LABELS } }
}

// ---------------------------------------------------------------- the last start

/** The last start's phases as bars from process start on a log axis: the phases on the way to serving in the accent, the
 *  ones after it in the de-emphasis gray, the budget and the moment of serving as lines with their words. */
export function Startup({ phases }: { phases: StartupPhase[] }) {
  const option = useMemo<EChartsOption>(() => {
    const ps = phaseOrder(phases)
    const names = ps.map((p) => `${p.background ? '↳ ' : ''}${p.name}`)
    const serving = phases.find((p) => p.name === 'socket')?.end_us
    const end = (p: StartupPhase) => (p.end_us ?? p.start_us) / 1000
    const budget = budgetLine({ xAxis: 50 }, 'budget 50 ms', 'start')
    const late = serving != null && serving / 1000 >= 50
    const vaxis = valueAxis(), axis = baseAxis()
    return {
      grid: { left: 12, right: 28, top: 22, bottom: 22, containLabel: true },
      tooltip: {
        ...TIP_FRAME, trigger: 'item',
        formatter: (x: any) => {
          const p = ps[x.dataIndex]
          if (!p) return ''
          return tip(p.name, [
            { value: ms(end(p) - p.start_us / 1000), label: p.background ? 'after serving, in the background' : 'on the way to serving', color: p.background ? OTHER : ACCENT, mark: 'rect' },
            { value: `${ms(p.start_us / 1000)} → ${p.end_us === null ? 'running' : ms(end(p))}`, label: 'from process start', strong: false },
          ], p.detail ? JSON.stringify(p.detail).slice(0, 160) : undefined)
        },
      },
      xAxis: { ...vaxis, type: 'log', logBase: 10, min: 1, axisLabel: { ...vaxis.axisLabel, formatter: msLogTick } },
      yAxis: { type: 'category', data: names, inverse: true, ...axis, axisLine: { show: false }, axisLabel: { ...axis.axisLabel, color: C.secondary, fontSize: 10.5 } },
      series: [
        { type: 'bar', stack: 'w', silent: true, itemStyle: { color: 'transparent' }, data: ps.map((p) => Math.max(1, p.start_us / 1000)) },
        {
          type: 'bar', stack: 'w', barWidth: 10,
          data: ps.map((p) => ({ value: Math.max(0.05, end(p) - Math.max(1, p.start_us / 1000)), itemStyle: { color: p.background ? OTHER : ACCENT, borderRadius: barRadius(true, 3) } })),
          // The two lines' words sit on their outer sides (the left line's to its left, the right one's to its right), so
          // they never print over each other however close the start came to its budget.
          markLine: {
            silent: true, symbol: 'none',
            data: [
              { ...budget, label: { ...budget.label, align: late ? 'right' as const : 'left' as const, padding: [0, 4] } },
              ...(serving ? [{ xAxis: serving / 1000, lineStyle: { color: C.text, width: 1, type: 'solid' as const }, label: { formatter: `serving ${ms(serving / 1000)}`, color: C.secondary, fontSize: 10, fontFamily: FONTS.mono, position: 'start' as const, align: late ? 'left' as const : 'right' as const, padding: [0, 4] } }] : []),
            ],
          },
        },
      ],
    }
  }, [phases])
  if (!phases.length) return <Empty>no start phases reported</Empty>
  return <Echart option={option} />
}

// ---------------------------------------------------------------- the turns

/** Each finished turn as a dot: when it ran (across), how long it took (up, on a log axis), and what it cost (its area);
 *  a failed turn is a triangle in the fault tone. The hover target is wider than the dot; a click opens the session. */
export function TurnsChart({ turns, title, span, onPick }: {
  turns: TurnRow[]; title: (sid: string | null | undefined) => string; span?: readonly [number, number]; onPick?: (sid: string) => void
}) {
  const done = useMemo(() => turns.filter((t) => t.elapsed_ms !== undefined), [turns])
  const option = useMemo<EChartsOption>(() => {
    const maxCost = Math.max(0, ...done.map((t) => t.cost ?? 0))
    const size = (c?: number) => (maxCost > 0 ? MARK.marker + 10 * Math.sqrt((c ?? 0) / maxCost) : MARK.marker + 2)
    const at = (t: TurnRow) => [t.start, Math.max(1, t.elapsed_ms ?? 1)]
    const vaxis = valueAxis()
    return {
      grid: { left: 12, right: 18, top: 12, bottom: 22, containLabel: true },
      tooltip: {
        ...TIP_FRAME, trigger: 'item',
        formatter: (x: any) => {
          const t = done[x.data?.i]
          if (!t) return ''
          return tip(title(t.session_id), [
            { value: ms(t.elapsed_ms), label: t.failed ? 'the turn, which failed' : 'the turn', color: t.failed ? TONE_MARK.fault : ACCENT, mark: t.failed ? 'triangle' : 'dot' },
            { value: ms(t.first_token_ms), label: 'to the first token', strong: false },
            { value: `${t.loops ?? '?'} · ${t.tool_calls ?? 0}`, label: 'loops · tool calls', strong: false },
            { value: usd(t.cost), label: 'its cost' },
          ], `${stamp(t.start)}${t.model ? ` · ${t.model}` : ''}${t.session_id ? ' · a click opens the session' : ''}`)
        },
      },
      xAxis: timeAxis(span),
      yAxis: { ...vaxis, type: 'log', logBase: 10, axisLabel: { ...vaxis.axisLabel, formatter: msLogTick } },
      series: [
        {
          type: 'scatter', name: 'turn', silent: true,
          data: done.flatMap((t) => (t.failed ? [] : [{ value: at(t), symbolSize: size(t.cost) }])),
          itemStyle: { color: ACCENT, opacity: 0.85, borderColor: C.surface, borderWidth: MARK.ring },
        },
        {
          type: 'scatter', name: 'failed', symbol: 'triangle', silent: true,
          data: done.flatMap((t) => (t.failed ? [{ value: at(t), symbolSize: Math.max(MARK.marker + MARK.ring, size(t.cost)) }] : [])),
          itemStyle: { color: TONE_MARK.fault, borderColor: C.surface, borderWidth: MARK.ring },
        },
        // The hover and click target: wider than the mark, so a reader never has to land on an 8 px dot.
        { type: 'scatter', name: 'hit', symbolSize: 24, z: 5, itemStyle: { color: 'rgba(0,0,0,0)' }, emphasis: { disabled: true }, data: done.map((t, i) => ({ value: at(t), i })) },
      ],
    }
  }, [done, title, span])
  if (!done.length) return <Empty>no finished turns in this range</Empty>
  return <Echart option={option} onClick={(x: any) => { const t = done[x?.data?.i]; if (t?.session_id) onPick?.(t.session_id) }} />
}

// ---------------------------------------------------------------- the tools

/** Each tool's calls, its share that succeeded, its p50 and p95, and its time in all as a bar on one scale. The table is
 *  the form: a failure or a denial says itself in words with its mark, never by colour alone. */
export function ToolBoard({ rows }: { rows: LedgerEntry[] | undefined }) {
  const stats = useMemo(() => toolStats(rows), [rows])
  const maxTime = Math.max(1, ...stats.map((s) => s.durations.reduce((a, b) => a + b, 0)))
  if (!stats.length) return <Empty>no tool calls in this range</Empty>
  return (
    <div className="h-full overflow-auto">
      <table className="viz-table w-full text-[12px]">
        <caption className="sr-only">each tool's calls, successes, p50 and p95, and its time in all</caption>
        <thead>
          <tr>
            <th className="text-left">tool</th><th className="text-right">calls</th><th className="text-right">ok</th>
            <th className="text-right">p50</th><th className="text-right">p95</th><th className="w-[30%] text-left">time in all</th>
          </tr>
        </thead>
        <tbody>
          {stats.map((s) => {
            const total = s.durations.reduce((a, b) => a + b, 0)
            const settled = s.ok + s.failed
            return (
              <tr key={s.tool}>
                <td className="num text-ink">
                  {s.tool}
                  {s.denied > 0 && <span className="ml-1.5 inline-flex items-center gap-0.5 text-ink-faint"><X size={10} style={{ color: TONE_MARK.fault }} aria-hidden />{s.denied} denied</span>}
                </td>
                <td className="num text-right text-ink">{s.calls}</td>
                <td className="num text-right text-ink" title={s.failed ? `${s.failed} of ${settled} failed` : undefined}>
                  {s.failed > 0 && <AlertTriangle size={10} className="mr-1 inline align-[-1px]" style={{ color: TONE_MARK.wait }} aria-hidden />}
                  {settled ? pct(s.ok / settled) : '—'}{s.failed > 0 && <span className="text-ink-faint"> · {s.failed} failed</span>}
                </td>
                <td className="num text-right text-ink-dim">{ms(quantile(s.durations, 0.5))}</td>
                <td className="num text-right text-ink-dim">{ms(quantile(s.durations, 0.95))}</td>
                <td>
                  <div className="flex items-center gap-2">
                    <span className="h-1.5 flex-1"><span className="block h-full" style={{ width: `${(total / maxTime) * 100}%`, minWidth: total > 0 ? 2 : 0, background: ACCENT, borderRadius: '0 3px 3px 0' }} /></span>
                    <span className="num w-[7ch] text-right text-[11px] text-ink-dim">{ms(total)}</span>
                  </div>
                </td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

// ---------------------------------------------------------------- context growth

/** One session's estimated prompt size at each compile, as a step line (a recompile is the drop), with its wash. */
export function ContextGrowth({ rows, sessions }: { rows: LedgerEntry[] | undefined; sessions: SessionInfo[] }) {
  const series = useMemo(() => contextSeries(rows), [rows])
  const option = useMemo<EChartsOption>(() => {
    const title = titleOf(sessions)
    const all = [...series.values()].flat()
    const sc = niceScale(Math.max(0, ...all.map((p) => p[1])))
    const vaxis = valueAxis()
    return {
      grid: { left: 12, right: 18, top: 12, bottom: 22, containLabel: true },
      tooltip: {
        ...TIP_FRAME, trigger: 'axis', axisPointer: { type: 'line', lineStyle: { color: C.axis, width: 1, type: 'solid' } },
        formatter: (ps: any) => {
          const list = (Array.isArray(ps) ? ps : [ps]).filter((p: any) => p?.value)
          if (!list.length) return ''
          return tip(stamp(list[0].value[0]), list.map((p: any): TipRow => ({ value: `${tokens(p.value[1])} tokens`, label: p.seriesName, color: p.color, mark: 'line' })))
        },
      },
      xAxis: timeAxis(),
      yAxis: { ...vaxis, min: 0, max: sc.max, interval: sc.interval, axisLabel: { ...vaxis.axisLabel, formatter: tokenTick(sc.interval) } },
      // The deck draws its one session in the accent; were there more, each would take its slot, never a cycled hue (the
      // Bridge reads many sessions as ranks instead, ContextRanks).
      series: [...series].map(([sid, pts], i) => {
        const color = i < CATEGORICAL.dark.length ? CATEGORICAL.dark[i] : OTHER
        return {
          name: title(sid), type: 'line' as const, step: 'end' as const, data: pts, showSymbol: pts.length <= 30,
          symbol: 'circle', symbolSize: MARK.marker + MARK.ring,
          lineStyle: { width: MARK.line, color, cap: 'round' as const, join: 'round' as const },
          itemStyle: { color, borderColor: C.surface, borderWidth: MARK.ring },
          areaStyle: { color, opacity: MARK.wash },
        }
      }),
    }
  }, [series, sessions])
  if (!series.size) return <Empty>no context compiles in the rows read</Empty>
  return <Echart option={option} />
}

/** The sessions' prompts, the largest first: each session's latest estimated size as a bar on one scale, its compiles as
 *  a sparkline beside it (a recompile is the drop), and its size in the ink. A click opens the session's context. */
export function ContextRanks({ rows, sessions, onPick, top = 12 }: { rows: LedgerEntry[] | undefined; sessions: SessionInfo[]; onPick?: (sid: string) => void; top?: number }) {
  const growth = useMemo(() => growthBySession(contextSeries(rows)), [rows])
  const title = useMemo(() => titleOf(sessions), [sessions])
  if (!growth.length) return <Empty>no context compiles in this range</Empty>
  const max = growth[0].latest
  const shown = growth.slice(0, top)
  const rest = growth.slice(top)
  return (
    <TipArea className="h-full overflow-y-auto overflow-x-hidden px-1">
      {shown.map((g: Growth) => (
        <TipTarget key={g.session} onClick={onPick ? () => onPick(g.session) : undefined} label={`${title(g.session)}: ${tokens(g.latest)} tokens`}
          className="viz-row grid w-full grid-cols-[minmax(0,38%)_1fr_56px_auto] items-center gap-x-2 px-1 py-[5px]"
          tip={<TipBody head={title(g.session)} rows={[
            { value: `${tokens(g.latest)} tokens`, label: 'its prompt now', color: ACCENT, mark: 'rect' },
            { value: `${tokens(g.max)} tokens`, label: 'its largest' },
          ]} foot={`${g.points.length} compile${g.points.length === 1 ? '' : 's'}, ${clock(g.first)} to ${clock(g.last)}${onPick ? ' · a click opens its context' : ''}`} />}>
          <span className="truncate text-left text-[12px] text-ink-dim">{title(g.session)}</span>
          <span className="h-2.5"><span className="block h-full" style={{ width: `${max > 0 ? (g.latest / max) * 100 : 0}%`, minWidth: 2, background: ACCENT, borderRadius: '0 4px 4px 0' }} /></span>
          <span className="w-[56px]"><MiniSpark values={g.points.map((p) => p[1])} height={16} /></span>
          <span className="num w-[6ch] text-right text-[11px] text-ink">{tokens(g.latest)}</span>
        </TipTarget>
      ))}
      {rest.length > 0 && <div className="px-1 py-1 text-[10.5px] text-ink-faint">and {rest.length} more session{rest.length === 1 ? '' : 's'} (the table lists every compile)</div>}
    </TipArea>
  )
}

// ---------------------------------------------------------------- the token mix

/** Each recent model call's tokens as a column stacked by kind, in the token kinds' colours (the money river's and
 *  Economics'), a 2 px gap between kinds and the top one's end rounded; the newest 48 calls. */
export function TokenMix({ calls, title }: { calls: ProviderCall[]; title?: (sid: string | null | undefined) => string }) {
  const last = useMemo(() => calls.slice(-48), [calls])
  const [ref, width] = useWidth<HTMLDivElement>()
  // The 2 px gap between kinds where a column is wide enough to keep its colour after it; a hairline of a column keeps none.
  const gap = width && (width - 70) / Math.max(1, last.length) < 12 ? 0 : MARK.gap / 2
  const option = useMemo<EChartsOption>(() => {
    const per = last.map((c) => kindTokens([c]))
    const vals = TOKEN_KINDS.map((k) => per.map((t) => t[k.key]))
    const tops = stackTop(vals)
    const sc = niceScale(Math.max(1, ...per.map((t) => TOKEN_KINDS.reduce((a, k) => a + t[k.key], 0))))
    const vaxis = valueAxis(), axis = baseAxis()
    // One call a column, labelled by its time; two calls in one second print it once.
    const labels = last.map((c, i) => { const l = clock(c.at); return i > 0 && clock(last[i - 1].at) === l ? '' : l })
    return {
      grid: { left: 12, right: 18, top: 12, bottom: 22, containLabel: true },
      tooltip: {
        ...TIP_FRAME, trigger: 'axis', axisPointer: { type: 'shadow', shadowStyle: { color: 'rgba(176,141,87,0.08)' } },
        formatter: (ps: any) => {
          const i = (Array.isArray(ps) ? ps[0] : ps)?.dataIndex
          const c = last[i], t = per[i]
          if (!c || !t) return ''
          const total = TOKEN_KINDS.reduce((a, k) => a + t[k.key], 0)
          return tip(`${stamp(c.at)} · ${c.model}`, [
            ...TOKEN_KINDS.filter((k) => t[k.key] > 0).map((k): TipRow => ({ value: tokens(t[k.key]), label: k.word, color: k.color, mark: 'rect' })),
            { value: tokens(total), label: 'tokens in all' },
            { value: usd(c.cost), label: 'its cost', strong: false },
          ], title ? title(c.session_id) : undefined)
        },
      },
      xAxis: { type: 'category', data: labels, ...axis, axisLabel: { ...axis.axisLabel, interval: 'auto' as const } },
      yAxis: { ...vaxis, min: 0, max: sc.max, interval: sc.interval, axisLabel: { ...vaxis.axisLabel, formatter: tokenTick(sc.interval) } },
      series: TOKEN_KINDS.map((k, i) => ({
        name: k.word, type: 'bar' as const, stack: 'tokens', barMaxWidth: MARK.bar, barCategoryGap: '24%',
        itemStyle: { color: k.color, borderColor: C.surface, borderWidth: gap },
        data: vals[i].map((v, j) => ({ value: v, itemStyle: { borderRadius: tops[j] === i ? barRadius(false) : 0 } })),
      })),
    }
  }, [last, title, gap])
  if (!calls.length) return <Empty>no model calls in this range</Empty>
  return <div ref={ref} className="h-full w-full"><Echart option={option} /></div>
}
