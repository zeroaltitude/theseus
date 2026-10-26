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

test('the lead rule is one constant, and the two rules pick different rows for Theseus on the high tier', () => {
  assert.equal(manifest.reason, null)
  const t = high.find((s) => s.arm === 'theseus')!, largest = groupTasks(high)
  assert.equal(largest, 89)
  const newest = leadRow(t, 'newest', largest)
  assert.equal(newest.date, '2026-10-23')
  assert.equal(newest.tasks, 6)
  const comparable = leadRow(t, 'newest-comparable', largest)
  assert.equal(comparable.tasks, 89)
  assert.equal(comparable.date, '2026-10-21')
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
  const t = cardsOf(manifest, list, reports, 'newest').find((c) => c.id === 'terminal-bench@2.0:high')!.lines.find((l) => l.theseus)!
  assert.equal(t.chip, '6 of 89 tasks')
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
    assert.equal(c.noMetric, NO_METRIC)
    assert.deepEqual(c.lines, [])
    assert.equal(c.arms, null)
  }
  assert.ok(!NO_METRIC.match(/\d/))
  // Newest first after the pin.
  for (let i = 2; i < cards.length; i++) assert.ok(cards[i - 1].asOf >= cards[i].asOf)
})

test('a manifest it cannot read leaves the suite cards, with no number', () => {
  const cards = cardsOf(readManifest('{'), list, reports)
  assert.ok(cards.some((c) => c.id === 'terminal-bench' && c.kind === 'suite' && c.noMetric === NO_METRIC))
  assert.ok(cards.every((c) => c.lines.length === 0))
})

test('"every test" opens a set only where a task set reads one of the card’s reports', () => {
  const sets = [{ id: 'tb-v1-high', report: '2026-10-18-terminal-bench-v1-high' }]
  const cards = cardsOf(manifest, list, reports, LEAD, sets)
  assert.equal(cards.find((c) => c.id === 'terminal-bench@2.0:high')!.taskSet, 'tb-v1-high')
  assert.equal(cards.find((c) => c.id === 'terminal-bench@2.0:max')!.taskSet, null)
  assert.ok(cards.filter((c) => c.kind === 'suite').every((c) => c.taskSet === null))
})
