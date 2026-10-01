// Instruments shared by the Bridge and the session deck: turns, tools, context growth, token mix.
import { useMemo } from 'react'
import type { LedgerEntry, SessionInfo } from '@protocol'
import { contextSeries, quantile, toolStats, turnRows, type ProviderCall } from '@/lib/derive'
import { clock, ms, pct, short, stamp, tokens, usd } from '@/lib/format'
import { toneHex } from '@/lib/taxonomy'
import { Echart } from './Echart'
import { axisStyle, type EChartsOption } from '@/lib/chart'
import { Empty, Meter } from './ui'

const titleOf = (sessions: SessionInfo[]) => {
  const m = new Map(sessions.map((s) => [s.session_id, s.title || s.label || short(s.session_id)]))
  return (sid: string | null | undefined) => (sid ? m.get(sid) ?? short(sid) : '—')
}

/** Each turn as a bubble: when it ran, how long it took, and (by size) what it cost. Click to open its session. */
export function TurnsChart({ rows, sessions, onPick }: { rows: LedgerEntry[] | undefined; sessions: SessionInfo[]; onPick?: (sid: string) => void }) {
  const turns = useMemo(() => turnRows(rows).filter((t) => t.elapsed_ms !== undefined), [rows])
  const option = useMemo<EChartsOption>(() => {
    const title = titleOf(sessions)
    const maxCost = Math.max(0.0001, ...turns.map((t) => t.cost ?? 0))
    return {
      grid: { left: 48, right: 14, top: 14, bottom: 24 },
      tooltip: {
        trigger: 'item',
        formatter: (p: any) => {
          const t = turns[p.dataIndex]
          return `<b>${title(t.session_id)}</b><br/>${stamp(t.start)}<br/>${ms(t.elapsed_ms)} · first token ${ms(t.first_token_ms)}<br/>${t.loops ?? '?'} loops · ${t.tool_calls ?? 0} tools · ${usd(t.cost)}${t.failed ? '<br/><span style="color:#fb7185">failed</span>' : ''}`
        },
      },
      xAxis: { type: 'time', ...axisStyle, splitLine: { show: false } },
      yAxis: { type: 'log', logBase: 10, ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => ms(v) } },
      series: [{
        type: 'scatter',
        data: turns.map((t) => ({
          value: [t.start, Math.max(1, t.elapsed_ms ?? 1)],
          symbolSize: 8 + 26 * Math.sqrt((t.cost ?? 0) / maxCost),
          itemStyle: {
            color: t.failed ? toneHex.fault : toneHex.live, opacity: 0.75,
            borderColor: t.failed ? toneHex.fault : toneHex.model, borderWidth: 1,
            shadowBlur: 12, shadowColor: t.failed ? `${toneHex.fault}88` : `${toneHex.live}66`,
          },
        })),
      }],
    }
  }, [turns, sessions])
  if (!turns.length) return <Empty>no finished turns in the rows read</Empty>
  return <Echart option={option} onClick={(p: any) => { const t = turns[p.dataIndex]; if (t?.session_id) onPick?.(t.session_id) }} />
}

export function ToolBoard({ rows }: { rows: LedgerEntry[] | undefined }) {
  const stats = useMemo(() => toolStats(rows), [rows])
  const maxTime = Math.max(1, ...stats.map((s) => s.durations.reduce((a, b) => a + b, 0)))
  if (!stats.length) return <Empty>no tool calls in the rows read</Empty>
  return (
    <div className="h-full overflow-auto">
      <table className="w-full whitespace-nowrap text-[12px]">
        <thead className="sticky top-0 bg-hull/95 text-[10px] uppercase tracking-wider text-ink-faint backdrop-blur">
          <tr>
            <th className="px-3 py-1.5 text-left font-semibold">tool</th>
            <th className="px-2 py-1.5 text-right font-semibold">calls</th>
            <th className="px-2 py-1.5 text-right font-semibold">ok</th>
            <th className="px-2 py-1.5 text-right font-semibold">p50</th>
            <th className="px-2 py-1.5 text-right font-semibold">p95</th>
            <th className="w-24 px-3 py-1.5 text-left font-semibold">time</th>
          </tr>
        </thead>
        <tbody>
          {stats.map((s) => {
            const total = s.durations.reduce((a, b) => a + b, 0)
            const settled = s.ok + s.failed
            return (
              <tr key={s.tool} className="border-t border-line/60 hover:bg-white/[0.03]">
                <td className="num px-3 py-1 text-tool">{s.tool}{s.denied ? <span className="ml-1 text-fault">· {s.denied} denied</span> : null}</td>
                <td className="num px-2 py-1 text-right text-ink">{s.calls}</td>
                <td className="num px-2 py-1 text-right" style={{ color: s.failed ? toneHex.wait : toneHex.ok }}>{settled ? pct(s.ok / settled) : '—'}</td>
                <td className="num px-2 py-1 text-right text-ink-dim">{ms(quantile(s.durations, 0.5))}</td>
                <td className="num px-2 py-1 text-right text-ink-dim">{ms(quantile(s.durations, 0.95))}</td>
                <td className="px-3 py-1"><Meter value={total} max={maxTime} tone="tool" /></td>
              </tr>
            )
          })}
        </tbody>
      </table>
    </div>
  )
}

export function ContextGrowth({ rows, sessions }: { rows: LedgerEntry[] | undefined; sessions: SessionInfo[] }) {
  const series = useMemo(() => contextSeries(rows), [rows])
  const option = useMemo<EChartsOption>(() => {
    const title = titleOf(sessions)
    const palette = [toneHex.think, toneHex.live, toneHex.model, toneHex.tool, toneHex.ok, toneHex.money, toneHex.wait]
    return {
      grid: { left: 46, right: 12, top: 26, bottom: 24 },
      legend: { top: 0, left: 0, type: 'scroll', itemWidth: 10, itemHeight: 6, textStyle: { color: '#94a3b8', fontSize: 10 } },
      tooltip: { trigger: 'axis', valueFormatter: (v: any) => `${tokens(Number(v))} tokens` },
      xAxis: { type: 'time', ...axisStyle, splitLine: { show: false } },
      yAxis: { type: 'value', ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => tokens(v) } },
      series: [...series.entries()].map(([sid, pts], i) => ({
        name: title(sid), type: 'line', step: 'end', showSymbol: pts.length < 30, symbolSize: 5, data: pts,
        lineStyle: { width: 1.6, color: palette[i % palette.length] }, itemStyle: { color: palette[i % palette.length] },
        areaStyle: { color: palette[i % palette.length], opacity: 0.06 },
      })),
    }
  }, [series, sessions])
  if (!series.size) return <Empty>no context compiles in the rows read</Empty>
  return <Echart option={option} />
}

export function TokenMix({ calls }: { calls: ProviderCall[] }) {
  const last = calls.slice(-48)
  const option = useMemo<EChartsOption>(() => ({
    grid: { left: 46, right: 12, top: 26, bottom: 22 },
    legend: { top: 0, left: 0, itemWidth: 10, itemHeight: 6, textStyle: { color: '#94a3b8', fontSize: 10 } },
    tooltip: { trigger: 'axis', axisPointer: { type: 'shadow' }, valueFormatter: (v: any) => tokens(Number(v)) },
    xAxis: { type: 'category', data: last.map((c) => clock(c.at).slice(0, 5)), ...axisStyle, splitLine: { show: false } },
    yAxis: { type: 'value', ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => tokens(v) } },
    series: [
      { name: 'cache read', type: 'bar', stack: 't', data: last.map((c) => c.usage.cache_read_input_tokens), itemStyle: { color: toneHex.think, opacity: 0.85 } },
      { name: 'cache write', type: 'bar', stack: 't', data: last.map((c) => c.usage.cache_creation_input_tokens), itemStyle: { color: toneHex.model, opacity: 0.85 } },
      { name: 'input', type: 'bar', stack: 't', data: last.map((c) => c.usage.input_tokens), itemStyle: { color: toneHex.live, opacity: 0.85 } },
      { name: 'output', type: 'bar', stack: 't', data: last.map((c) => c.usage.output_tokens), itemStyle: { color: toneHex.money, opacity: 0.9, borderRadius: [2, 2, 0, 0] } },
    ],
  }), [last])
  if (!calls.length) return <Empty>no model calls in the rows read</Empty>
  return <Echart option={option} />
}
