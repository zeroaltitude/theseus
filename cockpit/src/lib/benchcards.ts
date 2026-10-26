// The browse surface's pure half (theseus-sam3s): one card per benchmark, newest first. A benchmark the manifest
// measures (a pass rate with an interval) gets its newest numbers; every other suite gets a card from its newest report
// and the README's index, with the words "no comparable headline metric yet" and never a number of its own. The page
// must not imply Theseus is measured on Terminal-Bench alone.
import { GITHUB_DOCS, type Run } from './bench.ts'
import { framingOfRow, type Framing } from './benchframing.ts'
import { groupTasks, LEAD, leadRow, type IndexRow, type LeadRule, type Manifest, type Series } from './benchindex.ts'

export const NO_METRIC = 'no comparable headline metric yet'

/** The omnibus review: the one undated page, pinned first (`/benchmarks/omnibus`). */
export const OMNIBUS = 'omnibus'

/** One arm's line on a card: the row that speaks for it, and a chip when it covers fewer tasks than its group's most. */
export interface ArmLine { arm: string; name: string; row: IndexRow; chip: string | null; theseus: boolean; framing: Framing }

export interface Card {
  id: string
  /** `pinned`: the omnibus; `measured`: from the manifest; `suite`: from reports alone. */
  kind: 'pinned' | 'measured' | 'suite'
  title: string
  tier: string | null
  /** The newest date this card knows of. */
  asOf: string
  /** Measured arms (null where no manifest row counts them). */
  arms: number | null
  /** The arms' lines, best pass rate first; empty for a suite with no manifest row. */
  lines: ArmLine[]
  /** The words in the metric's place for a card with no manifest row; null on a measured card. */
  noMetric: string | null
  /** A report's title, for a card made from reports. */
  headline: string | null
  /** The task set whose per-task table the card opens ("every test"), where the card's reports have one. */
  taskSet: string | null
  /** The report the card opens, and its GitHub address; null when the build does not hold that report. */
  report: string | null
  github: string | null
  /** A newer report of this suite that the manifest does not hold, said so rather than guessed. */
  newer: { report: string; date: string; title: string } | null
}

const BENCH_NAMES: Record<string, string> = {
  'terminal-bench': 'Terminal-Bench', 'terminal-bench@2.0': 'Terminal-Bench 2.0', harbor: 'Harbor',
  'gate-bench': 'Gate bench', 'memory-exam': 'Memory exam', retrieval: 'Retrieval', recall: 'Recall', async: 'Async', context: 'Context',
  h2h: 'Head to head', f10: 'The first ten minutes', 'swe-bench': 'SWE-bench',
}
const nameOf = (b: string) => BENCH_NAMES[b] ?? b

/** The suite a manifest benchmark name belongs to (`terminal-bench@2.0` → `terminal-bench`). */
const suiteOf = (benchmark: string) => benchmark.replace(/@.*$/, '')

export const githubUrl = (report: string, reports: readonly string[]): string | null => (reports.includes(report) ? `${GITHUB_DOCS}${report}.md` : null)
const heldOr = (report: string, reports: readonly string[]) => (reports.includes(report) ? report : null)

export const taskChip = (row: IndexRow, largest: number): string | null =>
  row.tasks !== null && largest > 0 && row.tasks < largest ? `${row.tasks} of ${largest} tasks` : null

/** The first of these reports (the card's lead first) that a task set reads. */
const setFor = (names: readonly string[], sets: readonly { id: string; report: string }[]): string | null => {
  for (const n of names) { const s = sets.find((x) => x.report === n); if (s) return s.id }
  return null
}

/** The cards, newest first with the omnibus pinned at the top. `list` is the runs newest first; `reports` the names the build holds. */
export function cardsOf(manifest: Manifest, list: readonly Run[], reports: readonly string[], rule: LeadRule = LEAD, sets: readonly { id: string; report: string }[] = []): Card[] {
  const out: Card[] = []
  if (reports.includes(OMNIBUS)) {
    const o = list.find((r) => r.name === OMNIBUS)
    out.push({
      id: OMNIBUS, kind: 'pinned', title: 'The omnibus review', tier: null, asOf: o?.date ?? '', arms: null, lines: [], noMetric: null,
      headline: 'what every run says, in one page: start here', taskSet: null, report: OMNIBUS, github: githubUrl(OMNIBUS, reports), newer: null,
    })
  }
  // The measured benchmarks: one card per benchmark and tier.
  const groups = new Map<string, Series[]>()
  for (const s of manifest.series) {
    const key = `${s.benchmark}|${s.tier ?? ''}`
    groups.set(key, [...(groups.get(key) ?? []), s])
  }
  const covered = new Set<string>()
  const measured: Card[] = []
  for (const [key, group] of groups) {
    const largest = groupTasks(group)
    const lines = group.map((s) => {
      const row = leadRow(s, rule, largest)
      return { arm: s.arm, name: row.harness.replace(/\s*\(this run\)$/, ''), row, chip: taskChip(row, largest), theseus: s.arm === 'theseus', framing: framingOfRow(row) }
    }).sort((a, b) => b.row.pass.value - a.row.pass.value)
    const first = group[0]
    const asOf = group.reduce((d, s) => (s.newest.date > d ? s.newest.date : d), '')
    covered.add(suiteOf(first.benchmark))
    const top = lines[0].row
    measured.push({
      id: key.replace('|', ':'), kind: 'measured', title: nameOf(first.benchmark), tier: first.tier, asOf, arms: group.length, lines,
      noMetric: null, headline: null, taskSet: setFor(lines.map((l) => l.row.report), sets), report: heldOr(top.report, reports), github: githubUrl(top.report, reports), newer: null,
    })
  }
  // A report of a measured suite that no manifest row cites, and is newer than the suite's cards: said, not guessed.
  const cited = new Set(manifest.rows.map((r) => r.report))
  for (const suite of covered) {
    const newest = list.find((r) => r.suite === suite && r.name !== OMNIBUS)
    const cards = measured.filter((c) => suiteOf(c.id.split(':')[0]) === suite)
    if (newest && cards.length && !cited.has(newest.name) && newest.date > cards.reduce((d, c) => (c.asOf > d ? c.asOf : d), '')) {
      cards.sort((a, b) => b.asOf.localeCompare(a.asOf))[0].newer = { report: newest.name, date: newest.date, title: newest.title }
    }
  }
  // Every other suite: its newest report, no number.
  const seen = new Set<string>()
  const suites: Card[] = []
  for (const r of list) {
    if (r.name === OMNIBUS || covered.has(r.suite) || seen.has(r.suite)) continue
    seen.add(r.suite)
    suites.push({
      id: r.suite, kind: 'suite', title: nameOf(r.suite), tier: null, asOf: r.date, arms: null, lines: [], noMetric: NO_METRIC,
      headline: r.title, taskSet: setFor([r.name], sets), report: r.name, github: githubUrl(r.name, reports), newer: null,
    })
  }
  return [...out, ...[...measured, ...suites].sort((a, b) => b.asOf.localeCompare(a.asOf) || a.title.localeCompare(b.title) || (a.tier ?? '').localeCompare(b.tier ?? ''))]
}
