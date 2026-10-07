// A chart's panel, after the chart method (theseus-hnof, `lib/viz.ts`): the theme's frame (the bezel, the engraved title)
// around the method's inside: a legend when there are two series or more (none for one: the title names it), the chart,
// and its table view, the same numbers as rows, behind a toggle in the panel's header and kept in the address
// (`?table=<id>,…`, so a table deep-links). Also the stat tile, and the hover tip of the charts drawn in HTML.
import { createContext, useContext, useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react'
import { animate } from 'motion/react'
import { useCalm } from '@/lib/calm'
import { useTableView } from '@/lib/chartview'
import { useMode } from '@/lib/mode'
import { daylightColor } from '@/lib/daylight'
import { cn } from '@/lib/format'
import { toneHex, type Tone } from '@/lib/taxonomy'
import { CATEGORICAL, CHROME, OTHER } from '@/lib/viz'
import { Panel, Segmented } from './ui'
import '@/viz.css'

export type Mark = 'rect' | 'line' | 'dot' | 'ring' | 'triangle'
export interface LegendItem { key: string; label: string; color: string; mark?: Mark }
export interface Column<R> {
  key: string; label: string; cell: (r: R) => ReactNode
  /** Numbers align right, in tabular figures; names align left. */
  num?: boolean; title?: (r: R) => string | undefined; className?: string
}
export interface TableSpec<R> { columns: Column<R>[]; rows: R[]; rowKey: (r: R) => string; caption: string }

const VIEWS = ['chart', 'table'] as const

export function ChartPanel<R>({ id, title, icon, actions, className, height, legend, table, empty, children }: {
  id: string; title: ReactNode; icon?: ReactNode; actions?: ReactNode; className?: string
  /** The body's height in px, the same for the chart (axis labels included) and its table, which scrolls; unset, the body
   *  grows with its content (a grid of small multiples). */
  height?: number
  legend?: LegendItem[]; table: TableSpec<R>
  /** Shown instead of the chart and the toggle when there is nothing to draw. */
  empty?: ReactNode
  children: ReactNode
}) {
  const [tableOn, setTable] = useTableView(id)
  return (
    <Panel title={title} icon={icon} className={className} bodyClassName="p-2"
      actions={<>{actions}{!empty && <Segmented value={tableOn ? 'table' : 'chart'} options={VIEWS} onChange={(v) => setTable(v === 'table')} />}</>}>
      <div className="viz-body flex flex-col" style={height ? { height } : { maxHeight: 640 }}>
        {empty ? <div className="flex h-full items-center justify-center text-sm text-ink-faint">{empty}</div>
          : tableOn ? <div className="min-h-0 flex-auto overflow-auto"><DataTable {...table} /></div>
          : <>
              {legend && legend.length >= 2 && <Legend items={legend} className="px-2 pb-1 pt-0.5" />}
              <div className="relative min-h-0 flex-auto">{children}</div>
            </>}
      </div>
    </Panel>
  )
}

/** The chart and table toggle for a chart outside a panel (a tab's own chart): the same address key as a panel's. */
export function TableToggle({ id }: { id: string }) {
  const [on, set] = useTableView(id)
  return <Segmented value={on ? 'table' : 'chart'} options={VIEWS} onChange={(v) => set(v === 'table')} />
}

/** The table of a chart outside a panel, in the table view's look. */
export function ChartTable<R>(spec: TableSpec<R>) {
  return <DataTable {...spec} />
}

/** The key: a swatch that mirrors the mark (a rect for bars, a stroke for lines, a dot or ring for markers), and its name
 *  in the ink. Present for two series or more. */
export function Legend({ items, className }: { items: LegendItem[]; className?: string }) {
  if (items.length < 2) return null
  return (
    <ul className={cn('flex flex-wrap items-center gap-x-3.5 gap-y-1 text-[11px] text-ink-dim', className)} aria-label="legend">
      {items.map((i) => (
        <li key={i.key} className="flex min-w-0 items-center gap-1.5">
          <Swatch color={i.color} mark={i.mark} />
          <span className="max-w-[28ch] truncate" title={i.label}>{i.label}</span>
        </li>
      ))}
    </ul>
  )
}

export function Swatch({ color: night, mark = 'rect' }: { color: string; mark?: Mark }) {
  // By day a key's colour is its series' daylight step, as the chart's own marks are (lib/daylight.ts).
  const color = useMode((s) => s.mode) === 'light' ? daylightColor(night) : night
  const style: CSSProperties = mark === 'line' ? { width: 14, height: 2, borderRadius: 1, background: color }
    : mark === 'dot' ? { width: 8, height: 8, borderRadius: 4, background: color }
    : mark === 'ring' ? { width: 8, height: 8, borderRadius: 4, boxShadow: `inset 0 0 0 2px ${color}` }
    : mark === 'triangle' ? { width: 9, height: 8, background: color, clipPath: 'polygon(50% 0, 100% 100%, 0 100%)' }
    : { width: 10, height: 8, borderRadius: 2, background: color }
  return <span aria-hidden className="inline-block shrink-0" style={style} />
}

function DataTable<R>({ columns, rows, rowKey, caption }: TableSpec<R>) {
  return (
    <table className="viz-table w-full text-[12px]">
      <caption className="sr-only">{caption}</caption>
      <thead>
        <tr>{columns.map((c) => <th key={c.key} scope="col" className={c.num ? 'text-right' : 'text-left'}>{c.label}</th>)}</tr>
      </thead>
      <tbody>
        {rows.map((r) => (
          <tr key={rowKey(r)}>
            {columns.map((c) => (
              <td key={c.key} title={c.title?.(r)} className={cn(c.num ? 'num text-right text-ink' : 'text-ink-dim', c.className)}>{c.cell(r)}</td>
            ))}
          </tr>
        ))}
      </tbody>
    </table>
  )
}

/** A single number (the method's stat tile): its label engraved, the value in the ink at proportional figures, and a
 *  line of context. The value glides to a change, except in calm. Optionally: its history as a sparkline; a state, said by
 *  the label's icon in the state's tone (the label's words say it too); and a click through to the page that explains it. */
export function StatTile({ label, icon, value, format, hint, spark, tone, onClick, title }: {
  label: string; icon?: ReactNode; value: number; format: (n: number) => string; hint?: ReactNode
  spark?: number[]; tone?: Tone; onClick?: () => void; title?: string
}) {
  const body = (
    <>
      <div className="flex items-center gap-1.5 font-display text-[10.5px] font-bold uppercase tracking-[0.12em] text-gold/90">
        {icon && <span className="shrink-0" style={tone ? { color: toneHex[tone] } : undefined}>{icon}</span>}
        <span className="truncate">{label}</span>
      </div>
      <div className="viz-figure mt-1.5 text-[26px] font-semibold leading-tight text-ink"><Glide value={value} format={format} /></div>
      <div className="mt-1 truncate text-[11px] text-ink-faint" title={typeof hint === 'string' ? hint : undefined}>{hint}</div>
      {spark && <div className="mt-1.5 pr-1"><MiniSpark values={spark} /></div>}
    </>
  )
  return onClick
    ? <button type="button" onClick={onClick} title={title} className="panel viz-tile block w-full px-3.5 pb-3 pt-3 text-left">{body}</button>
    : <div className="panel px-3.5 pb-3 pt-3" title={title}>{body}</div>
}

/** A tile's history (the method's sparkline): a line in the de-emphasis gray from zero, the newest value a dot in the
 *  accent. Drawn in SVG: a tile is cheap. */
export function MiniSpark({ values, height = 22 }: { values: number[]; height?: number }) {
  // By day, the newest value's dot takes the daylight accent, and its ring the panel's face.
  const day = useMode((s) => s.mode) === 'light'
  if (values.length < 2) return <div className="border-b border-line" style={{ height: height / 2 }} aria-hidden />
  const lo = Math.min(0, ...values), hi = Math.max(...values)
  const span = hi - lo || 1
  const y = (v: number) => 2 + (1 - (v - lo) / span) * (height - 4)
  const pts = values.map((v, i) => `${((i / (values.length - 1)) * 100).toFixed(2)},${y(v).toFixed(2)}`).join(' ')
  return (
    <div className="relative" style={{ height }} aria-hidden>
      <svg width="100%" height={height} viewBox={`0 0 100 ${height}`} preserveAspectRatio="none" className="block overflow-visible">
        <polyline points={pts} fill="none" stroke={OTHER} strokeWidth={1.5} vectorEffect="non-scaling-stroke" strokeLinejoin="round" strokeLinecap="round" />
      </svg>
      <span className="absolute right-0 h-2 w-2 -translate-y-1/2 translate-x-1/2 rounded-full"
        style={{ top: y(values[values.length - 1]), background: day ? daylightColor(CATEGORICAL.dark[0]) : CATEGORICAL.dark[0], boxShadow: `0 0 0 2px ${day ? daylightColor(CHROME.dark.surface) : CHROME.dark.surface}` }} />
    </div>
  )
}

function Glide({ value, format }: { value: number; format: (n: number) => string }) {
  const calm = useCalm((s) => s.calm)
  const [shown, setShown] = useState(value)
  const from = useRef(value)
  useEffect(() => {
    if (calm) { from.current = value; return }
    const ctl = animate(from.current, value, { duration: 0.6, ease: [0.22, 1, 0.36, 1], onUpdate: setShown })
    from.current = value
    return () => ctl.stop()
  }, [value, calm])
  return <>{format(calm ? value : shown)}</>
}

// ---------------------------------------------------------------- the hover tip of a chart drawn in HTML

export interface TipLine { value: string; label: string; color?: string; mark?: Mark }

/** A tip's body: values lead, labels follow, each keyed by a stroke of its colour. React sets every name as text. */
export function TipBody({ head, rows, foot }: { head?: ReactNode; rows: TipLine[]; foot?: ReactNode }) {
  return (
    <div className="text-[12px] leading-[1.45]">
      {head && <div className="mb-0.5 text-[11px] text-ink-faint">{head}</div>}
      <div className="grid grid-cols-[auto_auto_1fr] items-center gap-x-[7px]">
        {rows.map((r, i) => (
          <div key={i} className="contents">
            {r.color ? <Swatch color={r.color} mark={r.mark ?? 'line'} /> : <span />}
            <span className="num text-right font-semibold text-ink">{r.value}</span>
            <span className="max-w-[260px] truncate text-ink-dim">{r.label}</span>
          </div>
        ))}
      </div>
      {foot && <div className="mt-0.5 text-[11px] text-ink-faint">{foot}</div>}
    </div>
  )
}

type Show = (at: { x: number; y: number } | null, body?: ReactNode) => void
const TipCtx = createContext<{ show: Show; box: () => DOMRect | undefined } | null>(null)

/** The area a hover tip lives in: one tip for every target inside it, kept within the area. */
export function TipArea({ children, className }: { children: ReactNode; className?: string }) {
  const ref = useRef<HTMLDivElement>(null)
  const [t, setT] = useState<{ x: number; y: number; w: number; body: ReactNode } | null>(null)
  const [ctx] = useState(() => ({
    show: ((at, body) => setT(at ? { ...at, w: ref.current?.clientWidth ?? 0, body } : null)) as Show,
    box: () => ref.current?.getBoundingClientRect(),
  }))
  return (
    <div ref={ref} className={cn('relative', className)}>
      <TipCtx.Provider value={ctx}>{children}</TipCtx.Provider>
      {t && (
        <div className="brass-tip pointer-events-none absolute w-max max-w-[340px]"
          style={t.x > t.w * 0.55 ? { right: t.w - t.x + 12, top: t.y + 14 } : { left: t.x + 12, top: t.y + 14 }}>
          {t.body}
        </div>
      )}
    </div>
  )
}

/** A mark or a row with a tip on hover and on keyboard focus; a button when it does something. */
export function TipTarget({ tip, children, className, style, onClick, label }: {
  tip: ReactNode; children?: ReactNode; className?: string; style?: CSSProperties; onClick?: () => void; label?: string
}) {
  const ctx = useContext(TipCtx)
  const at = (x: number, y: number) => { const b = ctx?.box(); return b ? { x: x - b.left, y: y - b.top } : null }
  const handlers = {
    onMouseMove: (e: React.MouseEvent) => ctx?.show(at(e.clientX, e.clientY), tip),
    onMouseLeave: () => ctx?.show(null),
    onFocus: (e: React.FocusEvent<HTMLElement>) => { const r = e.currentTarget.getBoundingClientRect(); ctx?.show(at(r.left + Math.min(r.width, 160) / 2, r.bottom - 8), tip) },
    onBlur: () => ctx?.show(null),
  }
  return onClick
    ? <button type="button" aria-label={label} className={className} style={style} onClick={onClick} {...handlers}>{children}</button>
    : <span tabIndex={0} aria-label={label} className={className} style={style} {...handlers}>{children}</span>
}
