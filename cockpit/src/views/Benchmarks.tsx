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
  armsOf, axes, axesOf, axisName, inverted, family, frontier, harness, showsName, CROWDED, theseusOf, interval, points, property, readIndex, reportTitle, runs, sameArm, staircase, SMALL, unequal, UNEQUAL,
  type Note, type Placed, type Point, type Property, type TaskSet, type Unit,
} from '@/lib/bench'
import { benchmarkName, isSlice, setById, setReports, slicePoints, undrawn } from '@/lib/benchslices'
import { REPORTS, useBenchFile, useBenchFiles, useBenchManifest, useBenchSets, useTaskCatalogue } from '@/lib/benchfiles'
import { LEAD, sparkline, type Series } from '@/lib/benchindex'
import { byFraming, FRAMING_WORDS, FRAMINGS, framingOf, modelLabel, plannedArms } from '@/lib/benchframing'
import { BenchTasks } from '@/components/BenchTasks'
import { taskRows } from '@/lib/benchtasks'
import { BenchIndex } from '@/components/BenchIndex'
import { cardsOf } from '@/lib/benchcards'
import { cloudCaption, cloudDrawn, cloudTip, cloudUnit } from '@/lib/benchcloud'
import type { EChartsOption } from '@/lib/chart'
import { CATEGORICAL, CHROME, FONTS, MARK, OTHER, TIP_FRAME, baseAxis, niceStep, usdTick, valueAxis } from '@/lib/viz'
import { tip } from '@/lib/viztip'
import { useMode } from '@/lib/mode'
import { daylightColor } from '@/lib/daylight'
import { Echart } from '@/components/Echart'
import { ChartPanel, Swatch, type Column } from '@/components/ChartPanel'
import { Panel } from '@/components/ui'
import { YourWork } from '@/components/YourWork'

const C = CHROME.dark

/** A harness's colour: its slot in the house palette, or the de-emphasis gray for one with none (Pi, Codex CLI, Aider, OpenCode, OpenHands). */
function harnessColor(key: string): string {
  const slot = harness(key).slot
  return slot === null ? OTHER : CATEGORICAL.dark[slot]
}

/** A point's name on the chart: its harness, and its run where the set holds several. */
function pointName(p: Point, many: boolean): string {
  // A framing-B point says its model ("Codex CLI, GPT-6-Astra"): it is not the model its peers ran.
  const name = [harness(p.harness).name, modelLabel(p)].filter(Boolean).join(', ')
  return many ? `${name}, ${p.runLabel}` : name
}

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

  const { sets: SETS, loading: setsLoading } = useBenchSets()
  // The task sets' files: every report they read — a slice spans the runs that measured its task set, not one.
  const reports = useMemo(() => [...new Set(SETS.flatMap(setReports))], [SETS])
  const csv = useBenchFiles(reports, 'csv'), json = useBenchFiles(reports, 'json'), md = useBenchFiles(reports, 'md')
  const files = useMemo(() => (csv.data && json.data && md.data ? { csv: csv.data, json: json.data, md: md.data } : null), [csv.data, json.data, md.data])
  /** A set's points, however it reads them: a slice gathers its members' runs, a built-in folds its own table. */
  const pointsOf = useMemo(() => (s: TaskSet, by: Unit = 'arm') => !files ? []
    : isSlice(s) ? slicePoints(s, files, by)
      : points(s, files.csv.get(s.report) ?? '', files.json.get(s.report), files.md.get(s.report), s.report.slice(0, 10), by), [files])
  const all = useMemo(() => {
    if (!files || setsLoading) return null
    return new Map(SETS.map((s) => [s.id, pointsOf(s)]))
  }, [files, SETS, setsLoading, pointsOf])

  const set = setById(SETS, params.get('set')) ?? SETS[0]
  const { manifest } = useBenchManifest()
  const missing = useMemo(() => undrawn(SETS, list, manifest.skipped), [SETS, list, manifest.skipped])
  const ps = useMemo(() => all?.get(set.id) ?? [], [all, set.id])
  const { usable, one } = useMemo(() => axes(ps), [ps])
  // The axes in the address when this task set measured them, else its own where it measured those, else the first it
  // did measure; never one property on both. A set that priced no trial (the 10-13 runs, Harbor) opens on an axis it
  // has rather than on an empty chart: a default is only a default where the points carry it.
  const pickAxis = (k: 'x' | 'y', d: string) =>
    usable.find((p) => p.id === params.get(k)) ?? usable.find((p) => p.id === d) ?? usable[0] ?? property(d)!
  const px = pickAxis('x', axesOf(set)[0])
  const asked = pickAxis('y', axesOf(set)[1])
  const py = asked !== px ? asked : property(axesOf(set)[1]) !== px ? property(axesOf(set)[1])! : usable.find((p) => p !== px) ?? asked
  // A task set keeps the axes picked when it measured both of them, else it opens on its own.
  const pickSet = (id: string) => setParams((p) => {
    const next = all?.get(id) ?? []
    const ok = (k: 'x' | 'y') => { const v = p.get(k); return !v || axes(next).usable.some((u) => u.id === v) }
    p.set('set', id)
    if (!ok('x') || !ok('y')) { p.delete('x'); p.delete('y') }
    return p
  }, { replace: true })
  const placed = useMemo(() => frontier(ps, px, py), [ps, px, py])
  // The finer unit behind the arm points: counted always (the caption states the fold), drawn only at ?cloud=on. The
  // frontier and the verdict above are worked out from the arm points and never see these.
  const cloudOn = params.get('cloud') === 'on'
  const unit = useMemo(() => cloudUnit(ps), [ps])
  // The task table (`?tasks=on`) and its drawer (`?task=<id>`): per-task points, counted within this set's one report.
  const tasksOn = params.get('tasks') === 'on' || !!params.get('task')
  const catalogue = useTaskCatalogue()
  const taskTable = useMemo(() => (tasksOn && ps.length ? taskRows(pointsOf(set, 'task')) : []), [tasksOn, pointsOf, set, ps.length])
  const cloud = useMemo(() => (ps.length ? pointsOf(set, unit) : []), [pointsOf, set, ps.length, unit])
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

      <YourWork />

      <Cards list={list} />

      <Newest />

      <ChartPanel<Point> help="bench.frontier"
        id="frontier" height={470}
        title={<span className="flex items-center gap-2">The frontier <span className="font-sans text-[11px] font-normal normal-case tracking-normal text-ink-faint">{set.label}</span></span>}
        icon={<Trophy size={14} />}
        empty={!all ? 'Reading the runs…' : ps.length < 2 ? 'This task set has fewer than two points.' : undefined}
        table={frontierTable(ps, placed, px, py, usable, set)}
        actions={<SetPicker sets={SETS} value={set.id} onChange={pickSet} missing={missing} />}>
        <div className="flex h-full flex-col">
          <AxisRow px={px} py={py} usable={usable} one={one}
            onX={(id) => put('x', id)} onY={(id) => put('y', id)} onSwap={() => setParams((p) => { p.set('x', py.id); p.set('y', px.id); return p }, { replace: true })} />
          <Verdict placed={placed} ps={ps} px={px} py={py} many={new Set(ps.map((p) => p.run)).size > 1} />
          <div className="flex flex-wrap items-center gap-x-3 px-2 pb-1 text-[11px] text-ink-faint">
            <span>{cloudCaption(ps, cloud, unit)}</span>
            <label className="flex items-center gap-1.5 text-ink-dim">
              <input type="checkbox" checked={cloudOn} onChange={(e) => put('cloud', e.target.checked ? 'on' : null)} />show them
            </label>
            <button type="button" className="underline decoration-line underline-offset-2 hover:text-live" onClick={() => put('tasks', tasksOn ? null : 'on')}>
              {tasksOn ? 'hide every test' : 'every test'}
            </button>
          </div>
          <div className="min-h-0 flex-1">
            <FrontierChart placed={placed} px={px} py={py} many={new Set(ps.map((p) => p.run)).size > 1} cloud={cloudOn ? cloud : []}
              onPick={(p) => nav(`/benchmarks/${p.report}`)} />
          </div>
          <Key placed={placed} px={px} py={py} />
        </div>
      </ChartPanel>

      {tasksOn && taskTable.length > 0 && <BenchTasks catalogue={catalogue} rows={taskTable} label={set.label} selected={params.get('task')} onSelect={(t) => put('task', t)} />}

      <Conditions set={set} />

      <Panel help="bench.runs" title="Every run, newest first" icon={<ListOrdered size={14} />} bodyClassName="p-0">
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

/** The browse cards over the manifest and the reports (`lib/benchcards.ts`). */
function Cards({ list }: { list: ReturnType<typeof runs> }) {
  const { loading, manifest } = useBenchManifest()
  const { sets } = useBenchSets()
  const cards = useMemo(() => cardsOf(manifest, list, REPORTS, LEAD, sets), [manifest, list, sets])
  const arms = useBenchFile('arms', 'json')
  const planned = useMemo(() => plannedArms(arms.data), [arms.data])
  if (loading) return null
  return <BenchIndex cards={cards} reason={manifest.reason} planned={planned} skipped={manifest.skipped} />
}

const plainArms = (s: string) => s.replace(/`/g, '')

// ---------------------------------------------------------------- the newest measurements (theseus-jda0z)

/** One line per (benchmark, tier, arm): its newest measured pass rate, as of its date and commit, from the manifest. A plain section: the run list below stays the record. */
function Newest() {
  const { loading, manifest } = useBenchManifest()
  if (loading) return null
  if (manifest.reason) {
    return <section className="panel px-4 py-2.5 text-[12px] text-ink-faint" aria-label="the newest measurements">The newest measurements are not shown: {manifest.reason}.</section>
  }
  const groups = new Map<string, Series[]>()
  for (const s of manifest.series) {
    const name = `${s.benchmark}${s.tier ? `, ${s.tier} tier` : ''}`
    groups.set(name, [...(groups.get(name) ?? []), s])
  }
  return (
    <section className="panel px-4 py-3" aria-label="the newest measurements">
      <h2 className="font-display text-[11px] font-bold uppercase tracking-[0.12em] text-gold/90">The newest measurement of each arm</h2>
      <p className="mt-0.5 text-[11px] text-ink-faint">
        from <span className="font-mono">docs/benchmarks/index.json</span>: a pass rate with its 95% interval, as of the run that measured it.
        A run that only repeated an earlier run’s figures does not move a line.
      </p>
      <div className="mt-2 flex flex-col gap-2">
        {[...groups].sort(([, a], [, b]) => latest(b).localeCompare(latest(a))).map(([name, ss]) => (
          <table key={name} className="viz-table w-full text-[12px]">
            <caption className="text-left text-[11px] font-semibold text-ink-dim">{name}</caption>
            <thead className="sr-only"><tr><th scope="col">arm</th><th scope="col">pass rate</th><th scope="col">size</th><th scope="col">as of</th><th scope="col">history</th></tr></thead>
            <tbody>{ss.map((s) => <NewestRow key={s.key} s={s} />)}</tbody>
          </table>
        ))}
      </div>
    </section>
  )
}

/** The date of a group's newest line: the freshest benchmark leads. */
const latest = (ss: Series[]) => ss.reduce((d, s) => (s.newest.date > d ? s.newest.date : d), '')

function NewestRow({ s }: { s: Series }) {
  const n = s.newest, p = n.pass
  const pc = (x: number) => `${(x * 100).toFixed(1)}%`
  return (
    <tr className="align-top">
      <td className="!max-w-[34ch] !whitespace-normal text-ink">{n.harness.replace(/\s*\(this run\)$/, '')}</td>
      <td className="num text-ink">{pc(p.value)} <span className="text-ink-faint">[{pc(p.lo)}, {pc(p.hi)}]</span></td>
      <td className="num text-ink-dim">{p.n} trials{n.tasks ? `, ${n.tasks} tasks` : ''}</td>
      <td className="text-ink-dim">
        as of {n.date} · <span className="font-mono text-[11px]">{n.commit ? n.commit.slice(0, 8) : 'commit not stated'}</span>{' · '}
        <Link to={`/benchmarks/${n.report}`} className="text-ink-faint underline decoration-line underline-offset-2 hover:text-live">its report</Link>
      </td>
      <td><Spark s={s} /></td>
    </tr>
  )
}

/** The arm's pass rate over the runs that measured it, oldest to newest: a line, the newest point marked. */
function Spark({ s }: { s: Series }) {
  const w = 72, h = 18, pad = 3
  const pts = sparkline(s.history.map((r) => r.pass.value), w - 2 * pad, h - 2 * pad).map(([x, y]) => [x + pad, y + pad])
  const words = `${s.history.length} run${s.history.length === 1 ? '' : 's'}: ${s.history.map((r) => `${r.date} ${(r.pass.value * 100).toFixed(1)}%`).join(', ')}`
  const last = pts[pts.length - 1]
  return (
    <svg width={w} height={h} role="img" aria-label={words}>
      <title>{words}</title>
      {pts.length > 1 && <polyline points={pts.map((q) => q.join(',')).join(' ')} fill="none" stroke={OTHER} strokeWidth="1.5" />}
      <circle cx={last[0]} cy={last[1]} r="2.5" fill={harnessColor(s.arm)} />
    </svg>
  )
}

// ---------------------------------------------------------------- the pickers

/** The task set, as a pulldown (theseus-s5qwv): a chip a set wrapped into a second row and still named only the seven
 *  sets a `ui` block or a built-in made. Every set the manifest describes is here, grouped by its benchmark and
 *  measured newest first; and under them, named rather than hidden, the benchmarks the frontier cannot draw because
 *  they measured no rival arm to put a point beside. */
function SetPicker({ sets, value, onChange, missing }: {
  sets: readonly TaskSet[]; value: string; onChange: (id: string) => void
  missing: { suite: string; runs: string[]; why: string }[]
}) {
  const groups: { name: string; sets: TaskSet[] }[] = []
  for (const s of sets) {
    const name = isSlice(s) ? benchmarkName(s.benchmark) : 'measured across runs'
    const g = groups.find((x) => x.name === name)
    if (g) g.sets.push(s); else groups.push({ name, sets: [s] })
  }
  const arms = (s: TaskSet) => (isSlice(s) ? ` — ${new Set(s.members.map((m) => m.row.arm)).size} harnesses, ${s.members.length} runs` : '')
  return (
    <label className="flex min-w-0 items-center gap-1.5 text-[11px] text-ink-faint">
      <span className="font-display text-[10px] font-bold uppercase tracking-[0.12em] text-gold/90">set</span>
      <select aria-label="task set" value={value} onChange={(e) => onChange(e.target.value)}
        className="max-w-[46ch] truncate rounded-md bg-white/5 px-2 py-1 text-[12px] text-ink outline-none ring-1 ring-line focus:ring-live/40">
        {groups.map((g) => (
          <optgroup key={g.name} label={g.name}>
            {g.sets.map((s) => <option key={s.id} value={s.id} title={s.label}>{s.short}{arms(s)}</option>)}
          </optgroup>
        ))}
        {missing.length > 0 && (
          <optgroup label="published, and not drawn here — open them in the run list below">
            {missing.map((m) => (
              <option key={m.suite} value={value} disabled>
                {m.suite} · {m.runs.length} report{m.runs.length === 1 ? '' : 's'} · {m.why}
              </option>
            ))}
          </optgroup>
        )}
      </select>
    </label>
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
  const theseus = theseusOf(placed)
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
        : <>Theseus is <b className="font-semibold text-ink">behind the frontier</b> here: {theseus.length > 1 ? `none of its ${theseus.length} points is on it; for one, ` : ''}{who(theseus[0].point)} is dominated by {whos(theseus[0].dominators)}, each as good or better on both {py.short} and {px.short}.</>
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

function FrontierChart({ placed, px, py, many, cloud, onPick }: {
  placed: Placed[]; px: Property; py: Property; many: boolean; cloud: Point[]; onPick: (p: Point) => void
}) {
  const option = useMemo<EChartsOption>(() => {
    const vaxis = valueAxis(), axis = baseAxis()
    const xlog = logFor(placed.map((p) => p.x)), ylog = logFor(placed.map((p) => p.y))
    const keys = armsOf(placed)
    const item = (p: Placed) => {
      const color = harnessColor(p.point.harness)
      const showName = showsName(p, placed.length > CROWDED)
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
    const dots = cloudDrawn(cloud, px.id, py.id, xlog, ylog)
    // The cloud is data the reader asked to see, so the frame holds it too (a clipped dot would be a hidden one).
    const span = (axis: 'x' | 'y', pr: Property) => [...placed.flatMap((p) => {
      const i = interval(pr, p.point.values[pr.id])
      return [i.lo, i.hi, axis === 'x' ? p.x : p.y]
    }), ...dots.map((d) => (axis === 'x' ? d.x : d.y))]
    const xe = extent(span('x', px), px, xlog), ye = extent(span('y', py), py, ylog)
    // Ticks at the axis's own step: dollars with the decimals the step needs, so $0.08 and $0.10 read alike.
    const fmt = (p: Property, e: { interval?: number }) => (p.unit === 'usd' && e.interval ? usdTick(e.interval) : (v: number) => p.format(v))
    const ext = (e: { min?: number; max?: number }): [number, number] | undefined => (e.min !== undefined && e.max !== undefined ? [e.min, e.max] : undefined)
    const [ex, ey] = [ext(xe), ext(ye)]
    const { line, legs } = staircase(placed, px, py, ex && ey ? { x: ex, y: ey } : undefined)
    return {
      grid: { left: 64, right: 150, top: 18, bottom: 46, containLabel: false },
      tooltip: {
        ...TIP_FRAME, trigger: 'item',
        formatter: (raw: unknown) => {
          const prm = raw as { data?: { id?: string; cloud?: number } }
          if (prm.data?.cloud !== undefined) {
            const c = cloudTip(dots[prm.data.cloud].p, px.id, py.id), el = tip(c.title, [])
            for (const l of c.lines) { const d = document.createElement('div'); d.style.fontSize = '11px'; d.textContent = l; el.append(d) }
            return el
          }
          const p = placed.find((x) => x.point.id === prm.data?.id)
          return p ? pointTip(p, px, py, many) : ''
        },
      },
      xAxis: {
        ...vaxis, type: xlog ? 'log' : 'value', ...xe, inverse: inverted(px), name: axisName(px, 'x', xlog), nameLocation: 'middle', nameGap: 28,
        nameTextStyle: { color: C.secondary, fontSize: 11, fontFamily: FONTS.sans },
        axisLine: axis.axisLine, splitLine: { show: false }, axisLabel: { ...vaxis.axisLabel, formatter: fmt(px, xe), hideOverlap: true },
      },
      yAxis: {
        ...vaxis, type: ylog ? 'log' : 'value', ...ye, inverse: inverted(py), name: axisName(py, 'y', ylog), nameLocation: 'middle', nameGap: 50,
        nameTextStyle: { color: C.secondary, fontSize: 11, fontFamily: FONTS.sans },
        axisLabel: { ...vaxis.axisLabel, formatter: fmt(py, ye) },
      },
      series: [
        // The frontier: its points joined, keeping to the better side, in the ink (not a series colour: it is no arm's).
        { type: 'line', name: 'the frontier', data: line, symbol: 'none', silent: true, z: 1, lineStyle: { color: C.secondary, width: 1.5 }, animationDurationUpdate: 350 },
        // Its legs to the frame, dashed and fainter: where the frontier's points say a harness would be beaten, not measured.
        ...legs.map((leg, i) => ({ type: 'line' as const, name: `the frontier's leg ${i + 1}`, data: leg, symbol: 'none', silent: true, z: 1, lineStyle: { color: C.secondary, width: 1.5, type: 'dashed' as const, opacity: 0.6 } })),
        // The data behind the arm points: faint, in the arm's colour, no labels, a tip only.
        ...(dots.length ? [{
          type: 'scatter' as const, name: 'behind the points', z: 1.5, symbolSize: 4, silent: false, cursor: 'default',
          data: dots.map((d, i) => ({ value: [d.x, d.y], cloud: i, itemStyle: { color: harnessColor(d.p.harness), opacity: 0.3 } })),
        }] : []),
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
          data: placed.filter((p) => sameArm(p.point.harness) === k).map(item),
          labelLayout: { hideOverlap: false, moveOverlap: 'shiftY' as const },
        })),
        // The hit targets: 24 px round every point, so a point is easy to find with the pointer and to click.
        { type: 'scatter', name: 'hit', symbolSize: 24, z: 5, cursor: 'pointer', itemStyle: { color: 'rgba(0,0,0,0)' }, emphasis: { disabled: true },
          data: placed.map((p) => ({ value: [p.x, p.y], id: p.point.id })) },
      ],
    } as EChartsOption
  }, [placed, px, py, many, cloud])
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
    `${harness(pt.harness).name}${family(pt.harness) !== pt.harness ? ` (${pt.harness})` : ''} ${pt.version?.text ?? '(its build not stated)'} · ${pt.model ?? 'model not stated'}`,
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
function Key({ placed, px, py }: { placed: Placed[]; px: Property; py: Property }) {
  const day = useMode((s) => s.mode) === 'light'
  const ink = day ? daylightColor(C.secondary) : C.secondary
  const keys = armsOf(placed)
  // The legend in framing blocks (derived from the data): the controlled comparison first, then Theseus over every model, then the model-varying arms.
  const blocks = byFraming(keys, (k) => framingOf(placed.find((p) => sameArm(p.point.harness) === k)!.point))
  return (
    <ul className="flex flex-wrap items-center gap-x-3.5 gap-y-1 px-2 pt-1 text-[11px] text-ink-dim" aria-label="legend">
      {blocks.map((b) => (
        <li key={b.framing} className="flex basis-full flex-wrap items-center gap-x-3.5 gap-y-1" aria-label={b.words}>
          {blocks.length > 1 && (
            <span className="text-[10px] font-semibold uppercase tracking-wide text-ink-faint">
              {b.words}{b.items.length === 1 ? ' · one arm measured so far' : ''}
            </span>
          )}
          {b.items.map((k) => (
            <span key={k} className="flex items-center gap-1.5"><Swatch color={harnessColor(k)} mark="dot" />{harness(k).name}{harness(k).slot === null ? ' (other: no slot of its own)' : ''}</span>
          ))}
        </li>
      ))}
      <li className="flex items-center gap-1.5">
        <svg width="14" height="2" aria-hidden><rect width="14" height="2" rx="1" fill={ink} /></svg>the frontier
      </li>
      <li className="flex items-center gap-1.5">
        <svg width="14" height="2" aria-hidden><path d="M0 1 H14" stroke={ink} strokeWidth="1.5" strokeDasharray="3 2" opacity="0.6" /></svg>its legs to the frame (where the frontier’s points beat a harness: not measured)
      </li>
      {[px, py].filter(inverted).map((p) => (
        <li key={p.id} className="text-ink-faint">{p.short}: lower is better, so lower is {p === px ? 'to the right' : 'up'}</li>
      ))}
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
    <Panel help="bench.differs" title="What differs between the harnesses here" icon={<FlaskConical size={14} />} bodyClassName="px-4 py-2.5">
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
    { key: 'framing', label: 'compared as', cell: (p) => FRAMING_WORDS[framingOf(p)] },
    { key: 'version', label: 'version', cell: (p) => p.version?.text ?? 'not stated' },
    { key: 'model', label: 'model', cell: (p) => p.model ?? '—' },
    { key: 'effort', label: 'effort', cell: (p) => p.effort?.text ?? 'not recorded', title: (p) => p.effort?.text },
    { key: 'date', label: 'date', cell: (p) => p.date },
    { key: 'n', label: 'trials', num: true, cell: (p) => p.n },
    ...props.map((pr) => ({ key: pr.id, label: pr.short, num: true, cell: (p: Point) => cell(p, pr), title: () => `${pr.label}: ${pr.about}` })),
    { key: 'tags', label: 'its own conditions', cell: (p) => p.tags.map((t) => `${UNEQUAL.includes(t.kind) ? '◆ ' : ''}${t.text}`).join('; ') || '—', title: (p) => p.tags.map((t) => t.text).join('; ') },
  ]
  return { caption: `every point of ${set.label}, every property it measured`, rows: [...ps].sort((a, b) => FRAMINGS.indexOf(framingOf(a)) - FRAMINGS.indexOf(framingOf(b))), rowKey: (p: Point) => p.id, columns }
}
