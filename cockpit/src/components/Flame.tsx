// A turn's trace as a flame chart: every span the daemon recorded (admission, loops, compiles, provider calls with
// their first byte and first token, settles, tools), laid out on the turn's own microsecond clock.
import { useMemo } from 'react'
import type { Span } from '@protocol'
import { us } from '@/lib/format'
import { toneHex, type Tone } from '@/lib/taxonomy'
import { axisStyle, type EChartsOption } from '@/lib/chart'
import { Echart } from './Echart'
import { Empty } from './ui'

const kindTone: Record<string, Tone> = {
  turn: 'live', lock: 'wait', loop: 'model', compile: 'think', store: 'idle', provider: 'model', tool: 'tool', mark: 'money',
}

export interface Flat { name: string; kind: string; depth: number; start: number; end: number; attrs: unknown }

export function flatten(s: Span, depth = 0, out: Flat[] = []): Flat[] {
  out.push({ name: s.name, kind: s.kind, depth, start: s.start_us, end: s.end_us ?? s.start_us, attrs: s.attrs })
  for (const c of s.children ?? []) flatten(c, depth + 1, out)
  return out
}

/** `cursor` (µs on the turn's clock) draws the replay's line and dims what has not started yet. */
export function Flame({ trace, onPick, cursor }: { trace: Span | null | undefined; onPick?: (f: Flat) => void; cursor?: number | null }) {
  const flat = useMemo(() => (trace ? flatten(trace) : []), [trace])
  const option = useMemo<EChartsOption>(() => {
    const maxDepth = Math.max(0, ...flat.map((f) => f.depth))
    const end = Math.max(1, ...flat.map((f) => f.end))
    return {
      grid: { left: 8, right: 8, top: 6, bottom: 24 },
      tooltip: {
        trigger: 'item',
        formatter: (p: any) => {
          const f = flat[p.dataIndex]
          const a = f.attrs ? JSON.stringify(f.attrs).slice(0, 260).replace(/</g, '&lt;') : ''
          return `<b>${f.name}</b> <span style="color:#c8bb9b">${f.kind}</span><br/>${us(f.start)} → ${us(f.end)} · <b>${us(f.end - f.start)}</b>${a ? `<br/><span style="font-family:monospace;font-size:11px;color:#c8bb9b">${a}</span>` : ''}`
        },
      },
      dataZoom: [{ type: 'inside', xAxisIndex: 0, filterMode: 'weakFilter' }, { type: 'slider', xAxisIndex: 0, height: 14, bottom: 2, borderColor: 'transparent', backgroundColor: 'rgba(176,141,87,0.05)', fillerColor: 'rgba(34,211,238,0.12)', handleSize: 10, showDetail: false }],
      xAxis: { type: 'value', min: 0, max: end, ...axisStyle, axisLabel: { ...axisStyle.axisLabel, formatter: (v: number) => us(v) } },
      // A fixed row count keeps rows a flame chart's height (about 26 px), however shallow the trace is.
      yAxis: { type: 'value', min: 0, max: Math.max(maxDepth + 1, 13), inverse: true, show: false },
      series: [{
        type: 'custom',
        renderItem: (_p: any, api: any) => {
          const depth = api.value(0)
          const s = api.coord([api.value(1), depth])
          const e = api.coord([api.value(2), depth + 1])
          const f = flat[_p.dataIndex]
          const tone = kindTone[f.kind] ?? 'idle'
          const h = Math.max(4, e[1] - s[1] - 3)
          if (f.kind === 'mark') {
            // A mark (first byte, first token) is an instant: a small diamond at the top of its row.
            const cx = s[0]; const cy = s[1] + 6
            return { type: 'polygon' as const, shape: { points: [[cx, cy - 5], [cx + 4, cy], [cx, cy + 5], [cx - 4, cy]] }, style: { fill: toneHex[tone] } }
          }
          const w = Math.max(1.5, e[0] - s[0])
          const future = cursor != null && f.start > cursor
          const base = f.kind === 'loop' || f.kind === 'turn' ? 0.28 : 0.85
          const rect = { type: 'rect' as const, shape: { x: s[0], y: s[1] + 1, width: w, height: h, r: 3 }, style: { fill: toneHex[tone], opacity: future ? base * 0.25 : base } }
          if (w < 46) return rect
          return {
            type: 'group',
            children: [rect, { type: 'text' as const, style: { text: f.name, x: s[0] + 5, y: s[1] + 1 + h / 2, verticalAlign: 'middle', fill: '#efe3c8', font: '11px Inter Variable', width: w - 8, overflow: 'truncate' } }],
          }
        },
        encode: { x: [1, 2], y: 0 },
        data: flat.map((f) => [f.depth, f.start, f.end]),
      },
      ...(cursor != null ? [{
        type: 'line' as const, data: [], silent: true,
        markLine: {
          silent: true, symbol: 'none', animation: false,
          lineStyle: { color: toneHex.live, width: 2, type: 'solid' as const },
          label: { formatter: us(cursor), color: toneHex.live, fontFamily: 'JetBrains Mono Variable', fontSize: 11 },
          data: [{ xAxis: cursor }],
        },
      }] : [])],
    }
  }, [flat, cursor])
  if (!trace) return <Empty>pick a turn with a trace</Empty>
  return <Echart option={option} onClick={(p: any) => onPick?.(flat[p.dataIndex])} />
}
