// The frontier's slices (theseus-s5qwv): every arm the project measured on one task set, gathered from the manifest
// (`docs/benchmarks/index.json`) instead of from a single report's per-trial table.
//
// Before this a task set was one report's CSV, and a set existed at all only when its dataset carried a hand-written
// `ui` block. Two of the 54 datasets have one, so the page offered seven chips; and because a set read one table, its
// default drew four arms while the manifest held eighteen rows on that same 89-task set. A slice is one (benchmark,
// tier, tasks, task set); its members are that set's measured rows, each drawn as its own point and named with its run.
//
// Two things the manifest knows that a table does not:
//   - A row with `carried_from` repeats an earlier report's figures rather than measuring again. It is not a second
//     point; it is the same point cited again, folded into the member it repeats (`alsoIn`). Ten of the default
//     slice's eighteen rows are such citations.
//   - A single-arm run's table may name no arm at all, and the 10-21 rerun's table spells its one arm `theseus-routed`
//     while the manifest names two theseus variants in that report. So a member's figures come from its table when its
//     arm resolves there *and folds to the figure the manifest states*, and from the manifest otherwise.
import { type Point, type Spread, type TaskSet, type Unit, type Value, parseCsv, points } from './bench.ts'
import type { IndexRow } from './benchindex.ts'

/** One measured arm of a slice, with the reports that cite it and what it did not share with its peers. */
export interface SliceMember {
  row: IndexRow
  /** Later reports that cite this measurement instead of making one of their own, oldest first. */
  alsoIn: string[]
  /** What this member did not share with the rest of its slice, each in its own words. */
  differs: string[]
  /** When it was measured, against the slice's newest run: said on every member, not only the old ones. */
  when: string
}

/** A task set the manifest describes, offered to the frontier as any other set is. */
export interface Slice extends TaskSet {
  benchmark: string
  tier: string | null
  taskCount: number | null
  members: SliceMember[]
  /** Set ids an older address may name (a `ui` block's set this slice absorbed), so a saved link still opens. */
  aliases: string[]
}

export const isSlice = (s: TaskSet): s is Slice => Array.isArray((s as Slice).members)

/** The reports a set reads: a slice's members', else its own one. */
export const setReports = (s: TaskSet): string[] =>
  isSlice(s) ? [...new Set(s.members.map((m) => m.row.report))] : [s.report]

// ---------------------------------------------------------------- a slice's words

const NAMES: Record<string, string> = {
  'terminal-bench@2.0': 'Terminal-Bench 2.0',
  'terminal-bench': 'Terminal-Bench',
  'swe-bench': 'SWE-bench Verified',
  'harbor': 'Harbor',
  'bughunt-rs': 'Bug Hunt RS',
}

/** A benchmark's name as the reports write it. */
export const benchmarkName = (b: string): string => NAMES[b] ?? b

const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/(^-|-$)/g, '')

/** The key that makes two rows one comparison: the same benchmark, tier, task count and task set. */
const groupKey = (r: IndexRow) => [r.benchmark, r.tier ?? '', r.tasks ?? 0, r.taskSet ?? 'full'].join('\u0000')

/** A slice's id, which lives in `?set=`. */
const sliceId = (r: IndexRow) =>
  [slug(r.benchmark), r.tier ?? 'untiered', `${r.tasks ?? 0}`, r.taskSet && r.taskSet !== 'full' ? slug(r.taskSet) : '']
    .filter((p) => p !== '').join('-')

const plural = (n: number, one: string, many = `${one}s`) => `${n} ${n === 1 ? one : many}`

/** The pulldown's own words for a slice: what it measures, in one line. */
function shortOf(r: IndexRow): string {
  const whole = !r.taskSet || r.taskSet === 'full'
  return [benchmarkName(r.benchmark), r.tier, `${whole ? '' : 'a subset: '}${plural(r.tasks ?? 0, 'task')}`]
    .filter(Boolean).join(' · ')
}

function labelOf(r: IndexRow, members: readonly SliceMember[]): string {
  const whole = !r.taskSet || r.taskSet === 'full'
  const tasks = whole ? `all ${plural(r.tasks ?? 0, 'task')}` : `${plural(r.tasks ?? 0, 'task')} of it`
  const tier = r.tier ? `, ${r.tier.toUpperCase()} tier` : ''
  return `${benchmarkName(r.benchmark)}${tier}: ${tasks}, ${plural(new Set(members.map((m) => m.row.arm)).size, 'harness', 'harnesses')}`
}

function aboutOf(members: readonly SliceMember[]): string {
  const dates = members.map((m) => m.row.date).sort()
  const arms = new Set(members.map((m) => m.row.arm)).size
  const when = dates[0] === dates[dates.length - 1] ? `on ${dates[0]}` : `from ${dates[0]} to ${dates[dates.length - 1]}`
  const cited = members.reduce((a, m) => a + m.alsoIn.length, 0)
  const also = cited ? `; ${plural(cited, 'later row')} cite${cited === 1 ? 's' : ''} one of these runs rather than measuring again` : ''
  return `Every arm the manifest records on this task set: ${plural(arms, 'harness', 'harnesses')} over ${plural(members.length, 'measured run')}, ${when}${also}.`
}

// ---------------------------------------------------------------- what a member did not share

/** The commonest value among the slice's members, which is what "the slice's own" means here. */
function modal<T>(xs: readonly T[]): T | undefined {
  const n = new Map<string, { v: T; k: number }>()
  for (const x of xs) {
    const key = JSON.stringify(x)
    const g = n.get(key)
    if (g) g.k++; else n.set(key, { v: x, k: 1 })
  }
  return [...n.values()].sort((a, b) => b.k - a.k)[0]?.v
}

/** What a member did not share with the rest of its slice, in the words the page marks it with. A member that shares
 *  everything gets nothing: the diamond has to mean something, so it is never put on a point for nothing. */
function differencesIn(members: SliceMember[]): void {
  const refModel = modal(members.map((m) => m.row.armModel).filter((x): x is string => !!x))
  const refK = modal(members.map((m) => m.row.k).filter((x): x is number => x !== null))
  const refEffort = modal(members.map((m) => m.row.effort.join(', ')).filter(Boolean))
  const newest = members.map((m) => m.row.date).sort().slice(-1)[0]
  for (const m of members) {
    const d: string[] = []
    const { row } = m
    if (row.armModel && refModel && row.armModel !== refModel) d.push(`ran ${row.armModel}, where the rest of this set ran ${refModel}`)
    if (row.k !== null && refK !== undefined && row.k !== refK) d.push(`ran ${plural(row.k, 'attempt')} a task, where the rest ran ${refK}`)
    const effort = row.effort.join(', ')
    if (effort && refEffort && effort !== refEffort) d.push(`ran at ${effort} effort, where the rest ran at ${refEffort}`)
    m.differs = d
    // Its date is said on every point, never only on the old ones: a set spanning several runs says so on each of
    // them, and the mark is kept for a condition that differed, so that it still means something when it is there.
    m.when = row.date === newest ? `measured ${row.date}, this set's newest run`
      : `measured ${row.date}, before this set's newest run of ${newest}: its own build and its own prices`
  }
}

// ---------------------------------------------------------------- the slices

/** The manifest's rows as task sets, newest and most-measured first. A row that cites an earlier report's figures is
 *  folded into the member it cites, by that report and arm; a citation whose measurement this manifest does not hold
 *  stands as a member of its own rather than being dropped. */
export function slicesFrom(rows: readonly IndexRow[]): Slice[] {
  const groups = new Map<string, IndexRow[]>()
  for (const r of rows) {
    const g = groups.get(groupKey(r))
    if (g) g.push(r); else groups.set(groupKey(r), [r])
  }
  const out: Slice[] = []
  for (const g of groups.values()) {
    const measured = g.filter((r) => !r.carried)
    const members: SliceMember[] = measured
      .sort((a, b) => a.date.localeCompare(b.date) || a.variant.localeCompare(b.variant))
      .map((row) => ({ row, alsoIn: [], differs: [], when: '' }))
    for (const c of g.filter((r) => r.carried)) {
      const host = members.find((m) => m.row.report === c.carriedFrom && m.row.arm === c.arm)
        ?? members.find((m) => m.row.arm === c.arm && Math.abs(m.row.pass.value - c.pass.value) < 1e-9)
      // A citation whose own measurement is not in this manifest is the only record of it: keep it as a member.
      if (host) { if (!host.alsoIn.includes(c.report)) host.alsoIn.push(c.report) }
      else members.push({ row: c, alsoIn: [], differs: [], when: '' })
    }
    if (!members.length) continue
    for (const m of members) m.alsoIn.sort()
    members.sort((a, b) => a.row.date.localeCompare(b.row.date) || a.row.variant.localeCompare(b.row.variant))
    differencesIn(members)
    const newest = [...members].sort((a, b) => b.row.date.localeCompare(a.row.date))[0].row
    out.push({
      id: sliceId(newest), label: labelOf(newest, members), short: shortOf(newest), about: aboutOf(members),
      report: newest.report, benchmark: newest.benchmark, tier: newest.tier, taskCount: newest.tasks,
      members, aliases: [],
      // A slice reads a table a member at a time, and `slicePoints` gives each member its own `keep`, `arm` and `run`.
      // These are a `TaskSet`'s shape, and the set-wide reading of its newest report, which is what a caller holding
      // this as a plain `TaskSet` would get.
      keep: () => true, arm: (r) => r.arm, run: () => runOf(newest, members),
      conditions: [], versions: {}, notes: {},
    })
  }
  // The set the page opens on: the most harnesses measured, then the newest, then the most runs behind it.
  return out.sort((a, b) =>
    new Set(b.members.map((m) => m.row.arm)).size - new Set(a.members.map((m) => m.row.arm)).size
    || newestDate(b).localeCompare(newestDate(a))
    || b.members.length - a.members.length)
}

const newestDate = (s: Slice) => s.members.map((m) => m.row.date).sort().slice(-1)[0] ?? ''

/** The sets the frontier offers: the manifest's slices first, each absorbing the hand-written `ui` block of a report it
 *  spans (so its words are kept and the set is not offered twice), then any generated set no slice holds, then the
 *  five built-ins, which span tasks across runs in ways a manifest row does not describe. */
export function frontierSets(rows: readonly IndexRow[], generated: readonly TaskSet[], builtins: readonly TaskSet[]): TaskSet[] {
  const slices = slicesFrom(rows)
  const held = new Set<string>()
  for (const g of generated) {
    const holder = slices.find((s) => s.members.some((m) => m.row.report === g.report))
    if (!holder) continue
    held.add(g.id)
    holder.aliases.push(g.id)
    // Its words, not its reach: the slice keeps its own membership and its own label.
    holder.about = g.about
    holder.conditions = g.conditions
    holder.versions = g.versions
    holder.notes = g.notes
    holder.effort = g.effort
    holder.axes = g.axes
  }
  return [...slices, ...generated.filter((g) => !held.has(g.id)), ...builtins]
}

/** The set an address names, by its id or by a set id it absorbed, so a saved `?set=tb-v1-high` still opens. */
export const setById = (sets: readonly TaskSet[], id: string | null): TaskSet | undefined =>
  id ? sets.find((s) => s.id === id) ?? sets.find((s) => isSlice(s) && s.aliases.includes(id)) : undefined

// ---------------------------------------------------------------- a slice's points

/** How far a folded rate may sit from the figure the manifest states and still be the same measurement. */
const ROUNDED = 1e-4

export interface Files { csv: ReadonlyMap<string, string>; json: ReadonlyMap<string, string>; md: ReadonlyMap<string, string> }

/** A member's key in its report's own table, or null when the table cannot name it. In order: a table with no `arm`
 *  column is wholly this arm's; else the variant, then the arm, by name; else, where this report measured one arm and
 *  its table holds one arm, they are each other. */
export function csvArmOf(member: SliceMember, slice: Slice, csv: string): string | null | undefined {
  if (!csv) return undefined
  const rows = parseCsv(csv)
  if (!rows.length) return undefined
  if (!('arm' in rows[0])) return null
  const arms = [...new Set(rows.map((r) => r.arm))].filter(Boolean)
  if (arms.includes(member.row.variant)) return member.row.variant
  if (arms.includes(member.row.arm)) return member.row.arm
  const mine = slice.members.filter((m) => m.row.report === member.row.report)
  if (arms.length === 1 && mine.length === 1) return arms[0]
  return undefined
}

/** A point from the manifest's own figures, for a member whose table cannot supply it. The intervals are the ones the
 *  run published, carried as published rather than re-derived from figures this build never folded. */
function statedPoint(slice: Slice, m: SliceMember, run: { id: string; label: string }): Point {
  const r = m.row
  // A rate's interval is the manifest's own Wilson, which `interval()` reads from `lo`/`hi` as it does a folded rate's.
  const values: Record<string, Value> = {
    pass: { v: r.pass.value, n: r.pass.n, lo: r.pass.lo, hi: r.pass.hi, min: r.pass.value, max: r.pass.value },
  }
  // A mean's is the report's bootstrap, which this build never resampled: it stands as published.
  if (r.usdPerTrial) {
    const { value: v, lo, hi } = r.usdPerTrial
    values.usd = { v, n: r.trials, lo, hi, min: lo, max: hi, ci: { lo, hi, kind: 'stated' as Spread } }
  }
  const flat = (v: number): Value => ({ v, n: r.trials, min: v, max: v })
  if (r.usdPerSolve !== null) values['usd-solved'] = flat(r.usdPerSolve)
  if (r.agentS !== null) values['agent-s'] = flat(r.agentS)
  return {
    id: `${slice.id}:${run.id}:${r.arm}`, set: slice.id, harness: r.arm, run: run.id, runLabel: run.label,
    report: r.report, date: r.date, n: r.trials, tasks: r.tasks ?? 0, values, tags: [],
    ...(r.armModel ? { armModel: r.armModel } : {}),
    ...(r.commit ? { version: { text: r.commit } } : {}),
  }
}

/** The segments of a report slug that its same-day peers do not all share: what actually differed between two runs of
 *  one arm on one day, which is usually the model (`…-openclaw-astra-high-k1` against `…-openclaw-sonnet-high-k1`
 *  leaves `astra`). Empty only where the slugs match, which a run's id already forbids. */
function slugMark(report: string, peers: readonly string[]): string {
  const theirs = peers.map((p) => new Set(p.split('-')))
  return report.split('-').filter((seg) => !theirs.every((t) => t.has(seg))).join('-')
}

/** A member's run as the chart names it. A point's name is its harness and its run, drawn beside the dot, so the run
 *  has to be short: the date, which is what tells two runs of one arm apart, and a suffix only where one slice holds
 *  two of an arm's runs on the same day. The manifest's long `harness` line is the tip's and the table's.
 *
 *  The suffix is the variant where the variant says something the arm did not — but a variant may simply repeat the
 *  arm (two models of one harness both recorded `openclaw`), and then it tells nothing apart and both runs would be
 *  drawn under one name. Where that happens the report slug's own differing segments do the work instead. */
function runOf(r: IndexRow, members: readonly SliceMember[]): { id: string; label: string } {
  const id = `${r.report}|${r.variant}`
  const sameDay = members.filter((m) => m.row.arm === r.arm && m.row.date === r.date)
  if (sameDay.length <= 1) return { id, label: r.date }
  if (new Set(sameDay.map((m) => m.row.variant)).size === sameDay.length) return { id, label: `${r.date}, ${r.variant}` }
  const mark = slugMark(r.report, sameDay.filter((m) => m.row.report !== r.report).map((m) => m.row.report))
  return { id, label: `${r.date}, ${mark || r.report}` }
}

/** A slice's points: one a member. Its own table where that table can name it and folds to the figure the manifest
 *  states, the manifest's figures otherwise; and in either case its differences from its peers, so a point that did
 *  not share the set's conditions is drawn as one (`unequal`). */
export function slicePoints(slice: Slice, files: Files, by: Unit = 'arm'): Point[] {
  const out: Point[] = []
  for (const m of slice.members) {
    const { row } = m
    const run = runOf(row, slice.members)
    const csv = files.csv.get(row.report) ?? ''
    const arm = csvArmOf(m, slice, csv)
    let folded: Point[] = []
    if (arm !== undefined) {
      const shim: TaskSet = {
        ...slice, report: row.report,
        keep: (r) => arm === null || r.arm === arm,
        arm: () => row.arm,
        run: () => run,
      }
      folded = points(shim, csv, files.json.get(row.report), files.md.get(row.report), row.date, by)
    }
    // The table stands only where it says what the manifest says: a wrong match must never draw the wrong numbers.
    // The manifest rounds a rate to four decimals (the 10-07 arms read 0.8214 for 23/28), so the test is that
    // rounding's own width, which is still four orders of magnitude tighter than any mismatch it has to catch.
    const agrees = folded.length > 0 && (by !== 'arm'
      || (folded.length === 1 && !!folded[0].values.pass && Math.abs(folded[0].values.pass.v - row.pass.value) <= ROUNDED))
    const made = agrees ? folded : by === 'arm' ? [statedPoint(slice, m, run)] : []
    for (const p of made) {
      if (by !== 'arm') { out.push(p); continue }
      const tags = [...p.tags]
      if (!agrees) tags.push({ kind: 'sample', text: `its figures are the manifest's: ${plural(row.trials, 'trial')} on ${plural(row.tasks ?? 0, 'task')}, as ${row.report} published them` })
      for (const text of m.differs) tags.push({ kind: 'note', text })
      tags.push({ kind: 'sample', text: m.when })
      if (m.alsoIn.length) tags.push({ kind: 'sample', text: `cited again by ${m.alsoIn.join(', ')}` })
      out.push({ ...p, tags })
    }
  }
  return out
}

/** The reports no set reaches, by suite: a benchmark the page has published that this chart cannot draw. Named rather
 *  than hidden (theseus-s5qwv), with the reason, so the pulldown answers "where are the rest?" instead of implying
 *  there are none. A run the manifest set aside states its own reason; the rest measured no rival arm to draw against. */
export function undrawn(
  sets: readonly TaskSet[],
  all: readonly { name: string; suite: string }[],
  skipped: readonly { report: string; why: string }[] = [],
): { suite: string; runs: string[]; why: string }[] {
  const drawn = new Set(sets.flatMap(setReports))
  const aside = new Map(skipped.map((s) => [s.report, s.why]))
  const bySuite = new Map<string, { runs: string[]; why: string }>()
  for (const r of all) {
    if (drawn.has(r.name)) continue
    const g = bySuite.get(r.suite) ?? bySuite.set(r.suite, { runs: [], why: '' }).get(r.suite)!
    g.runs.push(r.name)
    g.why ||= aside.get(r.name) ?? ''
  }
  return [...bySuite]
    .map(([suite, g]) => ({ suite, runs: g.runs, why: g.why || 'no rival arm measured on one task set' }))
    .sort((a, b) => b.runs.length - a.runs.length || a.suite.localeCompare(b.suite))
}
