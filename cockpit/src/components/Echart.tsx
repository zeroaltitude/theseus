// A thin React wrapper over ECharts, tree-shaken to the series and components the cockpit uses.
import { useEffect, useRef } from 'react'
import * as echarts from 'echarts/core'
import {
  BarChart, CustomChart, GaugeChart, HeatmapChart, LineChart, PieChart, SankeyChart, ScatterChart,
  SunburstChart, ThemeRiverChart, TreemapChart,
} from 'echarts/charts'
import {
  DataZoomComponent, GridComponent, LegendComponent, MarkAreaComponent, MarkLineComponent,
  PolarComponent, SingleAxisComponent, TitleComponent, TooltipComponent, VisualMapComponent,
} from 'echarts/components'
import { CanvasRenderer } from 'echarts/renderers'
import { base, type EChartsOption } from '@/lib/chart'
import { useCalm } from '@/lib/calm'
import { useMode } from '@/lib/mode'
import { daylight } from '@/lib/daylight'

echarts.use([
  BarChart, CustomChart, GaugeChart, HeatmapChart, LineChart, PieChart, SankeyChart, ScatterChart,
  SunburstChart, ThemeRiverChart, TreemapChart,
  DataZoomComponent, GridComponent, LegendComponent, MarkAreaComponent, MarkLineComponent,
  PolarComponent, SingleAxisComponent, TitleComponent, TooltipComponent, VisualMapComponent,
  CanvasRenderer,
])

interface Props {
  option: EChartsOption
  className?: string
  style?: React.CSSProperties
  onClick?: (params: unknown) => void
  /** The first dataZoom's window, in percent, after the user moves it. */
  onDataZoom?: (z: { start: number; end: number }) => void
}

export function Echart({ option, className, style, onClick, onDataZoom }: Props) {
  const el = useRef<HTMLDivElement>(null)
  const chart = useRef<echarts.ECharts | null>(null)
  const clickRef = useRef(onClick)
  const zoomRef = useRef(onDataZoom)
  useEffect(() => { clickRef.current = onClick; zoomRef.current = onDataZoom }, [onClick, onDataZoom])

  useEffect(() => {
    if (!el.current) return
    const c = echarts.init(el.current, undefined, { renderer: 'canvas' })
    chart.current = c
    c.on('click', (p) => clickRef.current?.(p))
    c.on('datazoom', () => {
      const dz = (c.getOption() as { dataZoom?: { start?: number; end?: number }[] }).dataZoom?.[0]
      if (dz) zoomRef.current?.({ start: dz.start ?? 0, end: dz.end ?? 100 })
    })
    const ro = new ResizeObserver(() => c.resize())
    ro.observe(el.current)
    return () => {
      ro.disconnect()
      c.dispose()
      chart.current = null
    }
  }, [])

  // Calm (the operator's or the system's reduced motion): every chart draws at once, with no transition. A chart's own
  // tooltip goes over the base's, so each keeps the night-glass frame and sets only what is its own.
  const calm = useCalm((s) => s.calm)
  // Daylight (lib/mode.ts): every night colour in the option goes to its daylight step (lib/daylight.ts).
  const mode = useMode((s) => s.mode)
  useEffect(() => {
    const own = option.tooltip
    const tooltip = own && !Array.isArray(own) ? { ...(base.tooltip as object), ...own } : own ?? base.tooltip
    const o = { ...base, ...option, tooltip, ...(calm ? { animation: false } : {}) } as EChartsOption
    chart.current?.setOption(mode === 'light' ? daylight(o) : o, { notMerge: false, lazyUpdate: true })
  }, [option, calm, mode])

  return <div ref={el} className={className} style={{ width: '100%', height: '100%', ...style }} />
}
