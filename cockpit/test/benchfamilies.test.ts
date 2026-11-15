// The Benchmarks page says how much it is showing (GitHub issue 17, step 4): every report the build embeds belongs to
// exactly one family, every family is on the page once (ranked in the table, or listed with one reason), and the counts
// the sentence above the table gives come from the build's own files. Over the real `docs/benchmarks/`, as the card
// tests are, so a report that arrives with no family fails here and not on the page.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { readIndex, runs } from '../src/lib/bench.ts'
import { LEAD, readManifest } from '../src/lib/benchindex.ts'
import { cardsOf, OMNIBUS } from '../src/lib/benchcards.ts'
import { UNRANKED_WHY, familiesOf, familyIdOf, ranks, summaryWords, whyNotRanked } from '../src/lib/benchfamilies.ts'

const DIR = new URL('../../docs/benchmarks/', import.meta.url)
const manifest = readManifest(readFileSync(new URL('index.json', DIR), 'utf8'))
// The names the page embeds (`REPORTS` in lib/benchfiles.ts): every markdown file but the README.
const reports = readdirSync(DIR).filter((f) => f.endsWith('.md') && f !== 'README.md').map((f) => f.slice(0, -3)).sort()
const list = runs(reports, readIndex(readFileSync(new URL('README.md', DIR), 'utf8')))
const cards = cardsOf(manifest, list, reports, LEAD, [])
const fam = familiesOf(cards, list, reports, manifest)

test('every report the build embeds belongs to exactly one family', () => {
  assert.ok(reports.length > 50, 'the build holds its reports')
  assert.deepEqual(fam.unplaced, [], 'a report no family takes')
  const placed = fam.families.flatMap((f) => f.reports)
  assert.equal(placed.length, reports.length, 'each report once')
  assert.deepEqual([...placed].sort(), reports, 'the families hold exactly the build\'s reports')
  assert.equal(fam.reports, reports.length)
  assert.ok(fam.families.every((f) => f.reports.length > 0))
})

test('every family is on the page once: a ranked row or one reason, never both and never neither', () => {
  assert.equal(fam.ranked.length + fam.rest.length, fam.families.length)
  assert.equal(new Set(fam.families.map((f) => f.card.id)).size, fam.families.length)
  for (const f of fam.ranked) assert.equal(f.why, null, f.card.id)
  for (const f of fam.rest) {
    assert.ok(f.why && f.why.length > 0 && f.why.length < 90, `${f.card.id} has one short reason: ${f.why}`)
    assert.ok(!/\n/.test(f.why!))
  }
  // Every card that is not a family is a card with no report in this build: there is none.
  assert.equal(fam.families.length, cards.length)
})

test('a family links its newest report, which it holds', () => {
  for (const f of fam.families) {
    assert.ok(reports.includes(f.newest), f.card.id)
    assert.ok(f.reports.includes(f.newest))
    const dates = f.reports.map((r) => list.find((x) => x.name === r)!.date)
    assert.equal(list.find((x) => x.name === f.newest)!.date, [...dates].sort().at(-1), `${f.card.id}: the newest is the latest date`)
  }
})

test('the table ranks what the card rule ranks, and the rule is the old one', () => {
  assert.deepEqual(fam.ranked.map((f) => f.card.id), cards.filter((c) => !c.weak && !c.legacy && c.lines.some((l) => l.theseus)).map((c) => c.id))
  assert.ok(fam.ranked.length >= 1 && fam.rest.length >= 1)
  assert.ok(fam.ranked.every((f) => ranks(f.card)))
})

test('the reasons are the family\'s own: a smoke, an earlier run, a speed measurement, a review', () => {
  const why = (id: string) => fam.rest.find((f) => f.card.id === id)?.why
  assert.match(why('harbor:')!, /2 trials on 1 task/, 'a smoke says its trials and tasks')
  assert.equal(why('terminal-bench@2.0:'), 'superseded by the tiered runs')
  assert.match(why('gate-bench')!, /speed/)
  assert.match(why('gate-bench')!, /no pass rate/)
  assert.match(why(OMNIBUS)!, /not a measurement of its own/)
  assert.equal(whyNotRanked(fam.ranked[0].card), null)
})

test('a suite the build holds has its reason written, so a new suite cannot go unsaid', () => {
  for (const f of fam.rest.filter((x) => x.card.kind !== 'measured')) assert.ok(UNRANKED_WHY[f.card.id], `${f.card.id} has no reason in UNRANKED_WHY`)
  for (const id of Object.keys(UNRANKED_WHY)) assert.ok(fam.families.some((f) => f.card.id === id), `UNRANKED_WHY names ${id}, which the build does not hold`)
})

test('where a report goes: its benchmark and tier, its suite, or the suite\'s oldest measured family', () => {
  const id = (r: string) => familyIdOf(r, list.find((x) => x.name === r)!.suite, cards, manifest)
  assert.equal(id('2026-10-27-terminal-bench-theseus-1.0.3-max-k3'), 'terminal-bench@2.0:max')
  assert.equal(id('2026-10-26-terminal-bench-aider-high-k1'), 'terminal-bench@2.0:high')
  assert.equal(id('2026-10-13-terminal-bench-r1'), 'terminal-bench@2.0:')
  assert.equal(id('2026-10-12-gate-bench-stop-point'), 'gate-bench')
  assert.equal(id('2026-10-26-bughunt-rs-theseus-k3'), 'bughunt-rs', 'a run the manifest sets aside goes to its suite')
  assert.equal(id(OMNIBUS), OMNIBUS)
  // The first full run is cited by no manifest row; Terminal-Bench is measured, so it joins the oldest of its families.
  assert.equal(id('2026-10-04-terminal-bench-first-full-run'), 'terminal-bench:')
  assert.equal(familyIdOf('x', 'unheard-of', cards, manifest), null)
})

test('the sentence above the table counts the build\'s own files and says why the others are not ranked', () => {
  const s = summaryWords(fam)
  assert.ok(s.includes(`holds ${reports.length} reports`), s)
  assert.ok(s.includes(`in ${fam.families.length} families of benchmark`), s)
  assert.ok(s.includes(`ranks ${fam.ranked.length} of them`), s)
  assert.ok(s.includes(`The other ${fam.rest.length} are listed under it with the reason each is not ranked`), s)
  assert.match(s, /measured beside another harness/)
  assert.ok(!/\n/.test(s) && s.split('. ').length <= 4, 'one plain passage')
  // The edges: one family, nothing ranked, nothing left over.
  const one = { ...fam, families: fam.families.slice(0, 1), ranked: [], rest: fam.families.slice(0, 1), reports: 1 }
  assert.match(summaryWords(one), /holds 1 report, in 1 family of benchmark\. None of them can be ranked yet: it is listed below/)
  assert.doesNotMatch(summaryWords({ ...fam, rest: [] }), /The other/)
})
