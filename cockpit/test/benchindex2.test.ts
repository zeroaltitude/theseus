// The browse cards (theseus-sam3s) over the real index.json and README: the lead rule, the GitHub links, and a suite
// with no manifest row showing no number.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { GITHUB_DOCS, readIndex, runs } from '../src/lib/bench.ts'
import { groupTasks, LEAD, leadRow, readManifest } from '../src/lib/benchindex.ts'
import { cardsOf, NO_METRIC } from '../src/lib/benchcards.ts'

const DIR = new URL('../../docs/benchmarks/', import.meta.url)
const manifest = readManifest(readFileSync(new URL('index.json', DIR), 'utf8'))
const reports = readdirSync(DIR).filter((f) => f.endsWith('.md') && f !== 'README.md').map((f) => f.slice(0, -3)).sort()
const list = runs(reports, readIndex(readFileSync(new URL('README.md', DIR), 'utf8')))
const high = manifest.series.filter((s) => s.benchmark === 'terminal-bench@2.0' && s.tier === 'high')

/** The committed manifest with one run withheld, so a rule can be proved on the row it is about rather than on
 *  whichever run happens to be newest the day the test is read. */
function withoutRun(report: string) {
  const raw = JSON.parse(readFileSync(new URL('index.json', DIR), 'utf8'))
  return readManifest(JSON.stringify({ ...raw, runs: raw.runs.filter((r: { report: string }) => r.report !== report) }))
}

test('the lead rule is one constant, and the two rules pick different rows for Theseus on the high tier', () => {
  assert.equal(manifest.reason, null)
  const t = high.find((s) => s.arm === 'theseus')!, largest = groupTasks(high)
  assert.equal(largest, 89)
  // Since the 10-26 cell (all 89 tasks x 3) the newest row is also the comparable one, so the two rules agree here.
  const newest = leadRow(t, 'newest', largest)
  assert.equal(newest.date, '2026-10-26')
  assert.equal(newest.tasks, 89)
  const comparable = leadRow(t, 'newest-comparable', largest)
  assert.equal(comparable, newest)
  // They still part on a series whose newest row is short: the 10-23 retest ran 6 of the 89 tasks.
  const short = { ...t, history: t.history.filter((r) => r.date <= '2026-10-23'), newest: t.history.find((r) => r.date === '2026-10-23')! }
  assert.equal(leadRow(short, 'newest', largest).tasks, 6)
  assert.equal(leadRow(short, 'newest-comparable', largest).tasks, 89)
  assert.ok(['newest', 'newest-comparable'].includes(LEAD))
  // An arm with no row at the largest count falls back to its newest.
  const c = high.find((s) => s.arm === 'codex')!
  assert.equal(leadRow(c, 'newest-comparable', largest), c.newest)
})

test('every row prints trials and tasks, and a short row carries its chip, under either rule', () => {
  for (const rule of ['newest', 'newest-comparable'] as const) {
    const card = cardsOf(manifest, list, reports, rule).find((c) => c.id === 'terminal-bench@2.0:high')!
    for (const l of card.lines) {
      assert.ok(l.row.trials > 0)
      if (l.row.tasks !== null && l.row.tasks < 89) assert.equal(l.chip, `${l.row.tasks} of 89 tasks`)
      else assert.equal(l.chip, null)
    }
  }
  // Theseus's newest high-tier row now covers all 89 tasks, so under either rule it carries no chip.
  const t = cardsOf(manifest, list, reports, 'newest').find((c) => c.id === 'terminal-bench@2.0:high')!.lines.find((l) => l.theseus)!
  assert.equal(t.chip, null)
  // The max tier's lead is the 10-27 frozen 1.0.3 run, all 30 of the hard set, so it carries none either.
  const max = cardsOf(manifest, list, reports, 'newest').find((c) => c.id === 'terminal-bench@2.0:max')!.lines.find((l) => l.theseus)!
  assert.equal(max.row.tasks, 30)
  assert.equal(max.chip, null)
  // Every row the manifest now holds is a full one, so the chip rule has no live example to stand on. Withhold the
  // 10-27 run and the lead falls back to the 10-23 retest, which ran 27 of the hard 30 and must say so: the rule is
  // proved on a short row rather than on whichever run happens to be newest.
  const short = withoutRun('2026-10-27-terminal-bench-theseus-1.0.3-max-k3')
  const lead = cardsOf(short, list, reports, 'newest').find((c) => c.id === 'terminal-bench@2.0:max')!.lines.find((l) => l.theseus)!
  assert.equal(lead.row.report, '2026-10-23-terminal-bench-retest-max')
  assert.equal(lead.chip, '27 of 30 tasks')
})

test('every GitHub address names a report that exists, in the one shape', () => {
  const re = /^https:\/\/github\.com\/zeroaltitude\/theseus\/blob\/main\/docs\/benchmarks\/[A-Za-z0-9._-]+\.md$/
  const cards = cardsOf(manifest, list, reports)
  assert.ok(cards.length > 5)
  for (const c of cards) {
    if (!c.github) continue
    assert.match(c.github, re)
    assert.ok(c.github.startsWith(GITHUB_DOCS))
    assert.ok(reports.includes(c.github.slice(GITHUB_DOCS.length, -3)), c.github)
  }
  // A report the build does not hold gets no link rather than a dead one.
  assert.ok(cardsOf(manifest, list, ['omnibus']).filter((c) => c.report !== 'omnibus').every((c) => c.github === null))
})

test('the omnibus is pinned first, and a suite with no manifest row shows no numeral of its own', () => {
  const cards = cardsOf(manifest, list, reports)
  assert.equal(cards[0].id, 'omnibus')
  assert.equal(cards[0].github, `${GITHUB_DOCS}omnibus.md`)
  const suites = cards.filter((c) => c.kind === 'suite')
  const ids = suites.map((c) => c.id)
  for (const want of ['gate-bench', 'memory-exam', 'retrieval', 'recall', 'async', 'context']) assert.ok(ids.includes(want), want)
  for (const c of suites) {
    // Its run's own one-line finding stands in the metric's place; the old words are the last resort (theseus-s5qwv).
    assert.equal(c.noMetric, c.finding ? null : NO_METRIC)
    assert.deepEqual(c.lines, [])
    assert.equal(c.arms, null)
  }
  assert.ok(!NO_METRIC.match(/\d/))
  // Newest first after the pin, the runs from before their benchmark's tiers excepted: they read last.
  const flow = cards.slice(1).filter((c) => !c.legacy)
  for (let i = 1; i < flow.length; i++) assert.ok(flow[i - 1].asOf >= flow[i].asOf)
})

test('a manifest it cannot read leaves the suite cards, with no number', () => {
  const cards = cardsOf(readManifest('{'), list, reports)
  const tb = cards.find((c) => c.id === 'terminal-bench')!
  assert.equal(tb.kind, 'suite')
  assert.ok(tb.finding ?? tb.noMetric)
  assert.ok(cards.every((c) => c.lines.length === 0))
  assert.ok(cards.every((c) => c.weak === null))
})

test('"every test" opens a set only where a task set reads one of the card’s reports', () => {
  const sets = [{ id: 'tb-v1-high', report: '2026-10-18-terminal-bench-v1-high' }]
  const cards = cardsOf(manifest, list, reports, LEAD, sets)
  assert.equal(cards.find((c) => c.id === 'terminal-bench@2.0:high')!.taskSet, 'tb-v1-high')
  assert.equal(cards.find((c) => c.id === 'terminal-bench@2.0:max')!.taskSet, null)
  assert.ok(cards.filter((c) => c.kind === 'suite').every((c) => c.taskSet === null))
})
