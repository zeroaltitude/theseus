// The card view's hotfix (theseus-s5qwv), over the real index.json and README: a run the reader cannot score is set
// aside by name and never costs the page its other cards; a card prints a pass rate only where the run supports one,
// and otherwise its own one-line finding; the untiered Terminal-Bench 2.0 run is labelled as the earlier run and sinks.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { readIndex, runs } from '../src/lib/bench.ts'
import { readManifest } from '../src/lib/benchindex.ts'
import { cardsOf, headlineWhy, LEGACY_WORDS, NO_METRIC, plainFinding } from '../src/lib/benchcards.ts'

const DIR = new URL('../../docs/benchmarks/', import.meta.url)
const manifest = readManifest(readFileSync(new URL('index.json', DIR), 'utf8'))
const reports = readdirSync(DIR).filter((f) => f.endsWith('.md') && f !== 'README.md').map((f) => f.slice(0, -3)).sort()
const list = runs(reports, readIndex(readFileSync(new URL('README.md', DIR), 'utf8')))
const cards = cardsOf(manifest, list, reports)
const card = (id: string) => cards.find((c) => c.id === id)

test('a run with no pass rate is set aside by name, and the manifest keeps every other row', () => {
  // Bug Hunt RS counts bugs fixed a trial, never a pass rate. Before this it returned EMPTY and the page lost
  // every card it had.
  assert.equal(manifest.reason, null)
  assert.ok(manifest.series.length > 10)
  const aside = manifest.skipped.find((s) => s.benchmark === 'bughunt-rs')
  assert.ok(aside, 'the Bug Hunt row is named among the skipped')
  assert.equal(aside!.report, '2026-10-26-bughunt-rs-theseus-k3')
  assert.match(aside!.why, /pass rate/i)
  // A row that is not a row at all is still a fault of the manifest, and still says so.
  assert.match(readManifest('{"version":1,"runs":[{"benchmark":"x"}]}').reason!, /row 0/)
})

test('Bug Hunt gets a card of its own, named, from its report', () => {
  const c = card('bughunt-rs')
  assert.ok(c, 'there is a Bug Hunt card')
  assert.equal(c!.title, 'Bug Hunt RS')
  assert.equal(c!.kind, 'suite')
  assert.equal(c!.asOf, '2026-10-26')
  assert.equal(c!.report, '2026-10-26-bughunt-rs-theseus-k3')
  // Its own finding stands in the metric's place, with the run's numbers in it.
  assert.ok(c!.finding && c!.finding.length > 40)
  assert.match(c!.finding!, /13 planted Rust bugs/)
  assert.ok(!c!.finding!.includes('**'))
})

test('the 10-26 cell is what the high tier shows for Theseus', () => {
  const c = card('terminal-bench@2.0:high')!
  // The card is as new as its newest arm, which is a rival's 10-27 run; Theseus's own row is the 10-26 cell.
  assert.ok(c.asOf >= '2026-10-26', c.asOf)
  const mine = c.lines.find((l) => l.theseus)!
  assert.equal(mine.row.date, '2026-10-26')
  assert.equal(mine.row.trials, 267)
  assert.equal(mine.row.tasks, 89)
  assert.ok(Math.abs(mine.row.pass.value - 0.783) < 0.002, String(mine.row.pass.value))
})

test('a card prints a pass rate only where the run supports one; a smoke says what it found instead', () => {
  // A measured card's id is `<benchmark>:<tier>`; Harbor has no tiers, so its id ends in the empty one.
  const harbor = card('harbor:')!
  // 100.0% [34.2%, 100.0%] on 2 trials of 1 task is a smoke, and never the card's headline.
  assert.ok(harbor.weak, 'the Harbor card is weak')
  assert.match(harbor.weak!, /1 task/)
  assert.ok(harbor.finding, 'and shows its own finding instead')
  assert.match(harbor.finding!, /fix-git/)
  // No data lost: every arm it measured is still on the card, with its numbers.
  assert.equal(harbor.lines.length, 3)
  assert.ok(harbor.lines.every((l) => l.row.trials === 2))

  const high = card('terminal-bench@2.0:high')!
  assert.equal(high.weak, null, 'a 267-trial cell over 89 tasks is a headline')

  // The rule itself, on invented rows.
  const row = (tasks: number | null, lo: number, hi: number, trials: number) =>
    ({ tasks, trials, pass: { value: (lo + hi) / 2, lo, hi, k: 1, n: trials } }) as never
  assert.equal(headlineWhy(row(89, 0.72, 0.83, 267)), null)
  assert.match(headlineWhy(row(1, 0.342, 1, 2))!, /1 task/)
  assert.match(headlineWhy(row(30, 0.1, 0.9, 40))!, /interval/)
  assert.equal(headlineWhy(row(null, 0.72, 0.83, 267)), null, 'a run that counts no tasks is judged on its interval')
  // A Wilson interval narrows towards 0 and 1, so width alone is most forgiving exactly where a small sample's
  // extremes live: 8 of 8 is [67.6%, 100.0%], inside half the scale, and still a smoke. The count is asked too.
  assert.match(headlineWhy(row(2, 0.676, 1, 8))!, /8 trials behind it/)
  assert.match(headlineWhy(row(2, 0, 0.49, 4))!, /4 trials behind it/)
  assert.equal(headlineWhy(row(2, 0.824, 1, 18)), null)
  // The words count properly at one.
  assert.match(headlineWhy(row(1, 0, 1, 1))!, /^1 trial on 1 task:/)
  // Harbor's interval is the whole evidence for the verdict, so it is still on the card to be read.
  assert.ok(harbor.lines.every((l) => l.row.pass.hi - l.row.pass.lo > 0.5))
})

test('a row the reader cannot score is handed to the page, and still counts as cited by the manifest', () => {
  // theseus-s5qwv finding 2: "named rather than dropped in silence" needs a reader. The view passes
  // `manifest.skipped` to BenchIndex, which prints it; without that a set-aside row for a benchmark with no README
  // row would be absent from the page with no notice anywhere.
  assert.ok(manifest.skipped.length > 0)
  const bughunt = card('bughunt-rs')!
  // And a set-aside report is still a report the manifest holds, so it is never announced as "not in the manifest yet".
  assert.equal(bughunt.newer, null)
  for (const c of cards) {
    if (c.newer) assert.ok(!manifest.skipped.some((s) => s.report === c.newer!.report), c.id)
  }
})

test('every suite card says what its run found, and falls back to the old words only with no finding', () => {
  const suites = cards.filter((c) => c.kind === 'suite')
  assert.ok(suites.length > 4)
  for (const c of suites) {
    assert.deepEqual(c.lines, [])
    assert.ok(c.finding ?? c.noMetric, `${c.id} says something in the metric's place`)
    if (c.finding) assert.equal(c.noMetric, null, `${c.id} does not say both`)
  }
  // The README's headline is the finding, its markup taken off and nothing added.
  assert.equal(plainFinding('**A** `b` c'), 'A b c')
  assert.equal(plainFinding('  x   y  '), 'x y')
  const recall = card('recall')!
  assert.equal(recall.asOf, '2026-10-26')
  assert.match(recall.finding!, /seed/i)
  assert.ok(!recall.finding!.includes(NO_METRIC))
})

test('the untiered Terminal-Bench 2.0 run is labelled the earlier run, and sinks below the tiered cards', () => {
  const legacy = card('terminal-bench@2.0:')!
  assert.equal(legacy.legacy, true)
  assert.equal(legacy.asOf, '2026-10-13')
  assert.ok(LEGACY_WORDS.length > 0)
  // A benchmark with no tiers at all is never legacy, and neither is a tiered card.
  assert.equal(card('terminal-bench@2.0:high')!.legacy, false)
  // "the earlier run" is a claim about time, and it is asked of time: an untiered run NEWER than every tiered one
  // is not history, however untiered, and must not be labelled or sunk.
  const later = cardsOf({ ...manifest, series: manifest.series.map((s) => (s.tier ? s : { ...s, newest: { ...s.newest, date: '2026-12-01' } })) }, list, reports)
  assert.equal(later.find((c) => c.id === 'terminal-bench@2.0:')!.legacy, false)
  for (const c of cards.filter((c) => c.id.startsWith('terminal-bench:') || c.kind !== 'measured')) assert.equal(c.legacy, false, c.id)
  // It sits after every card that is not legacy.
  const i = cards.indexOf(legacy)
  assert.ok(cards.slice(0, i).every((c) => !c.legacy))
  assert.ok(cards.slice(i).every((c) => c.legacy))
})

test('the omnibus stays pinned and the rest stay newest first among their kind', () => {
  assert.equal(cards[0].id, 'omnibus')
  const rest = cards.slice(1).filter((c) => !c.legacy)
  for (let i = 1; i < rest.length; i++) assert.ok(rest[i - 1].asOf >= rest[i].asOf, `${rest[i - 1].id} before ${rest[i].id}`)
  // The newest measured work reads first (the owner: Omnibus and TB2 should read first).
  assert.equal(rest[0].id, 'terminal-bench@2.0:high')
  // And the two suites of 2026-10-26 follow it, in their own order.
  assert.deepEqual(rest.filter((c) => c.asOf === '2026-10-26').map((c) => c.id), ['bughunt-rs', 'recall'])
  // Within a day a measured card always precedes a suite card.
  for (let i = 1; i < rest.length; i++) {
    if (rest[i - 1].asOf === rest[i].asOf && rest[i - 1].kind !== rest[i].kind) assert.equal(rest[i - 1].kind, 'measured')
  }
})
