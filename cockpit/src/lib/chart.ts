// The cockpit's chart look, shared by every ECharts option.
import type { EChartsOption } from 'echarts'

export type { EChartsOption }

/** Transparent canvas, the cockpit's ink, a dark tooltip. Merged under every option. */
export const base: EChartsOption = {
  backgroundColor: 'transparent',
  textStyle: { fontFamily: 'Inter Variable, ui-sans-serif, sans-serif', color: '#94a3b8' },
  animationDuration: 450,
  animationDurationUpdate: 350,
  tooltip: {
    backgroundColor: 'rgba(7,10,16,0.94)',
    borderColor: 'rgba(148,163,184,0.25)',
    textStyle: { color: '#e2e8f0', fontSize: 12 },
    extraCssText: 'backdrop-filter: blur(6px); border-radius: 8px; box-shadow: 0 8px 24px rgba(0,0,0,.5);',
  },
}

export const axisStyle = {
  axisLine: { lineStyle: { color: 'rgba(148,163,184,0.18)' } },
  axisTick: { show: false },
  axisLabel: { color: '#64748b', fontSize: 10, fontFamily: 'JetBrains Mono Variable, monospace' },
  splitLine: { lineStyle: { color: 'rgba(148,163,184,0.07)' } },
} as const
