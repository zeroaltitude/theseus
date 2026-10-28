// The benchmark manifest's pure half (src/lib/benchindex.ts, theseus-jda0z): the newest measurement of each
// (benchmark, tier, arm) with its history, a manifest that cannot be trusted read as empty with its reason, and the
// committed docs/benchmarks/index.json read for real.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { readManifest, seriesOf, sparkline } from '../src/lib/benchindex.ts'

const row = (report: string, date: string, k: number, extra: Record<string, unknown> = {}) => ({
  report, benchmark: 'fixturebench@1', tier: 'high', arm: 'alpha', variant: 'alpha', harness: 'Alpha',
  model: 'model-x', date, commit: 'abc12345', dataset: `docs/benchmarks/${report}.json`, trials: 9, tasks: 3,
  metrics: { pass_rate: { value: k / 9, lo: k / 9 - 0.1, hi: k / 9 + 0.1, k, n: 9 } }, ...extra,
})
const manifest = (runs: unknown[], version: unknown = 1) => JSON.stringify({ version, about: 'x', runs })

test('the newest row of a key wins, and its history runs oldest to newest', () => {
  // Written out of order: the manifest's own order is not what picks the newest.
  const m = readManifest(manifest([row('2026-03-05-b', '2026-03-05', 6), row('2026-03-01-a', '2026-03-01', 3), row('2026-03-03-c', '2026-03-03', 4)]))
  assert.equal(m.reason, null)
  assert.equal(m.series.length, 1)
  const s = m.series[0]
  assert.equal(s.newest.report, '2026-03-05-b')
  assert.equal(s.newest.pass.k, 6)
  assert.deepEqual(s.history.map((r) => r.date), ['2026-03-01', '2026-03-03', '2026-03-05'])
})

test('keys keep (benchmark, tier, arm) apart, and a carried row never speaks for a key', () => {
  const m = readManifest(manifest([
    row('2026-03-01-a', '2026-03-01', 3),
    row('2026-03-02-b', '2026-03-02', 5, { tier: 'max' }),
    row('2026-03-03-c', '2026-03-03', 3, { carried_from: '2026-03-01-a' }),
    row('2026-03-04-d', '2026-03-04', 7, { comparison: true }),
    row('2026-03-04-d', '2026-03-04', 8, { arm: 'theseus', variant: 'theseus-rt', harness: 'Theseus' }),
  ]))
  assert.deepEqual(m.series.map((s) => [s.arm, s.tier, s.newest.report]),
    [['theseus', 'high', '2026-03-04-d'], ['alpha', 'high', '2026-03-01-a'], ['alpha', 'max', '2026-03-02-b']])
  assert.equal(m.rows.length, 5, 'the rows stay; only the series leave the carried ones out')
})

test('two variants of an arm in one report make one point of its history', () => {
  const s = seriesOf(readManifest(manifest([
    row('2026-03-01-a', '2026-03-01', 3, { variant: 'alpha' }),
    row('2026-03-01-a', '2026-03-01', 4, { variant: 'alpha-routed' }),
    row('2026-03-02-b', '2026-03-02', 5),
  ])).rows)
  assert.deepEqual(s[0].history.map((r) => r.variant), ['alpha-routed', 'alpha'])
})

test('a manifest it cannot trust is empty, with a named reason, and never throws', () => {
  const bad: [unknown, RegExp][] = [
    [undefined, /no benchmark manifest/], [null, /no benchmark manifest/], ['', /no benchmark manifest/],
    ['{not json', /not valid JSON/], ['[]', /not an object/], [manifest([], 2), /version 2/],
    [JSON.stringify({ version: 1 }), /no `runs`/],
    [manifest([row('r', '2026-03-01', 3), { report: 'x' }]), /row 1 has no `benchmark`/],
    [manifest([row('r', 'March', 3)]), /row 0 .*date/],
    [manifest([row('r', '2026-03-01', 3, { trials: '9' })]), /row 0 has no `trials`/],
    [manifest([null]), /row 0 is not an object/],
    // A `pass_rate` that is there and broken is a fault of the manifest, not a run measured another way. The file is
    // hand-edited, so a `lo`/`low` slip is the realistic case, and it must not quietly drop a cell from the page.
    [manifest([row('r', '2026-03-01', 3, { metrics: { pass_rate: { value: 'high' } } })]), /row 0 has no whole `metrics.pass_rate`/],
    [manifest([row('r', '2026-03-01', 3, { metrics: { pass_rate: { value: 0.8, low: 0.7, hi: 0.9, k: 8, n: 10 } } })]), /row 0 has no whole `metrics.pass_rate`/],
  ]
  for (const [text, why] of bad) {
    const m = readManifest(text as string)
    assert.deepEqual([m.rows, m.series, m.skipped], [[], [], []], String(text))
    assert.match(m.reason ?? '', why)
  }
})

test('a whole row that states no pass rate is set aside by name, and never costs the manifest its other rows', () => {
  // theseus-s5qwv: Bug Hunt RS counts bugs fixed a trial. Before this one row emptied the whole manifest and the
  // Benchmarks page lost every card it had.
  const m = readManifest(manifest([
    row('good', '2026-03-01', 3),
    row('bugs', '2026-03-02', 3, { benchmark: 'bughunt-rs', metrics: { bugs_fixed_per_trial: { value: 3.33 } } }),
  ]))
  assert.equal(m.reason, null)
  assert.equal(m.rows.length, 1)
  assert.equal(m.rows[0].report, 'good')
  assert.deepEqual(m.skipped.map((s) => [s.report, s.benchmark]), [['bugs', 'bughunt-rs']])
  assert.match(m.skipped[0].why, /pass_rate/)
  // Only an absent pass rate is set aside. A row with no `metrics` at all states none either.
  const none = readManifest(manifest([row('r', '2026-03-01', 3, { metrics: undefined })]))
  assert.equal(none.reason, null)
  assert.equal(none.skipped.length, 1)
})

test('the sparkline puts the lowest rate at the foot and the highest at the top, and centres a flat one', () => {
  assert.deepEqual(sparkline([0.5, 0.75, 1], 100, 10), [[0, 10], [50, 5], [100, 0]])
  assert.deepEqual(sparkline([0.7, 0.7], 40, 8), [[0, 4], [40, 4]])
  assert.deepEqual(sparkline([0.7], 40, 8), [[20, 4]])
  assert.deepEqual(sparkline([], 40, 8), [])
})

test('the committed manifest parses, and its newest Terminal-Bench rows for Theseus are the newest cells', () => {
  const text = readFileSync(new URL('../../docs/benchmarks/index.json', import.meta.url), 'utf8')
  const m = readManifest(text)
  assert.equal(m.reason, null)
  assert.ok(m.rows.length > 40)
  const tb = m.series.filter((s) => s.benchmark === 'terminal-bench@2.0')
  const theseus = (tier: string) => tb.find((s) => s.arm === 'theseus' && s.tier === tier)!
  // The max tier gained the frozen 1.0.3 cell on 10-27, all 30 of the hard set x 3; its commit is recorded as the
  // bench binaries' build rather than a bare sha, since the trials agree on one and the harbor arm sits elsewhere.
  assert.equal(theseus('max').newest.report, '2026-10-27-terminal-bench-theseus-1.0.3-max-k3')
  assert.equal(theseus('max').newest.date, '2026-10-27')
  assert.equal(theseus('max').newest.tasks, 30)
  assert.equal(theseus('max').newest.trials, 90)
  assert.match(theseus('max').newest.commit ?? '', /92e3dbed/)
  // The high tier gained the frozen 1.0.3 cell on 10-26, all 89 tasks x 3 (theseus-cfttq).
  assert.equal(theseus('high').newest.report, '2026-10-26-terminal-bench-theseus-1.0.3-high-k3')
  assert.equal(theseus('high').newest.date, '2026-10-26')
  assert.equal(theseus('high').newest.tasks, 89)
  assert.equal(theseus('high').newest.trials, 267)
  // The history reaches back through the 10-21 rerun to the v1 run, one point a report.
  assert.deepEqual(theseus('max').history.map((r) => r.date), ['2026-10-18', '2026-10-21', '2026-10-23', '2026-10-27'])
  assert.deepEqual(theseus('high').history.map((r) => r.date), ['2026-10-18', '2026-10-21', '2026-10-23', '2026-10-26'])
  // The rivals were not run again on 10-23: their newest measurement is the v1 run's, not a re-cut of it.
  assert.equal(tb.find((s) => s.arm === 'claude-code' && s.tier === 'max')!.newest.date, '2026-10-18')
  // Every series' newest is its history's last.
  for (const s of m.series) assert.equal(s.newest, s.history[s.history.length - 1])
})
