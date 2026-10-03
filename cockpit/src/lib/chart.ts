// The cockpit's chart look, shared by every ECharts option.
import type { EChartsOption } from 'echarts'

export type { EChartsOption }

/** The cockpit's ink for charts (theseus-logs): ivory text, brass rules, a night-glass tooltip in a brass rim. */
export const ink = { text: '#c8bb9b', faint: '#9c907a', bright: '#efe3c8', rule: 'rgba(176,141,87,0.26)', grid: 'rgba(176,141,87,0.09)' } as const

/** Transparent canvas, the cockpit's ink, a dark tooltip. Merged under every option. */
export const base: EChartsOption = {
  backgroundColor: 'transparent',
  textStyle: { fontFamily: 'Inter Variable, ui-sans-serif, sans-serif', color: ink.text },
  animationDuration: 450,
  animationDurationUpdate: 350,
  tooltip: {
    backgroundColor: 'rgba(6,15,27,0.96)',
    borderColor: 'rgba(214,165,72,0.55)',
    textStyle: { color: ink.bright, fontSize: 12 },
    extraCssText: 'border-radius: 8px; box-shadow: 0 10px 28px rgba(0,0,0,.6), 0 0 16px -6px rgba(34,211,238,.35);',
  },
}

export const axisStyle = {
  axisLine: { lineStyle: { color: ink.rule } },
  axisTick: { show: false },
  axisLabel: { color: ink.faint, fontSize: 10, fontFamily: 'JetBrains Mono Variable, monospace' },
  splitLine: { lineStyle: { color: ink.grid } },
} as const
