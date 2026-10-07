// A turn's trace as a flame chart: every span the daemon recorded (admission, loops, compiles, provider calls with
// their first byte and first token, settles, tools), laid out on the turn's own microsecond clock. After the chart
// method (`lib/viz.ts`, theseus-hnof): each kind of span in its tone's step for marks, the turn and its loops as a wash
// (they hold the rest), a span's name inside it only where it fits and in whichever ink stands out from it, and a tip
// built as DOM (a span's attributes are data, set as text).
import { useMemo } from 'react'
import type { Span } from '@protocol'
import { us } from '@/lib/format'
import type { EChartsOption } from '@/lib/chart'
import { CHROME, FONTS, TIP_FRAME, TONE_MARK, baseAxis, inkOn } from '@/lib/viz'
import { tip } from '@/lib/viztip'
import { useMode } from '@/lib/mode'
import { daylightColor } from '@/lib/daylight'
import { flatten, spanColor, type Flat } from '@/lib/spans'
import { Echart } from './Echart'
import { Empty } from './ui'

const C = CHROME.dark

/** `cursor` (µs on the turn's clock) draws the replay's line and dims what has not started yet. */
export function Flame({ trace, onPick, cursor }: { trace: Span | null | undefined; onPick?: (f: Flat) => void; cursor?: number | null }) {
  const flat = useMemo(() => (trace ? flatten(trace) : []), [trace])
  // By day (lib/mode.ts): renderItem draws its own shapes, where daylight() cannot reach, so it paints them itself.
  const day = useMode((s) => s.mode) === 'light'
  const option = useMemo<EChartsOption>(() => {
    const paint = (c: string) => (day ? daylightColor(c) : c)
    const maxDepth = Math.max(0, ...flat.map((f) => f.depth))
    const end = Math.max(1, ...flat.map((f) => f.end))
    const axis = baseAxis()
    return {
      grid: { left: 8, right: 8, top: 6, bottom: 24 },
      tooltip: {
        ...TIP_FRAME, trigger: 'item',
        formatter: (p: any) => {
          const f = flat[p.dataIndex]
          if (!f) return ''
          const a = f.attrs ? JSON.stringify(f.attrs).slice(0, 260) : ''
          return tip(f.name, [
            { value: us(f.end - f.start), label: f.kind === 'mark' ? 'a mark' : `a ${f.kind} span`, color: f.name === 'judge' ? TONE_MARK.think : spanColor(f.kind), mark: f.kind === 'mark' ? 'dot' : 'rect' },
            { value: `${us(f.start)} → ${us(f.end)}`, label: 'on the turn\'s clock', strong: false },
          ], a || undefined)
        },
      },
      dataZoom: [{ type: 'inside', xAxisIndex: 0, filterMode: 'weakFilter' }, { type: 'slider', xAxisIndex: 0, height: 14, bottom: 2, borderColor: 'transparent', backgroundColor: 'rgba(176,141,87,0.05)', fillerColor: 'rgba(34,211,238,0.12)', handleSize: 10, showDetail: false }],
      xAxis: { type: 'value', min: 0, max: end, ...axis, splitLine: { show: true, lineStyle: { color: C.grid, width: 1, type: 'solid' } }, axisLabel: { ...axis.axisLabel, formatter: (v: number) => us(v) } },
      // A fixed row count keeps rows a flame chart's height (about 26 px), however shallow the trace is.
      yAxis: { type: 'value', min: 0, max: Math.max(maxDepth + 1, 13), inverse: true, show: false },
      series: [{
        type: 'custom',
        renderItem: (_p: any, api: any) => {
          const depth = api.value(0)
          const s = api.coord([api.value(1), depth])
          const e = api.coord([api.value(2), depth + 1])
          const f = flat[_p.dataIndex]
          const h = Math.max(4, e[1] - s[1] - 2)
          if (f.kind === 'mark') {
            // A mark (first byte, first token) is an instant: a small diamond at the top of its row, in a ring of the
            // surface. A judgment's dispatch (M5 23b, `judge`) is a larger one, in the thinking tone's step.
            const cx = s[0]; const cy = s[1] + 6
            const r = f.name === 'judge' ? 7 : 5
            return { type: 'polygon' as const, shape: { points: [[cx, cy - r], [cx + r - 1, cy], [cx, cy + r], [cx - r + 1, cy]] }, style: { fill: f.name === 'judge' ? TONE_MARK.think : spanColor('mark'), stroke: paint(C.surface), lineWidth: 1.5 } }
          }
          // A 2 px gap of the surface between neighbours, and the data end rounded.
          const w = Math.max(1.5, e[0] - s[0] - 1)
          const future = cursor != null && f.start > cursor
          const wash = f.kind === 'loop' || f.kind === 'turn'
          const fill = spanColor(f.kind)
          const opacity = (wash ? 0.3 : 1) * (future ? 0.25 : 1)
          const rect = { type: 'rect' as const, shape: { x: s[0], y: s[1] + 1, width: w, height: h, r: 3 }, style: { fill, opacity } }
          if (w < 46) return rect
          return {
            type: 'group',
            children: [rect, { type: 'text' as const, style: { text: f.name, x: s[0] + 5, y: s[1] + 1 + h / 2, verticalAlign: 'middle', fill: wash ? paint(C.text) : inkOn(fill, day ? 'light' : 'dark'), opacity: future ? 0.4 : 1, font: `11px ${FONTS.sans}`, width: w - 8, overflow: 'truncate', ellipsis: '…' } }],
          }
        },
        encode: { x: [1, 2], y: 0 },
        data: flat.map((f) => [f.depth, f.start, f.end]),
      },
      ...(cursor != null ? [{
        type: 'line' as const, data: [], silent: true,
        markLine: {
          silent: true, symbol: 'none', animation: false,
          lineStyle: { color: C.text, width: 1, type: 'solid' as const },
          label: { formatter: us(cursor), color: C.text, fontFamily: FONTS.mono, fontSize: 11 },
          data: [{ xAxis: cursor }],
        },
      }] : [])],
    }
  }, [flat, cursor, day])
  if (!trace) return <Empty>pick a turn with a trace</Empty>
  return <Echart option={option} onClick={(p: any) => onPick?.(flat[p.dataIndex])} />
}
