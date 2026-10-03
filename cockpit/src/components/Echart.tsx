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

  // Calm (the operator's or the system's reduced motion): every chart draws at once, with no transition.
  const calm = useCalm((s) => s.calm)
  useEffect(() => {
    chart.current?.setOption({ ...base, ...option, ...(calm ? { animation: false } : {}) } as EChartsOption, { notMerge: false, lazyUpdate: true })
  }, [option, calm])

  return <div ref={el} className={className} style={{ width: '100%', height: '100%', ...style }} />
}
