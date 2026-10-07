// The cockpit's chart look, after the chart method (`lib/viz.ts`, theseus-hnof): merged under every ECharts option by
// `components/Echart.tsx`. A chart takes its axes from `valueAxis()` and `baseAxis()`, its marks from `MARK`, its
// colours from the validated palettes, and its tooltip's body from `viztip.ts` (every name set as text, never markup).
import type { EChartsOption } from 'echarts'
import { CHROME, FONTS, TIP_FRAME } from './viz'

export type { EChartsOption }

const C = CHROME.dark

/** Transparent canvas, the sans in the ink, and the night-glass tooltip in its brass rim. Merged under every option. */
export const base: EChartsOption = {
  backgroundColor: 'transparent',
  textStyle: { fontFamily: FONTS.sans, color: C.secondary },
  animationDuration: 450,
  animationDurationUpdate: 350,
  tooltip: { ...TIP_FRAME },
}
