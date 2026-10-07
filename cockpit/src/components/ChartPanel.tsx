// A chart's panel, after the chart method (theseus-hnof, `lib/viz.ts`): the theme's frame (the bezel, the engraved title)
// around the method's inside: a legend when there are two series or more (none for one: the title names it), the chart,
// and its table view, the same numbers as rows, behind a toggle in the panel's header and kept in the address
// (`?table=<id>,…`, so a table deep-links). Also the stat tile, and the hover tip of the charts drawn in HTML.
import { createContext, useContext, useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react'
import { useSearchParams } from 'react-router'
import { animate } from 'motion/react'
import { useCalm } from '@/lib/calm'
import { cn } from '@/lib/format'
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

/** This panel's view, in the address: `?table=` lists the panels showing their table. */
function useTableView(id: string): [boolean, (on: boolean) => void] {
  const [params, setParams] = useSearchParams()
  const on = (params.get('table') ?? '').split(',').includes(id)
  const set = (v: boolean) => setParams((p) => {
    const s = new Set((p.get('table') ?? '').split(',').filter(Boolean))
    if (v) s.add(id); else s.delete(id)
    if (s.size) p.set('table', [...s].join(',')); else p.delete('table')
    return p
  }, { replace: true })
  return [on, set]
}

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

export function Swatch({ color, mark = 'rect' }: { color: string; mark?: Mark }) {
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
 *  line of context. The value glides to a change, except in calm. */
export function StatTile({ label, icon, value, format, hint }: { label: string; icon?: ReactNode; value: number; format: (n: number) => string; hint?: ReactNode }) {
  return (
    <div className="panel px-3.5 pb-3 pt-3">
      <div className="flex items-center gap-1.5 font-display text-[10.5px] font-bold uppercase tracking-[0.12em] text-gold/90">
        {icon}<span className="truncate">{label}</span>
      </div>
      <div className="viz-figure mt-1.5 text-[26px] font-semibold leading-tight text-ink"><Glide value={value} format={format} /></div>
      <div className="mt-1 truncate text-[11px] text-ink-faint" title={typeof hint === 'string' ? hint : undefined}>{hint}</div>
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
