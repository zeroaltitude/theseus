// Benchmarks (theseus-raf4): every run the project measured and published, and where Theseus sits among the harnesses
// it was measured against. The frontier is a Pareto chart of measured points (a harness in one run, on one task set),
// on two properties the operator picks, each with its better direction: the non-dominated points joined by the frontier,
// the dominated ones dimmed, every point a link to its run's report. Under it, every run, newest first; each opens its
// report whole (`/benchmarks/<report>`, BenchRun.tsx).
//
// Only measured data: the runs are `docs/benchmarks/` as this build embeds it (`lib/benchfiles.ts`), the points their
// per-trial tables folded by `lib/bench.ts`. The view reads nothing from the daemon and writes nothing: it is the
// record of finished runs, so it has no live state and no time machine's past.
import { useEffect, useMemo } from 'react'
import { Link, useNavigate, useSearchParams } from 'react-router'
import { ArrowLeftRight, FlaskConical, ListOrdered, Trophy } from 'lucide-react'
import {
  axes, frontier, harness, interval, points, property, readIndex, reportTitle, runs, staircase, SMALL, TASK_SETS, unequal, UNEQUAL,
  type Note, type Placed, type Point, type Property, type TaskSet,
} from '@/lib/bench'
import { REPORTS, useBenchFile, useBenchFiles } from '@/lib/benchfiles'
import type { EChartsOption } from '@/lib/chart'
import { CATEGORICAL, CHROME, FONTS, MARK, OTHER, TIP_FRAME, baseAxis, niceStep, usdTick, valueAxis } from '@/lib/viz'
import { tip } from '@/lib/viztip'
import { useMode } from '@/lib/mode'
import { daylightColor } from '@/lib/daylight'
import { cn } from '@/lib/format'
import { Echart } from '@/components/Echart'
import { ChartPanel, Swatch, type Column } from '@/components/ChartPanel'
import { Panel } from '@/components/ui'

const C = CHROME.dark

/** Each task set's axes when the address names none: the question its report asked first. */
const DEFAULT_AXES: Record<string, [string, string]> = {
  tb2: ['usd', 'pass'], fixgit: ['rss', 'usd'], worth: ['usd', 'agent-s'], async: ['respond-s', 'usd'],
}

/** A harness's colour: its slot in the house palette, or the de-emphasis gray for one with none (Pi). */
function harnessColor(key: string): string {
  const slot = harness(key).slot
  return slot === null ? OTHER : CATEGORICAL.dark[slot]
}

/** A point's name on the chart: its harness, and its run where the set holds several. */
function pointName(p: Point, many: boolean): string {
  return many ? `${harness(p.harness).name}, ${p.runLabel}` : harness(p.harness).name
}

const arrow = (p: Property, axis: 'x' | 'y') =>
  axis === 'x' ? (p.better === 'lower' ? '← better' : 'better →') : (p.better === 'lower' ? '↓ better' : '↑ better')

export default function Benchmarks() {
  const nav = useNavigate()
  const [params, setParams] = useSearchParams()
  const put = (k: string, v: string | null) => setParams((p) => { if (v) p.set(k, v); else p.delete(k); return p }, { replace: true })

  const readme = useBenchFile('README', 'md')
  const index = useMemo(() => readIndex(readme.data ?? ''), [readme.data])
  // The titles of reports the index lacks come from their own first heading, read only then.
  const unindexed = useMemo(() => (readme.data ? REPORTS.filter((n) => !index.some((r) => r.name === n)) : []), [readme.data, index])
  const extra = useBenchFiles(unindexed, 'md')
  const list = useMemo(() => runs(REPORTS, index, new Map([...(extra.data ?? new Map<string, string>())].map(([n, md]) => [n, reportTitle(md) ?? n]))), [index, extra.data])

  // The task sets' files: four reports' tables, data files and words.
  const setReports = useMemo(() => [...new Set(TASK_SETS.map((s) => s.report))], [])
  const csv = useBenchFiles(setReports, 'csv'), json = useBenchFiles(setReports, 'json'), md = useBenchFiles(setReports, 'md')
  const all = useMemo(() => {
    const [c, j, m] = [csv.data, json.data, md.data]
    if (!c || !j || !m) return null
    return new Map(TASK_SETS.map((s) => [s.id, points(s, c.get(s.report) ?? '', j.get(s.report), m.get(s.report), s.report.slice(0, 10))]))
  }, [csv.data, json.data, md.data])

  const set = TASK_SETS.find((s) => s.id === params.get('set')) ?? TASK_SETS[0]
  const ps = useMemo(() => all?.get(set.id) ?? [], [all, set.id])
  const { usable, one } = useMemo(() => axes(ps), [ps])
  // The axes in the address when this task set measured them, else its own; never one property on both.
  const pickAxis = (k: 'x' | 'y', d: string) => usable.find((p) => p.id === params.get(k)) ?? property(d)!
  const px = pickAxis('x', DEFAULT_AXES[set.id][0])
  const asked = pickAxis('y', DEFAULT_AXES[set.id][1])
  const py = asked !== px ? asked : property(DEFAULT_AXES[set.id][1]) !== px ? property(DEFAULT_AXES[set.id][1])! : usable.find((p) => p !== px) ?? asked
  // A task set keeps the axes picked when it measured both of them, else it opens on its own.
  const pickSet = (id: string) => setParams((p) => {
    const next = all?.get(id) ?? []
    const ok = (k: 'x' | 'y') => { const v = p.get(k); return !v || axes(next).usable.some((u) => u.id === v) }
    p.set('set', id)
    if (!ok('x') || !ok('y')) { p.delete('x'); p.delete('y') }
    return p
  }, { replace: true })
  const placed = useMemo(() => frontier(ps, px, py), [ps, px, py])
  // Every property's intervals for this task set, worked out while the page is idle, so a pick of any axis draws at
  // once (a bootstrap of 178 trials is tens of milliseconds; each value's is kept once worked out).
  useEffect(() => {
    const work = usable.flatMap((pr) => ps.filter((p) => p.values[pr.id]).map((p) => () => interval(pr, p.values[pr.id])))
    let id = 0
    const step = (d: IdleDeadline) => {
      while (work.length && d.timeRemaining() > 4) work.shift()!()
      if (work.length) id = requestIdleCallback(step)
    }
    id = requestIdleCallback(step)
    return () => cancelIdleCallback(id)
  }, [ps, usable])

  return (
    <div className="flex flex-col gap-3">
      <div className="panel flex flex-wrap items-end gap-x-6 gap-y-2 px-4 py-3">
        <div className="mr-2">
          <h1 className="ship-title !text-[26px]">Benchmarks</h1>
          <div className="mt-1 text-[11.5px] text-ink-dim">every run we measured and published, and where Theseus sits among the harnesses we ran beside it</div>
        </div>
        <div className="ml-auto text-right text-[11px] text-ink-faint">
          {list.length} reports in <span className="font-mono">docs/benchmarks/</span>, newest {list[0]?.date ?? '—'}
          <div>as this build embeds them: measured data only, nothing live</div>
        </div>
      </div>

      <ChartPanel<Point>
        id="frontier" height={470}
        title={<span className="flex items-center gap-2">The frontier <span className="font-sans text-[11px] font-normal normal-case tracking-normal text-ink-faint">{set.label}</span></span>}
        icon={<Trophy size={14} />}
        empty={!all ? 'Reading the runs…' : ps.length < 2 ? 'This task set has fewer than two points.' : undefined}
        table={frontierTable(ps, placed, px, py, usable, set)}
        actions={<SetPicker value={set.id} onChange={pickSet} />}>
        <div className="flex h-full flex-col">
          <AxisRow px={px} py={py} usable={usable} one={one}
            onX={(id) => put('x', id)} onY={(id) => put('y', id)} onSwap={() => setParams((p) => { p.set('x', py.id); p.set('y', px.id); return p }, { replace: true })} />
          <Verdict placed={placed} ps={ps} px={px} py={py} many={new Set(ps.map((p) => p.run)).size > 1} />
          <div className="min-h-0 flex-1">
            <FrontierChart placed={placed} px={px} py={py} many={new Set(ps.map((p) => p.run)).size > 1}
              onPick={(p) => nav(`/benchmarks/${p.report}`)} />
          </div>
          <Key placed={placed} />
        </div>
      </ChartPanel>

      <Conditions set={set} />

      <Panel title="Every run, newest first" icon={<ListOrdered size={14} />} bodyClassName="p-0">
        <div className="overflow-x-auto">
          <table className="viz-table w-full text-[12px]">
            <caption className="sr-only">every published benchmark run, newest first</caption>
            <thead>
              <tr>
                <th scope="col" className="text-left">date</th><th scope="col" className="text-left">suite</th>
                <th scope="col" className="text-left">run</th><th scope="col" className="text-left">arms</th>
                <th scope="col" className="text-left">headline</th>
              </tr>
            </thead>
            <tbody>
              {list.map((r) => (
                <tr key={r.name} className="cursor-pointer align-top" onClick={() => nav(`/benchmarks/${r.name}`)}>
                  <td className="num text-ink-dim">{r.date}</td>
                  <td className="font-mono text-[11px] text-ink-dim">{r.suite}</td>
                  <td className="!max-w-[46ch] !whitespace-normal">
                    <Link to={`/benchmarks/${r.name}`} className="text-ink hover:text-live" onClick={(e) => e.stopPropagation()}>{r.title}</Link>
                  </td>
                  <td className="!max-w-[28ch] !whitespace-normal text-ink-dim">{r.arms ? plainArms(r.arms) : '—'}</td>
                  <td className="!max-w-[70ch] !whitespace-normal text-ink-dim">{r.headline ?? '—'}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </Panel>
    </div>
  )
}

const plainArms = (s: string) => s.replace(/`/g, '')

// ---------------------------------------------------------------- the pickers

function SetPicker({ value, onChange }: { value: string; onChange: (id: string) => void }) {
  return (
    <div className="flex flex-wrap items-center rounded-md bg-black/30 p-0.5 ring-1 ring-line" role="tablist" aria-label="task set">
      {TASK_SETS.map((s) => (
        <button key={s.id} type="button" role="tab" aria-selected={s.id === value} title={s.label} onClick={() => onChange(s.id)}
          className={cn('rounded px-1.5 py-0.5 text-[10px] font-medium transition-colors', s.id === value ? 'bg-live/15 text-live shadow-[0_0_10px_-2px_rgba(34,211,238,0.5)]' : 'text-ink-faint hover:text-ink')}>
          {s.short}
        </button>
      ))}
    </div>
  )
}

function AxisRow({ px, py, usable, one, onX, onY, onSwap }: {
  px: Property; py: Property; usable: Property[]; one: { p: Property; who: string[] }[]
  onX: (id: string) => void; onY: (id: string) => void; onSwap: () => void
}) {
  const pick = (id: string, value: Property, on: (id: string) => void, label: string, other: Property) => (
    <label className="flex min-w-0 items-center gap-1.5 text-[11px] text-ink-faint">
      <span className="font-display text-[10px] font-bold uppercase tracking-[0.12em] text-gold/90">{label}</span>
      <select id={id} value={value.id} onChange={(e) => on(e.target.value)}
        className="num rounded-md bg-white/5 px-2 py-1 text-[12px] text-ink outline-none ring-1 ring-line focus:ring-live/40">
        {usable.map((p) => <option key={p.id} value={p.id} disabled={p === other}>{p.label} · {p.better} is better</option>)}
        {one.length > 0 && (
          <optgroup label="measured for one harness only">
            {one.map(({ p, who }) => <option key={p.id} value={p.id} disabled>{p.label} · {who.join(', ')} only</option>)}
          </optgroup>
        )}
      </select>
    </label>
  )
  return (
    <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 px-2 pb-1.5 pt-0.5">
      {pick('bench-y', py, onY, 'up', px)}
      {pick('bench-x', px, onX, 'across', py)}
      <button type="button" onClick={onSwap} title="swap the axes" aria-label="swap the axes"
        className="rounded-md p-1 text-ink-faint ring-1 ring-line hover:text-ink"><ArrowLeftRight size={13} /></button>
    </div>
  )
}

// ---------------------------------------------------------------- where Theseus sits, in words

function Verdict({ placed, ps, px, py, many }: { placed: Placed[]; ps: Point[]; px: Property; py: Property; many: boolean }) {
  const theseus = placed.filter((p) => p.point.harness === 'theseus')
  const missing = ps.length - placed.length
  let words: React.ReactNode
  if (!placed.length) words = 'No point measured both of these.'
  else if (!theseus.length) words = 'Theseus has no point with both of these.'
  else {
    const on = theseus.filter((p) => p.frontier)
    const off = theseus.find((p) => !p.frontier)
    const who = (p: Point) => <b className="font-semibold text-ink">{pointName(p, many)}</b>
    // The nearest three by name; the rest counted (the tip and the table name them all).
    const whos = (all: Point[]) => {
      const ds = all.slice(0, 3), more = all.length - ds.length
      return <>{ds.map((d, i) => <span key={d.id}>{i ? (i === ds.length - 1 && !more ? ' and ' : ', ') : ''}{who(d)}</span>)}{more > 0 && ` and ${more} more`}</>
    }
    words = on.length === theseus.length
      ? <>Theseus is <b className="font-semibold text-ink">on the frontier</b>{theseus.length === 2 ? ' in both of its points' : theseus.length > 2 ? ` in all ${theseus.length} of its points` : ''}: no harness measured here beats it on both {py.short} and {px.short}.</>
      : on.length
        ? <>Theseus is on the frontier in {on.length} of its {theseus.length} points; {who(off!.point)} is dominated by {whos(off!.dominators)}.</>
        : <>Theseus is <b className="font-semibold text-ink">behind the frontier</b> here: {who(theseus[0].point)} is dominated by {whos(theseus[0].dominators)}, each as good or better on both {py.short} and {px.short}.</>
  }
  return (
    <div className="px-2 pb-1 text-[12px] text-ink-dim">
      {words}
      {missing > 0 && <span className="text-ink-faint"> {missing} point{missing === 1 ? '' : 's'} measured only one of the two, so {missing === 1 ? 'it is' : 'they are'} not drawn (the table has {missing === 1 ? 'it' : 'them'}).</span>}
    </div>
  )
}

// ---------------------------------------------------------------- the chart

/** A log axis where a property spans more than thirty times over (memory, setup, CPU), so each harness reads. */
function logFor(vals: number[]): boolean {
  return vals.length > 1 && vals.every((v) => v > 0) && Math.max(...vals) / Math.min(...vals) >= 30
}

function FrontierChart({ placed, px, py, many, onPick }: {
  placed: Placed[]; px: Property; py: Property; many: boolean; onPick: (p: Point) => void
}) {
  const option = useMemo<EChartsOption>(() => {
    const vaxis = valueAxis(), axis = baseAxis()
    const xlog = logFor(placed.map((p) => p.x)), ylog = logFor(placed.map((p) => p.y))
    const keys = [...new Set(placed.map((p) => p.point.harness))]
    const item = (p: Placed) => {
      const color = harnessColor(p.point.harness)
      const showName = p.frontier || harness(p.point.harness).slot === null
      return {
        value: [p.x, p.y], id: p.point.id, name: pointName(p.point, many),
        symbol: unequal(p.point) ? 'diamond' : 'circle',
        symbolSize: (unequal(p.point) ? 13 : MARK.marker) + MARK.ring,
        itemStyle: p.frontier
          ? { color, borderColor: C.surface, borderWidth: MARK.ring }
          : { color: C.surface, borderColor: color, borderWidth: MARK.ring, opacity: 0.45 },
        label: {
          show: showName, formatter: pointName(p.point, many).replace(/[{}|]/g, ''), position: 'right' as const, distance: 7,
          color: p.frontier ? C.text : C.muted, fontSize: 11, fontFamily: FONTS.sans,
          // On the glass's face, so the frontier's line or a whisker passing behind a name never strikes it through.
          backgroundColor: C.surface, padding: [1, 3], borderRadius: 3,
        },
      }
    }
    // Each point's 95% interval on each axis (Wilson for a rate, a seeded bootstrap for a mean or a median, the range
    // under ten trials), as whiskers: dominance is read on the points, the whiskers say how far to trust it.
    const whiskers = placed.flatMap((p) => {
      const ix = interval(px, p.point.values[px.id]), iy = interval(py, p.point.values[py.id])
      const op = p.frontier ? 0.75 : 0.28
      return [
        { value: [ix.lo, p.y, ix.hi, p.y, ix.kind === 'range' ? op * 0.6 : op], itemStyle: { color: harnessColor(p.point.harness) } },
        { value: [p.x, iy.lo, p.x, iy.hi, iy.kind === 'range' ? op * 0.6 : op], itemStyle: { color: harnessColor(p.point.harness) } },
      ].filter((w) => w.value[0] !== w.value[2] || w.value[1] !== w.value[3])
    })
    const line = staircase(placed, px, py)
    // Each axis spans its points and their whiskers, with air on both sides and ends at clean steps, so no mark sits
    // on the frame and no tick reads 89.1%.
    const extent = (vals: number[], pr: Property, log: boolean): { min?: number; max?: number; interval?: number } => {
      if (!vals.length) return {}
      let lo = Math.min(...vals), hi = Math.max(...vals)
      if (log) return { min: 10 ** Math.floor(Math.log10(lo / 1.2)), max: 10 ** Math.ceil(Math.log10(hi * 1.2)) }
      const pad = (hi - lo || Math.abs(hi) || 1) * 0.08
      const step = niceStep(hi - lo + 2 * pad, 5)
      lo = Math.floor((lo - pad) / step) * step; hi = Math.ceil((hi + pad) / step) * step
      if (pr.fold === 'rate') { lo = Math.max(0, lo); hi = Math.min(1, hi) } else if (Math.min(...vals) >= 0) lo = Math.max(0, lo)
      return { min: +lo.toPrecision(12), max: +hi.toPrecision(12), interval: step }
    }
    const span = (axis: 'x' | 'y', pr: Property) => placed.flatMap((p) => {
      const i = interval(pr, p.point.values[pr.id])
      return [i.lo, i.hi, axis === 'x' ? p.x : p.y]
    })
    const xe = extent(span('x', px), px, xlog), ye = extent(span('y', py), py, ylog)
    // Ticks at the axis's own step: dollars with the decimals the step needs, so $0.08 and $0.10 read alike.
    const fmt = (p: Property, e: { interval?: number }) => (p.unit === 'usd' && e.interval ? usdTick(e.interval) : (v: number) => p.format(v))
    const name = (p: Property, a: 'x' | 'y', log: boolean) => `${p.label}${log ? ' (log scale)' : ''}   ${arrow(p, a)}`
    return {
      grid: { left: 64, right: 150, top: 18, bottom: 46, containLabel: false },
      tooltip: {
        ...TIP_FRAME, trigger: 'item',
        formatter: (raw: unknown) => {
          const prm = raw as { data?: { id?: string } }
          const p = placed.find((x) => x.point.id === prm.data?.id)
          return p ? pointTip(p, px, py, many) : ''
        },
      },
      xAxis: {
        ...vaxis, type: xlog ? 'log' : 'value', ...xe, name: name(px, 'x', xlog), nameLocation: 'middle', nameGap: 28,
        nameTextStyle: { color: C.secondary, fontSize: 11, fontFamily: FONTS.sans },
        axisLine: axis.axisLine, splitLine: { show: false }, axisLabel: { ...vaxis.axisLabel, formatter: fmt(px, xe), hideOverlap: true },
      },
      yAxis: {
        ...vaxis, type: ylog ? 'log' : 'value', ...ye, name: name(py, 'y', ylog), nameLocation: 'middle', nameGap: 50,
        nameTextStyle: { color: C.secondary, fontSize: 11, fontFamily: FONTS.sans },
        axisLabel: { ...vaxis.axisLabel, formatter: fmt(py, ye) },
      },
      series: [
        // The frontier: its points joined, keeping to the better side, in the ink (not a series colour: it is no arm's).
        { type: 'line', name: 'the frontier', data: line, symbol: 'none', silent: true, z: 1, lineStyle: { color: C.secondary, width: 1.5 }, animationDurationUpdate: 350 },
        ...(whiskers.length ? [{
          type: 'custom' as const, name: '95% intervals', silent: true, z: 2,
          data: whiskers,
          encode: { x: [0, 2], y: [1, 3] },
          renderItem: (_: unknown, api: { value: (i: number) => number; coord: (v: number[]) => number[]; visual: (k: string) => string }) => {
            const a = api.coord([api.value(0), api.value(1)]), b = api.coord([api.value(2), api.value(3)])
            return { type: 'line', shape: { x1: a[0], y1: a[1], x2: b[0], y2: b[1] }, style: { stroke: api.visual('color'), lineWidth: 1.5, opacity: api.value(4) } }
          },
        }] : []),
        ...keys.map((k) => ({
          type: 'scatter' as const, name: harness(k).name, z: 3, silent: true,
          itemStyle: { color: harnessColor(k) },
          data: placed.filter((p) => p.point.harness === k).map(item),
          labelLayout: { hideOverlap: false, moveOverlap: 'shiftY' as const },
        })),
        // The hit targets: 24 px round every point, so a point is easy to find with the pointer and to click.
        { type: 'scatter', name: 'hit', symbolSize: 24, z: 5, cursor: 'pointer', itemStyle: { color: 'rgba(0,0,0,0)' }, emphasis: { disabled: true },
          data: placed.map((p) => ({ value: [p.x, p.y], id: p.point.id })) },
      ],
    } as EChartsOption
  }, [placed, px, py, many])
  return <Echart option={option} onClick={(raw) => {
    const id = (raw as { data?: { id?: string } }).data?.id
    const p = placed.find((x) => x.point.id === id)
    if (p) onPick(p.point)
  }} />
}

/** A value's interval in words: "[64.9%, 78.0%]" with its kind, or nothing for one trial. */
function spreadWords(pr: Property, v: Point['values'][string]): string {
  if (v.n < 2 && pr.fold !== 'rate') return ''
  const i = interval(pr, v)
  return `[${pr.format(i.lo)}, ${pr.format(i.hi)}${i.kind === 'range' ? ', the range' : ''}]`
}

function pointTip(p: Placed, px: Property, py: Property, many: boolean): HTMLElement {
  const pt = p.point, color = harnessColor(pt.harness)
  const val = (pr: Property) => {
    const v = pt.values[pr.id]
    if (!v) return '–'
    return `${pr.format(v.v)} ${spreadWords(pr, v)}`
  }
  const rows = [
    { value: val(py), label: py.label, color, mark: 'dot' as const },
    { value: val(px), label: px.label, color, mark: 'dot' as const },
    { value: `${pt.n}`, label: `trials${pt.tasks > 1 ? `, ${pt.tasks} tasks` : ''}`, strong: false },
  ]
  // The point's who, when and how, a line each, after the numbers: they wrap, where a row's label would be cut.
  const lines = [
    `${harness(pt.harness).name} ${pt.version?.text ?? '(its build not stated)'} · ${pt.model ?? 'model not stated'}`,
    `effort: ${pt.effort?.text ?? 'not recorded'}`,
    `${pt.runLabel}, ${pt.date}`,
    ...pt.tags.map((t) => `${UNEQUAL.includes(t.kind) ? '◆ ' : ''}${t.text}`),
    p.frontier ? 'on the frontier' : `dominated by ${p.dominators.map((d) => pointName(d, many)).join(', ')}`,
    'click: its report',
  ]
  const el = tip(pointName(pt, many), rows)
  for (const [i, l] of lines.entries()) {
    const d = document.createElement('div')
    Object.assign(d.style, { fontSize: '11px', marginTop: i ? '1px' : '4px', maxWidth: '380px', whiteSpace: 'normal', opacity: i === lines.length - 1 ? '0.75' : '0.9' })
    d.textContent = l
    el.append(d)
  }
  return el
}

/** The chart's key: the harnesses' colours, then what a shape, a hollow point and the line mean. */
function Key({ placed }: { placed: Placed[] }) {
  const day = useMode((s) => s.mode) === 'light'
  const ink = day ? daylightColor(C.secondary) : C.secondary
  const keys = [...new Set(placed.map((p) => p.point.harness))]
  return (
    <ul className="flex flex-wrap items-center gap-x-3.5 gap-y-1 px-2 pt-1 text-[11px] text-ink-dim" aria-label="legend">
      {keys.map((k) => (
        <li key={k} className="flex items-center gap-1.5"><Swatch color={harnessColor(k)} mark="dot" />{harness(k).name}{harness(k).slot === null ? ' (other: no slot of its own)' : ''}</li>
      ))}
      <li className="flex items-center gap-1.5">
        <svg width="14" height="2" aria-hidden><rect width="14" height="2" rx="1" fill={ink} /></svg>the frontier
      </li>
      <li className="flex items-center gap-1.5">
        <svg width="10" height="10" aria-hidden><circle cx="5" cy="5" r="3.5" fill="none" stroke={ink} strokeWidth="2" opacity="0.6" /></svg>dominated (dimmed)
      </li>
      <li className="flex items-center gap-1.5">
        <svg width="12" height="10" aria-hidden><path d="M1 5 H11 M6 0.5 V9.5" stroke={ink} strokeWidth="1.5" opacity="0.7" /></svg>95% intervals (Wilson for a rate, a seeded bootstrap for a mean or median; the range under {SMALL} trials)
      </li>
      <li className="flex items-center gap-1.5">
        <svg width="11" height="11" aria-hidden><path d="M5.5 0.5 L10.5 5.5 L5.5 10.5 L0.5 5.5 Z" fill={ink} /></svg>ran under conditions its peers did not share
      </li>
    </ul>
  )
}

// ---------------------------------------------------------------- the conditions

function Conditions({ set }: { set: TaskSet }) {
  const cite = (n: Note) => (
    <Link to={`/benchmarks/${n.report}`} className="text-ink-faint underline decoration-line underline-offset-2 hover:text-live" title={`“${n.quote}”`}>its report</Link>
  )
  return (
    <Panel title="What differs between the harnesses here" icon={<FlaskConical size={14} />} bodyClassName="px-4 py-2.5">
      <p className="text-[12px] text-ink-dim">{set.about}</p>
      <ul className="mt-1.5 list-disc space-y-0.5 pl-5 text-[12px] text-ink-dim marker:text-gold/60">
        {set.conditions.map((n) => <li key={n.text}>{n.text} <span className="text-[11px]">({cite(n)})</span></li>)}
      </ul>
      <p className="mt-1.5 text-[11px] text-ink-faint">
        A point’s own conditions are in its tip and the table. Under {SMALL} trials a point is a size, not a ranking. Each note quotes its report
        (hover “its report”), and the cockpit’s tests hold every quote to the report’s words.
      </p>
    </Panel>
  )
}

// ---------------------------------------------------------------- the table view

function frontierTable(ps: Point[], placed: Placed[], px: Property, py: Property, usable: Property[], set: TaskSet) {
  const at = new Map(placed.map((p) => [p.point.id, p]))
  const many = new Set(ps.map((p) => p.run)).size > 1
  const props = [py, px, ...usable.filter((p) => p !== px && p !== py)]
  const cell = (pt: Point, pr: Property) => {
    const v = pt.values[pr.id]
    if (!v) return '–'
    return `${pr.format(v.v)} ${spreadWords(pr, v)}`.trim()
  }
  const columns: Column<Point>[] = [
    { key: 'point', label: 'point', cell: (p) => <Link to={`/benchmarks/${p.report}`} className="text-ink hover:text-live">{pointName(p, many)}</Link> },
    { key: 'place', label: `on ${py.short} × ${px.short}`, cell: (p) => { const q = at.get(p.id); return !q ? 'not drawn: it lacks one of the two' : q.frontier ? 'on the frontier' : `dominated by ${q.dominators.map((d) => pointName(d, many)).join(', ')}` } },
    { key: 'version', label: 'version', cell: (p) => p.version?.text ?? 'not stated' },
    { key: 'model', label: 'model', cell: (p) => p.model ?? '—' },
    { key: 'effort', label: 'effort', cell: (p) => p.effort?.text ?? 'not recorded', title: (p) => p.effort?.text },
    { key: 'date', label: 'date', cell: (p) => p.date },
    { key: 'n', label: 'trials', num: true, cell: (p) => p.n },
    ...props.map((pr) => ({ key: pr.id, label: pr.short, num: true, cell: (p: Point) => cell(p, pr), title: () => `${pr.label}: ${pr.about}` })),
    { key: 'tags', label: 'its own conditions', cell: (p) => p.tags.map((t) => `${UNEQUAL.includes(t.kind) ? '◆ ' : ''}${t.text}`).join('; ') || '—', title: (p) => p.tags.map((t) => t.text).join('; ') },
  ]
  return { caption: `every point of ${set.label}, every property it measured`, rows: ps, rowKey: (p: Point) => p.id, columns }
}
