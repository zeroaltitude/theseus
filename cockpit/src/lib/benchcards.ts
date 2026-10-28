// The browse surface's pure half (theseus-sam3s): one card per benchmark, newest first. A benchmark the manifest
// measures (a pass rate with an interval) gets its newest numbers; every other suite gets a card from its newest report
// and the README's index, and never a number of its own. The page must not imply Theseus is measured on
// Terminal-Bench alone.
//
// The owner's three card rules, 2026-10-27 (theseus-s5qwv):
//   - a card prints a pass rate only where the run supports one (`headlineWhy`); otherwise it prints the run's own
//     one-line finding, and keeps every arm it measured with its numbers underneath. No data lost.
//   - a suite with no manifest row says what its run found, not "no comparable headline metric yet"; those words are
//     the last resort, for a report the README states no headline for.
//   - the earlier, untiered run of a benchmark whose later runs are tiered is labelled as history and sinks below
//     the tiered cards, rather than standing beside them as a rival.
import { GITHUB_DOCS, plain, type Run } from './bench.ts'
import { framingOfRow, type Framing } from './benchframing.ts'
import { groupTasks, LEAD, leadRow, type IndexRow, type LeadRule, type Manifest, type Series } from './benchindex.ts'

export const NO_METRIC = 'no comparable headline metric yet'

/** The omnibus review: the one undated page, pinned first (`/benchmarks/omnibus`). */
export const OMNIBUS = 'omnibus'

/** The words on the earlier, untiered run of a benchmark whose later runs are tiered. */
export const LEGACY_WORDS = 'the earlier run, before the tiers'

// ---------------------------------------------------------------- when a measurement speaks as a headline

/** A measurement speaks as a card's headline only when it covers more than one task and its interval is narrower
 *  than half the scale. Harbor's "100.0% [34.2%, 100.0%], 2 trials, 1 task" is a smoke presented as a result, and a
 *  card must not print it as one. The run's own one-line finding stands in its place, and every arm it measured
 *  stays on the card with its numbers: the rule moves the emphasis, it loses no data. */
export const HEADLINE_MIN_TASKS = 2
export const HEADLINE_MIN_TRIALS = 10
export const HEADLINE_MAX_WIDTH = 0.5

/** Why a row does not speak as a headline, in the card's own words; null where it does.
 *  The three bars are asked in the order a reader would: how much was run, then how wide the answer is. The count
 *  is asked on its own because a Wilson interval narrows towards 0 and 1, so width alone is most forgiving exactly
 *  where a small sample's extremes live: 8 of 8 is [67.6%, 100.0%], inside half the scale, and still a smoke. */
export function headlineWhy(row: IndexRow): string | null {
  const n = (c: number, w: string) => `${c} ${w}${c === 1 ? '' : 's'}`
  if (row.tasks !== null && row.tasks < HEADLINE_MIN_TASKS) {
    return `${n(row.trials, 'trial')} on ${n(row.tasks, 'task')}: a smoke, not a result`
  }
  if (row.pass.n < HEADLINE_MIN_TRIALS) return `${n(row.pass.n, 'trial')} behind it: too few to lead with`
  const width = row.pass.hi - row.pass.lo
  if (width > HEADLINE_MAX_WIDTH) return `its interval spans ${(width * 100).toFixed(1)} points: too wide to lead with`
  return null
}

/** The README's headline as a card prints it: `plain`'s marks taken off and its lines joined, words untouched.
 *  It is a short paragraph, not a line, which is why the card clamps it and links the report beside it. */
export const plainFinding = (headline: string): string => plain(headline).replace(/\s+/g, ' ').trim()

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
  /** Why the card's numbers do not speak as a headline (`headlineWhy`), or null where they do. */
  weak: string | null
  /** The run's own one-line finding, the README's headline for the report this card opens. */
  finding: string | null
  /** The earlier, untiered run of a benchmark whose later runs are tiered: history, not a rival. */
  legacy: boolean
  /** The last-resort words in the metric's place: only where there is no finding and no headline to print. */
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
  'bughunt-rs': 'Bug Hunt RS',
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
      weak: null, finding: null, legacy: false,
      headline: 'what every run says, in one page: start here', taskSet: null, report: OMNIBUS, github: githubUrl(OMNIBUS, reports), newer: null,
    })
  }
  // The README's one-line finding for a report, where it states one.
  const findings = new Map(list.filter((r) => r.headline).map((r) => [r.name, plainFinding(r.headline!)]))
  // The measured benchmarks: one card per benchmark and tier.
  const groups = new Map<string, Series[]>()
  for (const s of manifest.series) {
    const key = `${s.benchmark}|${s.tier ?? ''}`
    groups.set(key, [...(groups.get(key) ?? []), s])
  }
  // A benchmark whose later runs are tiered: its untiered group is the run from before the tiers -- which is a
  // claim about time, so it is asked of time. The newest tiered measurement of each benchmark:
  const tieredAfter = new Map<string, string>()
  for (const s of manifest.series) {
    if (!s.tier) continue
    const d = tieredAfter.get(s.benchmark)
    if (!d || s.newest.date > d) tieredAfter.set(s.benchmark, s.newest.date)
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
    // The card leads with the arm on top; it is that measurement the headline rule judges.
    const weak = headlineWhy(top)
    // Its finding is its own newest report's -- the one that set `asOf` -- never the report of whichever rival
    // happens to hold the biggest number, which on the high tier is another harness's page entirely.
    const own = group.reduce((r, s) => (s.newest.date > r.date ? s.newest : r), group[0].newest)
    measured.push({
      id: key.replace('|', ':'), kind: 'measured', title: nameOf(first.benchmark), tier: first.tier, asOf, arms: group.length, lines,
      weak, finding: findings.get(own.report) ?? null, legacy: first.tier === null && asOf < (tieredAfter.get(first.benchmark) ?? ''),
      noMetric: null, headline: null, taskSet: setFor(lines.map((l) => l.row.report), sets), report: heldOr(top.report, reports), github: githubUrl(top.report, reports), newer: null,
    })
  }
  // A report of a measured suite that no manifest row cites, and is newer than the suite's cards: said, not guessed.
  const cited = new Set([...manifest.rows, ...manifest.skipped].map((r) => r.report))
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
    const finding = findings.get(r.name) ?? null
    suites.push({
      id: r.suite, kind: 'suite', title: nameOf(r.suite), tier: null, asOf: r.date, arms: null, lines: [],
      weak: null, finding, legacy: false, noMetric: finding ? null : NO_METRIC,
      headline: r.title, taskSet: setFor([r.name], sets), report: r.name, github: githubUrl(r.name, reports), newer: null,
    })
  }
  // Newest first, as the page promises — with two orderings inside a day, so that the most important work reads
  // first (the owner, 2026-10-27): a run from before its benchmark's tiers is history and reads last of all, and a
  // benchmark measured on the day comes before a suite that only has a report that day.
  const rank = (c: Card) => (c.kind === 'measured' ? 0 : 1)
  return [...out, ...[...measured, ...suites].sort((a, b) =>
    Number(a.legacy) - Number(b.legacy) || b.asOf.localeCompare(a.asOf) || rank(a) - rank(b) ||
    a.title.localeCompare(b.title) || (a.tier ?? '').localeCompare(b.tier ?? ''))]
}
