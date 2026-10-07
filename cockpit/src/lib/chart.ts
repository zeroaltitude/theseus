// The cockpit's chart look, after the chart method (`lib/viz.ts`, theseus-hnof): merged under every ECharts option by
// `components/Echart.tsx`. A chart takes its axes from `valueAxis()` and `baseAxis()`, its marks from `MARK`, its
// colours from the validated palettes, and its tooltip's body from `viztip.ts` (every name set as text, never markup).
import type { EChartsOption } from 'echarts'
import { CHROME, FONTS, TIP_FRAME } from './viz'

export type { EChartsOption }

const C = CHROME.dark

/** The cockpit's ink for charts: ivory text, brass rules, the night glass. */
export const ink = { text: C.secondary, faint: C.muted, bright: C.text, rule: C.axis, grid: C.grid } as const

/** Transparent canvas, the sans in the ink, and the night-glass tooltip in its brass rim. Merged under every option. */
export const base: EChartsOption = {
  backgroundColor: 'transparent',
  textStyle: { fontFamily: FONTS.sans, color: C.secondary },
  animationDuration: 450,
  animationDurationUpdate: 350,
  tooltip: { ...TIP_FRAME },
}

/** The method's quiet axes, for a chart that spreads one style on both of its axes: a hairline baseline, no ticks, muted
 *  labels that hide rather than overlap, and a solid hairline grid (a category or time axis turns its grid off). A
 *  chart drawn after the method takes `valueAxis()` and `baseAxis()` instead. */
export const axisStyle = {
  axisLine: { lineStyle: { color: C.axis, width: 1, type: 'solid' as const } },
  axisTick: { show: false },
  axisLabel: { color: C.muted, fontSize: 10, fontFamily: FONTS.mono, hideOverlap: true },
  splitLine: { lineStyle: { color: C.grid, width: 1, type: 'solid' as const } },
} as const
