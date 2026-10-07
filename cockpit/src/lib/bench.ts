// The Benchmarks tab's pure half (theseus-raf4): the published runs (`docs/benchmarks/`, which the cockpit's build
// embeds) read as the view needs them. The index of every run, a report's own parts, the per-trial tables of the Harbor
// runs folded into points (a harness arm in one run, on one task set), the properties a point is measured on with their
// better direction, and the Pareto frontier among the points of one task set.
//
// Only measured data: a point is its run's own trials, nothing interpolated or carried from another run. Where a
// condition that sets two points apart is in the data (trials that ended on an exception, trials partly on another
// model, a subset of the tasks, a small sample), it is computed here. Where it is only in a report's words (effort, the
// kinds of caps, the builds, an ablation), it is a note that carries the exact phrase of the report it rests on, and
// test/bench.test.ts holds every phrase to its report. Pure: `node --test` runs it as it is.
import { usd } from './figures.ts'

// ---------------------------------------------------------------- the files

/** A report's files, as the build embeds them: its name (`2026-10-04-terminal-bench-first-full-run`) and texts. */
export interface ReportFiles { name: string; md?: string; json?: string; csv?: string }

/** The suites a report's name can start with after its date (`docs/benchmarks/README.md`, "Its name"). */
export const SUITES = ['terminal-bench', 'swe-bench', 'harbor', 'gate-bench', 'memory-exam', 'retrieval', 'recall', 'async', 'context'] as const

/** A report's name from an embedded file's path: `…/2026-10-04-terminal-bench-first-full-run.csv` → its stem. */
export function reportName(path: string): string {
  const base = path.slice(path.lastIndexOf('/') + 1)
  return base.replace(/\.(md|json|csv)$/, '')
}

/** RFC 4180: commas, quoted fields with doubled quotes, CRLF or LF. The first row names the columns. */
export function parseCsv(text: string): Record<string, string>[] {
  const rows: string[][] = []
  let row: string[] = [], field = '', quoted = false
  for (let i = 0; i < text.length; i++) {
    const c = text[i]
    if (quoted) {
      if (c === '"') { if (text[i + 1] === '"') { field += '"'; i++ } else quoted = false }
      else field += c
    } else if (c === '"') quoted = true
    else if (c === ',') { row.push(field); field = '' }
    else if (c === '\n' || c === '\r') {
      if (c === '\r' && text[i + 1] === '\n') i++
      row.push(field); field = ''
      if (row.length > 1 || row[0] !== '') rows.push(row)
      row = []
    } else field += c
  }
  if (field !== '' || row.length) { row.push(field); rows.push(row) }
  const [head, ...body] = rows
  if (!head) return []
  return body.map((r) => Object.fromEntries(head.map((h, j) => [h, r[j] ?? ''])))
}

/** A cell as a number, or undefined when it is empty or not a number. */
export function num(s: string | undefined): number | undefined {
  if (s === undefined || s.trim() === '') return undefined
  const n = Number(s)
  return Number.isFinite(n) ? n : undefined
}

// ---------------------------------------------------------------- the index and a report's parts

/** A row of the README's "Every run, newest first". */
export interface IndexRow { name: string; date: string; suite: string; arms: string; headline: string; title: string }

/** Split a markdown table's line into its cells (a `|` inside backticks stays in its cell). */
export function tableCells(line: string): string[] {
  const cells: string[] = []
  let cell = '', code = false
  const s = line.trim().replace(/^\|/, '').replace(/\|$/, '')
  for (const c of s) {
    if (c === '`') code = !code
    if (c === '|' && !code) { cells.push(cell.trim()); cell = '' } else cell += c
  }
  cells.push(cell.trim())
  return cells
}

/** The README's index: one row a report, newest first, as the README lists them. */
export function readIndex(readme: string): IndexRow[] {
  const out: IndexRow[] = []
  for (const line of readme.split('\n')) {
    if (!/^\|\s*\d{4}-\d\d-\d\d\s*\|/.test(line)) continue
    const c = tableCells(line)
    if (c.length < 5) continue
    const link = /\[(.+)\]\(([^)]+)\.md\)\s*$/.exec(c[c.length - 1])
    if (!link) continue
    out.push({
      name: link[2], date: c[0], suite: c[1].replace(/`/g, ''), arms: c[2],
      headline: c.slice(3, -1).join(' | '), title: link[1],
    })
  }
  return out
}

/** A report's title, its first heading. */
export function reportTitle(md: string): string | undefined {
  return /^#\s+(.+)$/m.exec(md)?.[1].trim()
}

/** A report's sections (its `## ` headings, outside code), for the reader's contents. */
export function sections(md: string): string[] {
  let code = false
  const out: string[] = []
  for (const l of md.split('\n')) {
    if (l.startsWith('```')) code = !code
    else if (!code && l.startsWith('## ')) out.push(plain(l.slice(3)))
  }
  return out
}

/** "The answer first": the report's opening paragraph, without its bold lead. */
export function answerFirst(md: string): string | undefined {
  const m = /\*\*The answer first\.\*\*\s*([\s\S]*?)(?:\n\s*\n|$)/.exec(md)
  return m?.[1].replace(/\s+/g, ' ').trim()
}

/** The table at a glance (the report's first `| | |` table): each row's label and its text. */
export function atAGlance(md: string): { label: string; text: string }[] {
  const lines = md.split('\n')
  const start = lines.findIndex((l) => /^\|\s*\|\s*\|\s*$/.test(l))
  if (start < 0) return []
  const out: { label: string; text: string }[] = []
  for (const l of lines.slice(start + 2)) {
    if (!l.startsWith('|')) break
    const [label, ...rest] = tableCells(l)
    out.push({ label, text: rest.join(' | ') })
  }
  return out
}

/** Markdown's inline marks taken off a cell: code ticks, emphasis, links to their words. */
export function plain(md: string): string {
  return md.replace(/\[([^\]]*)\]\([^)]*\)/g, '$1').replace(/`([^`]*)`/g, '$1').replace(/\*\*?([^*]+)\*\*?/g, '$1').trim()
}

/** The model a report says its arms ran, in words: "Claude Sonnet 5.5 (`anthropic/…`) in every arm" → "Claude Sonnet 5.5". */
export function modelOf(md: string): string | undefined {
  const row = atAGlance(md).find((r) => r.label === 'Model')
  if (!row) return undefined
  return plain(row.text.replace(/\s*\([^)]*\)/g, '')).replace(/\s+(in every arm|throughout)$/, '').trim()
}

/** A run of the list: the index's row when it has one, else what the report says of itself. */
export interface Run { name: string; date: string; suite: string; title: string; arms?: string; headline?: string; indexed: boolean }

export function runs(names: readonly string[], index: readonly IndexRow[], titles: ReadonlyMap<string, string> = new Map()): Run[] {
  const byName = new Map(index.map((r) => [r.name, r]))
  const out: Run[] = names.filter((n) => n !== 'README').map((name) => {
    const r = byName.get(name)
    if (r) return { name, date: r.date, suite: r.suite, title: r.title, arms: r.arms, headline: r.headline, indexed: true }
    const date = name.slice(0, 10)
    const rest = name.slice(11)
    const suite = SUITES.find((s) => rest === s || rest.startsWith(`${s}-`)) ?? rest.split('-')[0]
    return { name, date, suite, title: titles.get(name) ?? name, indexed: false }
  })
  // Newest first; a day's reports in the index's own order, then by name.
  const order = new Map(index.map((r, i) => [r.name, i]))
  return out.sort((a, b) => b.date.localeCompare(a.date) || (order.get(a.name) ?? 1e9) - (order.get(b.name) ?? 1e9) || a.name.localeCompare(b.name))
}

// ---------------------------------------------------------------- the report's markdown, for the reader

/** A report's markdown made ready for the reader: each `<picture>` (GitHub's light and dark sources) becomes one image of
 *  its light source, whose dark twin the reader picks at night; raw HTML is not rendered otherwise. */
export function readerMarkdown(md: string): string {
  return md.replace(/<picture>[\s\S]*?<img\s+alt="([^"]*)"\s+src="([^"]+)"[^>]*>\s*<\/picture>/g,
    (_, alt: string, src: string) => `![${alt.replace(/[[\]]/g, '')}](${src})`)
}

/** Where a report's link goes: another report's reader, this run's data, a figure, a path in the repository, or the web. */
export type LinkTo =
  | { kind: 'report'; name: string; hash?: string }
  | { kind: 'data'; ext: 'json' | 'csv'; name: string }
  | { kind: 'repo'; path: string }
  | { kind: 'web'; href: string }
  | { kind: 'anchor'; hash: string }

export function linkTo(href: string): LinkTo {
  if (/^[a-z][a-z0-9+.-]*:/i.test(href)) return { kind: 'web', href }
  if (href.startsWith('#')) return { kind: 'anchor', hash: href.slice(1) }
  const [path, hash] = href.split('#')
  const local = /^(?:\.\/)?([^/]+)\.(md|json|csv)$/.exec(path)
  if (local) {
    if (local[2] === 'md') return local[1] === 'README' ? { kind: 'report', name: '', hash } : { kind: 'report', name: local[1], hash }
    return { kind: 'data', ext: local[2] as 'json' | 'csv', name: local[1] }
  }
  // A path out of docs/benchmarks/ (`../../bench/README.md`): the repository's, said as a path from its root.
  const parts: string[] = ['docs', 'benchmarks']
  for (const p of path.split('/')) { if (p === '..') parts.pop(); else if (p && p !== '.') parts.push(p) }
  return { kind: 'repo', path: parts.join('/') }
}

/** A figure's image path (`img/<report>/<figure>.svg`) → its report and name, and the twin for the mode. */
export function figureOf(src: string): { report: string; figure: string } | undefined {
  const m = /^(?:\.\/)?img\/([^/]+)\/([^/]+?)(?:-dark)?\.svg$/.exec(src)
  return m ? { report: m[1], figure: m[2] } : undefined
}

export function figurePath(report: string, figure: string, night: boolean): string {
  return `img/${report}/${figure}${night ? '-dark' : ''}.svg`
}

// ---------------------------------------------------------------- a figure's table

type Json = null | boolean | number | string | Json[] | { [k: string]: Json }
type Obj = { [k: string]: Json }

const isObj = (x: Json | undefined): x is Obj => !!x && typeof x === 'object' && !Array.isArray(x)

/** A value in a figure's format, at the record's own precision. */
export function figureValue(v: Json | undefined, format?: string): string {
  if (v === null || v === undefined) return '–'
  if (typeof v !== 'number') return typeof v === 'object' ? JSON.stringify(v) : String(v)
  const n = +v.toPrecision(6)
  switch (format) {
    case 'pct': return `${(v * 100).toFixed(1)}%`
    case 'usd': return usd(v, Math.abs(v) < 0.01 ? 5 : 4)
    case 'ms': return `${n} ms`
    case 's': return `${n} s`
    case 'min': return `${n} min`
    case 'mb': return `${n} MB`
    case 'x': return `${n}×`
    case 'int': return Math.round(v).toLocaleString('en-US')
    default: return `${n}`
  }
}

export interface FigureTable { columns: { label: string; num: boolean }[]; rows: string[][] }

const fmtOf = (spec: Obj, key: 'x' | 'y' | 'value'): string | undefined => {
  const a = spec[key]
  return isObj(a) && typeof a.format === 'string' ? a.format : undefined
}
const label = (o: Obj): string => String(o.label ?? o.title ?? o.arm ?? '')
const axisLabel = (spec: Obj, key: 'x' | 'y', fallback: string): string => {
  const a = spec[key]
  return isObj(a) && typeof a.label === 'string' ? a.label : fallback
}
const txt = (l: string) => ({ label: l, num: false })
const nm = (l: string) => ({ label: l, num: true })

/** A figure's spec (the report's JSON, `figures[]`) as the table of its numbers: the same values the figure draws, a
 *  row a mark, in the figure's own format. Every form the reports draw has its shape; a form not known yet gives each
 *  row's plain fields. */
export function figureTable(spec: Obj): FigureTable {
  const arr = (k: string): Obj[] => (Array.isArray(spec[k]) ? (spec[k] as Json[]).filter(isObj) : [])
  const fx = fmtOf(spec, 'x'), fy = fmtOf(spec, 'y')
  switch (spec.form) {
    case 'intervals': {
      const rows = arr('rows')
      const grouped = rows.some((r) => r.group !== undefined)
      return {
        columns: [...(grouped ? [txt('group')] : []), txt('row'), nm('value'), nm('low (95%)'), nm('high (95%)'), txt('note')],
        rows: rows.map((r) => [...(grouped ? [String(r.group ?? '')] : []), label(r), figureValue(r.value, fx), figureValue(r.lo, fx), figureValue(r.hi, fx), String(r.tip ?? '')]),
      }
    }
    case 'dumbbell':
      return {
        columns: [txt('row'), nm(String(spec.from_label ?? 'from')), nm(String(spec.to_label ?? 'to')), txt('note')],
        rows: arr('rows').map((r) => [label(r), figureValue(r.from, fx), figureValue(r.to, fx), String(r.tip ?? '')]),
      }
    case 'bars': {
      const cats = Array.isArray(spec.categories) ? spec.categories.map(String) : []
      const series = arr('series')
      const fv = fmtOf(spec, 'value')
      return {
        columns: [txt('category'), ...series.map((s) => nm(label(s)))],
        rows: cats.map((c, i) => [c, ...series.map((s) => figureValue(Array.isArray(s.values) ? s.values[i] : undefined, fv))]),
      }
    }
    case 'strip':
      return {
        columns: [txt('group'), nm(axisLabel(spec, 'x', 'value')), txt('note')],
        rows: arr('groups').flatMap((g) => (Array.isArray(g.values) ? g.values : []).map((v, i) =>
          [label(g), figureValue(v, fx), Array.isArray(g.tips) ? String(g.tips[i] ?? '') : ''])),
      }
    case 'scatter':
      return {
        columns: [txt('point'), nm(axisLabel(spec, 'x', 'x')), nm('x 95%'), nm(axisLabel(spec, 'y', 'y')), nm('y 95%'), txt('note')],
        rows: arr('points').map((p) => [label(p), figureValue(p.x, fx),
          p.xlo !== undefined ? `${figureValue(p.xlo, fx)} to ${figureValue(p.xhi, fx)}` : '–', figureValue(p.y, fy),
          p.ylo !== undefined ? `${figureValue(p.ylo, fy)} to ${figureValue(p.yhi, fy)}` : '–', String(p.tip ?? '')]),
      }
    case 'matrix': {
      const cols = arr('columns')
      const states = isObj(spec.state_labels) ? spec.state_labels : {}
      return {
        columns: [txt('row'), ...cols.map((c) => txt(label(c))), txt('note')],
        rows: arr('rows').map((r) => {
          const cells = String(r.cells ?? '')
          const tips = Array.isArray(r.tips) ? r.tips.map(String) : []
          return [label(r), ...cols.map((_, j) => tips[j] ?? String(states[cells[j]] ?? cells[j] ?? '')), String(r.note ?? '')]
        }),
      }
    }
    case 'lines':
      return {
        columns: [txt('series'), nm(axisLabel(spec, 'x', 'x')), nm(axisLabel(spec, 'y', 'y')), txt('note')],
        rows: arr('series').flatMap((s) => (Array.isArray(s.points) ? s.points : []).map((p, i) => {
          const [x, y] = Array.isArray(p) ? p : [undefined, undefined]
          return [label(s), figureValue(x, fx), figureValue(y, fy), Array.isArray(s.tips) ? String(s.tips[i] ?? '') : '']
        })),
      }
    case 'multiples':
      return {
        columns: [txt('panel'), txt('series'), nm(axisLabel(spec, 'x', 'x')), nm('value')],
        rows: arr('panels').flatMap((p) => {
          const py = isObj(p.y) && typeof p.y.format === 'string' ? p.y.format : undefined
          return (Array.isArray(p.series) ? p.series.filter(isObj) : []).flatMap((s) =>
            (Array.isArray(s.points) ? s.points : []).map((pt) => {
              const [x, y] = Array.isArray(pt) ? pt : [undefined, undefined]
              return [String(p.title ?? ''), label(s), figureValue(x, fx), figureValue(y, py)]
            }))
        }),
      }
    default: {
      const rows = arr('rows')
      const keys = [...new Set(rows.flatMap((r) => Object.keys(r).filter((k) => !isObj(r[k]))))]
      return { columns: keys.map(txt), rows: rows.map((r) => keys.map((k) => figureValue(r[k]))) }
    }
  }
}

// ---------------------------------------------------------------- harnesses, task sets, conditions

/** A harness arm, and its slot in the house palette (`docs/benchmarks/README.md`): Theseus 1, Claude Code 2, the
 *  paragraph 3; Pi has no slot (the eight are taken and slot 4 is held for OpenClaw), so it is drawn as "other". */
export interface Harness { key: string; name: string; slot: number | null }
export const HARNESSES: readonly Harness[] = [
  { key: 'theseus', name: 'Theseus', slot: 0 },
  { key: 'claude-code', name: 'Claude Code', slot: 1 },
  { key: 'theseus-batching', name: 'Theseus + paragraph', slot: 2 },
  { key: 'pi', name: 'Pi', slot: null },
]
export const harness = (key: string): Harness => HARNESSES.find((h) => h.key === key) ?? { key, name: key, slot: null }

/** A note from a report's words: what it says, and the exact phrase of the report it rests on (held by a test). */
export interface Note { text: string; report: string; quote: string }

const TB2 = '2026-10-04-terminal-bench-first-full-run'
const FIX = '2026-10-06-harbor-efficiency-checks'
const WORTH = '2026-10-03-harbor-worth-spike'
const ASYNC = '2026-10-06-async-smokes'

/** Each harness's effort at the time, as the reports say it. */
export const EFFORT: Record<string, Note> = {
  'theseus': { text: 'none sent (the API’s default for Sonnet 5.5 is high)', report: TB2, quote: 'Theseus sent no effort setting (the API\'s default for Sonnet 5.5 is high)' },
  'theseus-batching': { text: 'none sent (the API’s default for Sonnet 5.5 is high)', report: TB2, quote: 'Theseus sent no effort setting (the API\'s default for Sonnet 5.5 is high)' },
  'claude-code': { text: 'on each turn, its level not recorded (2.1.290, checked later, sends medium)', report: TB2, quote: '2.1.288\'s init events say per-turn effort was active' },
}

/** A task set: one report's per-trial table, the trials of it that ran the same tasks, and how its rows read. */
export interface TaskSet {
  id: string; label: string; short: string; report: string
  /** What the task set is, in a line. */
  about: string
  /** The trials of the table in this set. */
  keep: (r: Record<string, string>) => boolean
  /** A trial's harness key and its run (a point is a harness in a run). */
  arm: (r: Record<string, string>, arms: readonly string[]) => string
  run: (r: Record<string, string>) => { id: string; label: string }
  /** A run with a report of its own (the spike's, the full run's), by run id; the rest open the set's report. */
  runReport?: Record<string, string>
  /** The set's conditions that differ between its harnesses, from the report's words. */
  conditions: Note[]
  /** A harness's build or version, by run, where the report says it; '*' for every run. */
  versions: Record<string, Record<string, Note>>
  /** A point's own conditions from the report's words, by harness and run ('*' for every run). */
  notes: Record<string, Record<string, Note[]>>
}

const ABLATION: Note = { text: 'a prompt ablation, never shipped', report: TB2, quote: 'B is reported here once, as an ablation, and was never shipped.' }
const EFFORT_DIFFERS: Note = { text: 'Effort: Theseus sent none (the API’s default is high); Claude Code ran per-turn effort at a level not recorded.', report: TB2, quote: 'And the arms may not have thought at the same effort' }

export const TASK_SETS: readonly TaskSet[] = [
  {
    id: 'tb2', label: 'Terminal-Bench 2.0, all 89 tasks × 2 attempts', short: 'Terminal-Bench 2.0 · 89 tasks', report: TB2,
    about: 'The first full run: every task, two attempts an arm, Claude Sonnet 5.5 in every arm, the same $2.00 and 200 model calls a trial.',
    keep: () => true,
    arm: (r, arms) => arms['ABC'.indexOf(r.arm)] ?? r.arm,
    run: () => ({ id: 'b5', label: 'the first full run' }),
    conditions: [
      EFFORT_DIFFERS,
      { text: 'The $2.00 caps are of different kinds: Theseus reserves a call’s price before it runs; Claude Code stops after the call that passes it.', report: TB2, quote: 'The $2.00 caps are not the same kind.' },
      { text: 'Timeouts are not symmetric: a timed-out Claude Code keeps working in its container while the tests run.', report: TB2, quote: 'Claude Code\'s agent keeps running in its container while Harbor runs the tests' },
      { text: 'Theseus’s output was capped at 32,000 tokens a call; the model and Claude Code allow 128,000.', report: TB2, quote: 'Claude Code and the model\'s own limit allow 128,000.' },
    ],
    versions: {
      'theseus': { '*': { text: 'built at 079f1db', report: TB2, quote: 'Theseus built at `079f1db`' } },
      'theseus-batching': { '*': { text: 'built at 079f1db', report: TB2, quote: 'Theseus built at `079f1db`' } },
      'claude-code': { '*': { text: '2.1.288', report: TB2, quote: 'Claude Code 2.1.288' } },
    },
    notes: { 'theseus-batching': { '*': [ABLATION] } },
  },
  {
    id: 'fixgit', label: 'Terminal-Bench 2.0’s fix-git, one task over seven runs', short: 'fix-git · seven runs', report: FIX,
    about: 'One easy task, run again and again over four days: a point a run and harness. The only task set with each harness’s own memory and CPU, sampled inside the task’s container.',
    keep: (r) => r.task === 'fix-git',
    arm: (r) => r.arm,
    run: (r) => ({ id: r.run, label: r.run_label || r.run }),
    runReport: { worth: WORTH, 'b5-smoke': TB2, b5: TB2 },
    conditions: [
      EFFORT_DIFFERS,
      { text: 'Caps differed by run: $2.00 in the spike and the first full run, $0.40 in the plumbing’s checks, $1.00 in the reviews; Claude Code ran at Harbor’s defaults in the spike.', report: FIX, quote: 'a trial\'s spend cap was $2.00 in the spike and the first full run, $0.40' },
      { text: 'Theseus’s builds differed by run: the spike’s, the plumbing lane’s, then 079f1db.', report: FIX, quote: 'Theseus\'s binaries: the spike\'s build, then the plumbing lane\'s, then `079f1db` in every sampled check' },
      { text: 'Claude Code’s adapter changed: Harbor’s own, then the measured one.', report: FIX, quote: 'Claude Code 2.1.288 (`claude-code`, then `claude_code_agent:MeasuredClaudeCode`)' },
    ],
    versions: {
      'theseus': {
        'worth': { text: 'the spike’s build (dc3387f with 60dfb45)', report: WORTH, quote: 'Theseus at `dc3387f` with the spike\'s secrets change (`60dfb45`)' },
        'plumbing': { text: 'the plumbing lane’s build', report: FIX, quote: 'then the plumbing lane\'s' },
        'b5': { text: 'built at 079f1db', report: TB2, quote: 'Theseus built at `079f1db`' },
        'r11': { text: '079f1db (the full run’s static build)', report: FIX, quote: 'every sampled check ran the first full run\'s static build (`079f1db`)' },
        'r16': { text: '079f1db (the full run’s static build)', report: FIX, quote: 'every sampled check ran the first full run\'s static build (`079f1db`)' },
      },
      'theseus-batching': {
        'worth': { text: 'the spike’s build (dc3387f with 60dfb45)', report: WORTH, quote: 'Theseus at `dc3387f` with the spike\'s secrets change (`60dfb45`)' },
        'b5': { text: 'built at 079f1db', report: TB2, quote: 'Theseus built at `079f1db`' },
      },
    },
    notes: {
      'theseus': { 'plumbing': [{ text: 'two of its three trials ran with the agent’s timeout cut to 10.8 s on purpose', report: FIX, quote: 'twice with its agent timeout cut to 10.8 s' }] },
      'theseus-batching': { '*': [ABLATION] },
      'pi': { '*': [{ text: 'its caps were recorded, not enforced', report: FIX, quote: 'Pi\'s caps were recorded, not enforced.' }] },
    },
  },
  {
    id: 'worth', label: 'The worth spike’s four tasks', short: 'the worth spike · 4 tasks', report: WORTH,
    about: 'Three Terminal-Bench 2.0 tasks and one SWE-bench Verified task, one attempt each: the first day Theseus ran public benchmarks.',
    keep: () => true,
    arm: (r) => r.arm,
    run: () => ({ id: 'worth', label: 'the worth spike' }),
    conditions: [
      EFFORT_DIFFERS,
      { text: 'Caps: Theseus ran with $2.00 and 200 loops; Claude Code with Harbor’s defaults, no caps.', report: WORTH, quote: 'Theseus ran with a $2.00 and 200-loop cap; Claude Code with Harbor\'s defaults, no caps.' },
    ],
    versions: {
      'theseus': { '*': { text: 'dc3387f with 60dfb45', report: WORTH, quote: 'Theseus at `dc3387f` with the spike\'s secrets change (`60dfb45`)' } },
      'theseus-batching': { '*': { text: 'dc3387f with 60dfb45', report: WORTH, quote: 'Theseus at `dc3387f` with the spike\'s secrets change (`60dfb45`)' } },
      'claude-code': { '*': { text: '2.1.288', report: WORTH, quote: 'Claude Code 2.1.288, Harbor\'s own adapter' } },
    },
    notes: { 'theseus-batching': { '*': [ABLATION] } },
  },
  {
    id: 'async', label: 'The async bench’s interrupt family (smokes)', short: 'async · interrupt', report: ASYNC,
    about: 'A second message arrives while the agent works: how soon it acts on it, and what waiting costs. Smokes, not a full run.',
    keep: (r) => r.family === 'interrupt',
    arm: (r) => r.arm,
    run: () => ({ id: 'async', label: 'the async smokes' }),
    conditions: [
      { text: 'Effort: Theseus sends none (the API’s default is high), while Claude Code runs at medium.', report: ASYNC, quote: 'The effort setting (Theseus sends none, the API\'s default being high, while Claude Code runs at medium' },
      { text: 'Timeouts are not symmetric: a timed-out Claude Code keeps working while the tests run.', report: ASYNC, quote: 'a timed-out Claude Code keeps working while the tests run' },
    ],
    versions: {
      'theseus': { '*': { text: '079f1db', report: ASYNC, quote: 'Theseus\'s binaries `079f1db` throughout' } },
      'claude-code': { '*': { text: '2.1.288', report: ASYNC, quote: 'Claude Code 2.1.288 (`async_agents:ClaudeCodeAsync`' } },
    },
    notes: {},
  },
]

// ---------------------------------------------------------------- properties

export type Better = 'higher' | 'lower'

/** A property a point is measured on: its words, unit, better direction, and how its trials fold into one value. */
export interface Property {
  id: string; label: string; short: string; better: Better
  /** How a value prints; dollars say so, for an axis's ticks. */
  format: (v: number) => string
  unit?: 'usd'
  /** The trials' values folded: a rate (Wilson), a mean, a median, or dollars over solved trials. */
  fold: 'rate' | 'mean' | 'median' | 'per-solved'
  /** A trial's value, from its table's row. */
  read: (r: Record<string, string>) => number | undefined
  /** What the property is, for its tip and the table's head. */
  about: string
}

const sumOf = (...xs: (number | undefined)[]) => (xs.some((x) => x === undefined) ? undefined : xs.reduce<number>((a, x) => a + (x as number), 0))
const secs = (v: number) => (v < 10 ? `${+v.toFixed(2)} s` : v < 120 ? `${+v.toFixed(1)} s` : `${Math.round(v)} s`)
const count = (v: number) => (v < 100 ? `${+v.toFixed(2)}` : Math.round(v).toLocaleString('en-US'))
const toks = (v: number) => (v >= 1e6 ? `${+(v / 1e6).toFixed(2)}M` : v >= 1e3 ? `${+(v / 1e3).toFixed(1)}k` : `${Math.round(v)}`)

/** Whether a trial solved its task: the full run's `solved`, else a reward of 1. */
export function solved(r: Record<string, string>): boolean {
  if (r.solved !== undefined && r.solved !== '') return r.solved === '1'
  return (num(r.reward) ?? 0) >= 1
}

export const PROPERTIES: readonly Property[] = [
  { id: 'pass', label: 'pass rate (trials)', short: 'pass rate', better: 'higher', fold: 'rate', format: (v) => `${(v * 100).toFixed(1)}%`,
    read: (r) => (solved(r) ? 1 : 0), about: 'The share of trials that solved their task, with its Wilson 95% interval.' },
  { id: 'usd', label: 'dollars a trial (mean)', short: '$ a trial', better: 'lower', fold: 'mean', unit: 'usd', format: (v) => usd(v, v < 0.1 ? 4 : 3),
    read: (r) => num(r.dollars), about: 'The model’s bill for a trial, the mean over the priced trials.' },
  { id: 'usd-solved', label: 'dollars a solved trial', short: '$ a solve', better: 'lower', fold: 'per-solved', unit: 'usd', format: (v) => usd(v, v < 0.1 ? 4 : 3),
    read: (r) => num(r.dollars), about: 'Every priced trial’s dollars over the trials solved: what a solve costs.' },
  { id: 'tokens', label: 'tokens a trial (every kind, mean)', short: 'tokens a trial', better: 'lower', fold: 'mean', format: toks,
    read: (r) => sumOf(num(r.input), num(r.cache_read), num(r.cache_write), num(r.output)), about: 'Input, cache reads, cache writes and output, summed for a trial; the mean.' },
  { id: 'output', label: 'output tokens a trial (mean)', short: 'output tokens', better: 'lower', fold: 'mean', format: toks,
    read: (r) => num(r.output), about: 'The model’s output tokens a trial, the mean.' },
  { id: 'calls', label: 'model calls a trial (mean)', short: 'model calls', better: 'lower', fold: 'mean', format: count,
    read: (r) => num(r.model_calls), about: 'Round trips to the model a trial, the mean.' },
  { id: 'agent-s', label: 'agent time a trial (median)', short: 'wall time', better: 'lower', fold: 'median', format: secs,
    read: (r) => num(r.agent_s), about: 'The agent phase’s wall time, from its start to its end, the median.' },
  { id: 'trial-s', label: 'trial time, setup included (median)', short: 'trial time', better: 'lower', fold: 'median', format: secs,
    read: (r) => num(r.trial_s), about: 'The whole trial’s wall time, the harness’s install and the tests included, the median.' },
  { id: 'setup-s', label: 'setup in the task’s container (median)', short: 'setup (start)', better: 'lower', fold: 'median', format: secs,
    read: (r) => num(r.setup_s), about: 'Installing and starting the harness in the task’s container, the median: the start time measured across harnesses.' },
  { id: 'rss', label: 'the harness’s peak memory (MiB, median)', short: 'harness memory', better: 'lower', fold: 'median', format: (v) => `${+v.toFixed(1)} MiB`,
    read: (r) => num(r.harness_rss_mib), about: 'The harness’s own processes at their peak resident memory, sampled inside the container, apart from the commands they ran.' },
  { id: 'cpu-tool', label: 'the harness’s CPU a tool call (ms, median)', short: 'CPU a tool call', better: 'lower', fold: 'median', format: (v) => `${+v.toFixed(1)} ms`,
    read: (r) => num(r.harness_ms_per_tool_call), about: 'The harness’s own CPU time over its tool calls, apart from the commands they ran.' },
  { id: 'cpu', label: 'the harness’s CPU a trial (s, median)', short: 'harness CPU', better: 'lower', fold: 'median', format: (v) => `${+v.toFixed(2)} s`,
    read: (r) => num(r.harness_cpu_s), about: 'The harness’s own CPU time in a trial, apart from the commands it ran.' },
  { id: 'container-mem', label: 'the container’s peak memory (MiB, median)', short: 'container memory', better: 'lower', fold: 'median', format: (v) => `${Math.round(v)} MiB`,
    read: (r) => num(r.container_mem_peak_mib), about: 'The task container’s cgroup at its peak: the harness, its work and the task.' },
  { id: 'respond-s', label: 'time to act on a mid-task message (median)', short: 'turn latency', better: 'lower', fold: 'median', format: secs,
    read: (r) => num(r.responsiveness_s), about: 'From a message that arrives while the agent works to its answer in the ledger.' },
  { id: 'over-ideal', label: 'wall time over the ideal (median)', short: 'over the ideal', better: 'lower', fold: 'median', format: (v) => `${+v.toFixed(2)}×`,
    read: (r) => num(r.over_ideal), about: 'The trial’s wall time over the family’s ideal, the oracle’s plan.' },
  { id: 'wait-tax', label: 'model calls spent waiting (mean)', short: 'wait tax', better: 'lower', fold: 'mean', format: count,
    read: (r) => num(r.wait_tax_calls), about: 'Model calls made only to wait on running work.' },
]
export const property = (id: string): Property | undefined => PROPERTIES.find((p) => p.id === id)

// ---------------------------------------------------------------- points

/** A property's value at a point: the folded value, its trials, and its spread; a rate has its Wilson interval at
 *  once, a mean or a median its trials' values for `interval()`. */
export interface Value { v: number; n: number; lo?: number; hi?: number; min: number; max: number; xs?: number[] }

export interface Tag { kind: 'sample' | 'exception' | 'other-model' | 'subset' | 'note'; text: string; note?: Note }

/** The tags that mark a point as run under conditions its peers did not share (drawn as a diamond). A small sample and
 *  trials that failed on an exception are part of what was measured: said in the tip and the table, not marked. */
export const UNEQUAL: readonly Tag['kind'][] = ['other-model', 'subset', 'note']
export const unequal = (p: Point): boolean => p.tags.some((t) => UNEQUAL.includes(t.kind))

export interface Point {
  id: string; set: string; harness: string; run: string; runLabel: string; report: string
  /** The first trial's start where the table has one, else the report's date. */
  date: string
  model?: string
  /** The harness's build or version: the report's words (with their phrase), or the table's own cell. */
  version?: { text: string; note?: Note }
  effort?: Note
  /** Trials, tasks. */
  n: number; tasks: number
  values: Record<string, Value>
  /** What sets this point apart (not the set's conditions, said once for all its points). */
  tags: Tag[]
}

/** The Wilson score interval at 95% for k of n. */
export function wilson(k: number, n: number, z = 1.959964): { lo: number; hi: number } {
  if (n === 0) return { lo: 0, hi: 1 }
  const p = k / n, z2 = z * z
  const den = 1 + z2 / n
  const mid = (p + z2 / (2 * n)) / den
  const half = (z * Math.sqrt((p * (1 - p)) / n + z2 / (4 * n * n))) / den
  return { lo: Math.max(0, mid - half), hi: Math.min(1, mid + half) }
}

export function median(xs: readonly number[]): number {
  const s = [...xs].sort((a, b) => a - b)
  const m = s.length >> 1
  return s.length % 2 ? s[m] : (s[m - 1] + s[m]) / 2
}

/** A property's trials folded into its value at a point, or undefined when no trial measured it. */
export function fold(p: Property, trials: readonly Record<string, string>[]): Value | undefined {
  const xs = trials.map(p.read).filter((x): x is number => x !== undefined)
  if (!xs.length) return undefined
  const min = Math.min(...xs), max = Math.max(...xs)
  switch (p.fold) {
    case 'rate': {
      const k = xs.filter((x) => x === 1).length
      return { v: k / xs.length, n: xs.length, ...wilson(k, xs.length), min, max }
    }
    case 'mean': return { v: xs.reduce((a, x) => a + x, 0) / xs.length, n: xs.length, min, max, xs }
    case 'median': return { v: median(xs), n: xs.length, min, max, xs }
    case 'per-solved': {
      const priced = trials.filter((r) => p.read(r) !== undefined)
      const k = priced.filter(solved).length
      return k ? { v: xs.reduce((a, x) => a + x, 0) / k, n: xs.length, min, max } : undefined
    }
  }
}

/** A seeded generator (mulberry32): the same resamples on every page and in every test. */
function seeded(seed: number): () => number {
  let a = seed >>> 0
  return () => {
    a = (a + 0x6d2b79f5) >>> 0
    let t = a
    t = Math.imul(t ^ (t >>> 15), t | 1)
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61)
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296
  }
}

/** A sample's median found in place in linear time (Hoare's selection), with no allocation: a bootstrap of the median
 *  costs about what the mean's does, where sorting each resample cost five times as much. */
export function selectMedian(a: Float64Array): number {
  const n = a.length, k = n >> 1
  const select = (k: number) => {
    let lo = 0, hi = n - 1
    while (lo < hi) {
      const pivot = a[(lo + hi) >> 1]
      let i = lo, j = hi
      while (i <= j) {
        while (a[i] < pivot) i++
        while (a[j] > pivot) j--
        if (i <= j) { const t = a[i]; a[i] = a[j]; a[j] = t; i++; j-- }
      }
      if (k <= j) hi = j
      else if (k >= i) lo = i
      else break
    }
    return a[k]
  }
  const upper = select(k)
  if (n % 2) return upper
  // The lower middle is the largest value left of k, which the selection left on that side.
  let lower = -Infinity
  for (let i = 0; i < k; i++) if (a[i] > lower) lower = a[i]
  return (lower + upper) / 2
}

/** Resamples for a bootstrap here: the reports use 10,000 (`bench/report/stats.py`); a page draws a few points at
 *  once, and 2,000 holds a 95% interval's ends to well under a tenth of its width. */
export const RESAMPLES = 2000

const intervals = new WeakMap<Value, { lo: number; hi: number; kind: Spread }>()
export type Spread = 'wilson' | 'bootstrap' | 'range'

/** A value's 95% interval, by the reports' rules (`docs/benchmarks/README.md`, "Its statistics"): a rate's Wilson
 *  interval; a mean's (or a median's) seeded bootstrap; its range under ten trials. Worked out once a value. */
export function interval(p: Property, v: Value): { lo: number; hi: number; kind: Spread } {
  if (p.fold === 'rate') return { lo: v.lo!, hi: v.hi!, kind: 'wilson' }
  const known = intervals.get(v)
  if (known) return known
  let out: { lo: number; hi: number; kind: Spread }
  const xs = v.xs
  if (!xs || xs.length < SMALL || p.fold === 'per-solved') out = { lo: v.min, hi: v.max, kind: 'range' }
  else {
    const rnd = seeded(20261007), n = xs.length, stats = new Float64Array(RESAMPLES), buf = new Float64Array(n)
    for (let b = 0; b < RESAMPLES; b++) {
      let sum = 0
      for (let i = 0; i < n; i++) { const x = xs[(rnd() * n) | 0]; buf[i] = x; sum += x }
      stats[b] = p.fold === 'mean' ? sum / n : selectMedian(buf)
    }
    stats.sort()
    out = { lo: stats[Math.floor(0.025 * (RESAMPLES - 1))], hi: stats[Math.ceil(0.975 * (RESAMPLES - 1))], kind: 'bootstrap' }
  }
  intervals.set(v, out)
  return out
}

/** The note for a harness in a run from a table keyed by harness then run ('*' for every run). */
function pick<T>(byHarness: Record<string, Record<string, T>>, h: string, run: string): T | undefined {
  const m = byHarness[h]
  return m?.[run] ?? m?.['*']
}

/** A small sample: under ten trials a point is a size, not a ranking. */
export const SMALL = 10

/** A task set's points: its table's trials grouped by harness and run, each property folded, each point's tags. */
export function points(set: TaskSet, csv: string, json: string | undefined, md: string | undefined, date: string): Point[] {
  const arms = json ? ((JSON.parse(json) as { arms?: { key: string }[] }).arms ?? []).map((a) => a.key) : []
  const rows = parseCsv(csv).filter(set.keep)
  const model = md ? modelOf(md) : undefined
  const allTasks = new Set(rows.map((r) => r.task).filter(Boolean))
  const groups = new Map<string, { h: string; run: { id: string; label: string }; trials: Record<string, string>[] }>()
  for (const r of rows) {
    const h = set.arm(r, arms), run = set.run(r)
    const id = `${set.id}:${run.id}:${h}`
    const g = groups.get(id) ?? groups.set(id, { h, run, trials: [] }).get(id)!
    g.trials.push(r)
  }
  const out: Point[] = []
  for (const [id, g] of groups) {
    const values: Record<string, Value> = {}
    for (const p of PROPERTIES) { const v = fold(p, g.trials); if (v) values[p.id] = v }
    const tasks = new Set(g.trials.map((r) => r.task).filter(Boolean))
    const tags: Tag[] = []
    if (g.trials.length < SMALL) tags.push({ kind: 'sample', text: `a small sample: ${g.trials.length} trial${g.trials.length === 1 ? '' : 's'}` })
    const ex = g.trials.filter((r) => (r.harbor_exception ?? r.exception ?? '') !== '')
    if (ex.length) {
      const kinds = [...new Set(ex.map((r) => r.harbor_exception || r.exception))].join(', ')
      tags.push({ kind: 'exception', text: `${ex.length} of ${g.trials.length} trials ended on an exception (${kinds})` })
    }
    const other = g.trials.filter((r) => (num(r.other_model_dollars) ?? 0) > 0)
    if (other.length) tags.push({ kind: 'other-model', text: `${other.length} trials ran partly on another model (a fallback)` })
    if (allTasks.size > 1 && tasks.size < allTasks.size) tags.push({ kind: 'subset', text: `ran ${tasks.size} of the set’s ${allTasks.size} tasks` })
    for (const note of pick(set.notes, g.h, g.run.id) ?? []) tags.push({ kind: 'note', text: note.text, note })
    const versionNote = pick(set.versions, g.h, g.run.id)
    const csvVersion = g.trials.find((r) => r.version)?.version
    // A table's version cell names a release (Claude Code's, Pi's); Theseus's reads `theseus 0.0.1` in every build, so
    // its build comes from the report's words or is not stated.
    const version = versionNote ? { text: versionNote.text, note: versionNote }
      : csvVersion && !/^theseus /.test(csvVersion) ? { text: csvVersion } : undefined
    const started = g.trials.map((r) => r.started).filter(Boolean).sort()[0]
    out.push({
      id, set: set.id, harness: g.h, run: g.run.id, runLabel: g.run.label, report: set.runReport?.[g.run.id] ?? set.report,
      date: started ?? date, model, version, effort: EFFORT[g.h],
      n: g.trials.length, tasks: tasks.size, values, tags,
    })
  }
  // Harness by harness in the palette's order, a harness's runs by date.
  const rank = (h: string) => { const i = HARNESSES.findIndex((x) => x.key === h); return i < 0 ? HARNESSES.length : i }
  return out.sort((a, b) => rank(a.harness) - rank(b.harness) || a.date.localeCompare(b.date))
}

/** The properties a set's points can be drawn on: those at least two points measured. The rest, with who measured them. */
export function axes(ps: readonly Point[]): { usable: Property[]; one: { p: Property; who: string[] }[] } {
  const usable: Property[] = [], one: { p: Property; who: string[] }[] = []
  for (const p of PROPERTIES) {
    const who = ps.filter((x) => x.values[p.id])
    if (who.length >= 2) usable.push(p)
    else if (who.length === 1) one.push({ p, who: [harness(who[0].harness).name] })
  }
  return { usable, one }
}

// ---------------------------------------------------------------- the frontier

export interface Placed {
  point: Point; x: number; y: number; frontier: boolean
  /** The points that dominate it, the nearest first (by the better direction's sum, each axis in its own scale). */
  dominators: Point[]
  dominatedBy?: Point
}

/** The points with both properties, each on the frontier or dominated (and by which points). A point dominates another
 *  when it is at least as good on both axes and better on one, each in its own better direction. */
export function frontier(ps: readonly Point[], px: Property, py: Property): Placed[] {
  const sx = px.better === 'higher' ? 1 : -1, sy = py.better === 'higher' ? 1 : -1
  const placed = ps.filter((p) => p.values[px.id] && p.values[py.id])
    .map((point) => ({ point, x: point.values[px.id].v, y: point.values[py.id].v }))
  return placed.map((a) => {
    const by: { p: Point; d: number }[] = []
    for (const b of placed) {
      if (b === a) continue
      const gx = sx * (b.x - a.x), gy = sy * (b.y - a.y)
      if (gx >= 0 && gy >= 0 && (gx > 0 || gy > 0)) by.push({ p: b.point, d: Math.hypot(gx / (Math.abs(a.x) || 1), gy / (Math.abs(a.y) || 1)) })
    }
    const dominators = by.sort((u, v) => u.d - v.d).map((u) => u.p)
    return { ...a, frontier: !dominators.length, dominators, dominatedBy: dominators[0] }
  })
}

/** The frontier's line: its points from the left, a step between each two that keeps to the better side, so the
 *  region under the line is what the frontier's points beat. */
export function staircase(ps: readonly Placed[], px: Property, py: Property): [number, number][] {
  const f = ps.filter((p) => p.frontier).sort((a, b) => a.x - b.x || (py.better === 'higher' ? b.y - a.y : a.y - b.y))
  const out: [number, number][] = []
  // Left to right: where lower x is better, the line runs across at the last point's y to the next point's x, then to
  // it; where higher x is better, it moves to the next point's y first, then across.
  f.forEach((p, i) => {
    if (i > 0) {
      const prev = f[i - 1]
      out.push(px.better === 'lower' ? [p.x, prev.y] : [prev.x, p.y])
    }
    out.push([p.x, p.y])
  })
  // Two points at one place (a tie) and a step of no length draw nothing: one vertex each.
  return out.filter((v, i) => i === 0 || v[0] !== out[i - 1][0] || v[1] !== out[i - 1][1])
}
