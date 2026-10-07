// The Benchmarks tab's pure half (src/lib/bench.ts), on the real published runs (docs/benchmarks/): the index, the
// points against each report's own numbers, every note's phrase in its report, every figure's table, and the frontier.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import {
  answerFirst, atAGlance, axes, sections, figureTable, frontier, linkTo, modelOf, parseCsv, points, property, readIndex,
  readerMarkdown, reportName, runs, staircase, TASK_SETS, unequal, wilson, interval, EFFORT, type Point, type Property,
} from '../src/lib/bench.ts'

const DIR = new URL('../../docs/benchmarks/', import.meta.url)
const read = (f: string) => readFileSync(new URL(f, DIR), 'utf8')
const files = readdirSync(DIR)
const mds = files.filter((f) => f.endsWith('.md') && f !== 'README.md').map((f) => f.slice(0, -3))
const pointsOf = (id: string): Point[] => {
  const set = TASK_SETS.find((s) => s.id === id)!
  return points(set, read(`${set.report}.csv`), read(`${set.report}.json`), read(`${set.report}.md`), set.report.slice(0, 10))
}
const at = (ps: Point[], harness: string, run?: string) => ps.find((p) => p.harness === harness && (!run || p.run === run))!
const P = (id: string) => property(id)!
const squash = (s: string) => s.replace(/\s+/g, ' ')

test('the index lists every report, and every report is in the index', () => {
  const index = readIndex(read('README.md'))
  assert.equal(index.length, mds.length)
  assert.deepEqual(new Set(index.map((r) => r.name)), new Set(mds))
  const tb2 = index.find((r) => r.name === '2026-10-04-terminal-bench-first-full-run')!
  assert.equal(tb2.suite, 'terminal-bench')
  assert.match(tb2.headline, /Claude Code 81\.5%/)
  // Newest first, the index's own order inside a day; a report the index lacks still lists, from its name.
  const list = runs([...mds, '2026-10-09-gate-bench-new-one', 'README'], index, new Map([['2026-10-09-gate-bench-new-one', 'A new one']]))
  assert.equal(list.length, mds.length + 1)
  assert.deepEqual(list[0], { name: '2026-10-09-gate-bench-new-one', date: '2026-10-09', suite: 'gate-bench', title: 'A new one', indexed: false })
  assert.equal(list[1].name, index[0].name)
  for (let i = 1; i < list.length; i++) assert.ok(list[i - 1].date >= list[i].date)
})

test("a report's parts: its answer, its table at a glance, its model", () => {
  const md = read('2026-10-04-terminal-bench-first-full-run.md')
  assert.match(answerFirst(md)!, /^On all 89 tasks of Terminal-Bench 2\.0/)
  const glance = atAGlance(md)
  assert.deepEqual(glance.map((r) => r.label), ['Suite', 'Arms', 'Model', 'Tasks × attempts', 'Date and commit', 'Cost', 'Data'])
  assert.equal(modelOf(md), 'Claude Sonnet 5.5')
  assert.deepEqual(sections(md).slice(0, 4), ['The question', 'The setup', 'Results', 'Analysis'])
  assert.equal(modelOf(read('2026-10-06-harbor-efficiency-checks.md')), 'Claude Sonnet 5.5')
  for (const name of mds) assert.ok(answerFirst(read(`${name}.md`)), `${name} has its answer first`)
  assert.equal(reportName('/x/docs/benchmarks/2026-10-06-async-smokes.csv'), '2026-10-06-async-smokes')
})

test('the first full run: each arm as the report counts it', () => {
  const ps = pointsOf('tb2')
  assert.deepEqual(ps.map((p) => p.harness), ['theseus', 'claude-code', 'theseus-batching'])
  const a = at(ps, 'theseus'), b = at(ps, 'theseus-batching'), c = at(ps, 'claude-code')
  // 128, 131 and 145 of 178 trials, with the report's Wilson intervals.
  assert.equal(a.values.pass.v * 178, 128)
  assert.equal(Math.round(b.values.pass.v * 178), 131)
  assert.equal(Math.round(c.values.pass.v * 178), 145)
  assert.ok(Math.abs(a.values.pass.lo! - 0.648985) < 1e-5 && Math.abs(a.values.pass.hi! - 0.77996) < 1e-5)
  const json = JSON.parse(read('2026-10-04-terminal-bench-first-full-run.json'))
  // Dollars a trial over the priced trials, and a solve's price, to the report's own figures.
  assert.ok(Math.abs(a.values.usd.v - json.summary.arms.A.dollars_per_trial.mean) < 1e-4)
  assert.equal(a.values.usd.n, json.summary.arms.A.dollars_per_trial.n)
  assert.ok(Math.abs(a.values['usd-solved'].v - json.summary.arms.A.dollars_per_solved) < 1e-3)
  assert.ok(Math.abs(a.values['agent-s'].v - json.summary.arms.A.agent_secs.median) < 0.05) // the CSV keeps a tenth of a second
  // A mean's interval is a seeded bootstrap, as the report's: its ends within a small share of the report's width.
  const ci = interval(P('usd'), a.values.usd), rep = json.summary.arms.A.dollars_per_trial
  assert.equal(ci.kind, 'bootstrap')
  assert.ok(Math.abs(ci.lo - rep.lo) < 0.1 * (rep.hi - rep.lo) && Math.abs(ci.hi - rep.hi) < 0.1 * (rep.hi - rep.lo), `${ci.lo} ${ci.hi}`)
  assert.deepEqual(interval(P('usd'), a.values.usd), ci) // the same on every page: seeded, and kept
  assert.equal(interval(P('pass'), a.values.pass).kind, 'wilson')
  // Claude Code's six Sonnet 5 trials are in the data, and mark it; the paragraph is the ablation's note.
  assert.ok(c.tags.some((t) => t.kind === 'other-model' && t.text.startsWith('6 trials')))
  assert.ok(unequal(c) && unequal(b) && !unequal(a))
  assert.equal(a.version?.text, 'built at 079f1db')
  assert.equal(c.version?.text, '2.1.288')
  assert.equal(a.model, 'Claude Sonnet 5.5')
  assert.equal(a.effort, EFFORT.theseus)
  // Never sampled: the full run has no harness memory or CPU, and no axis offers them.
  const { usable } = axes(ps)
  assert.ok(usable.some((p) => p.id === 'pass') && usable.some((p) => p.id === 'tokens'))
  assert.ok(!usable.some((p) => p.id === 'rss' || p.id === 'cpu-tool'))
})

test("fix-git's runs: a point a run and harness, the harness's own memory and CPU", () => {
  const ps = pointsOf('fixgit')
  // Theseus in six runs, Claude Code in five, the paragraph in three, Pi once; the plumbing's prove-plus-comm left out.
  assert.equal(ps.filter((p) => p.harness === 'theseus').length, 6)
  assert.equal(ps.filter((p) => p.harness === 'claude-code').length, 5)
  assert.equal(ps.filter((p) => p.harness === 'theseus-batching').length, 3)
  assert.equal(ps.filter((p) => p.harness === 'pi').length, 1)
  assert.equal(ps.reduce((n, p) => n + p.n, 0), 20)
  const r16 = at(ps, 'theseus', 'r16'), cc16 = at(ps, 'claude-code', 'r16'), pi = at(ps, 'pi')
  // Under ten trials an interval is the range, by the reports' rule.
  assert.deepEqual(interval(P('usd'), at(ps, 'theseus', 'plumbing').values.usd), { lo: 0.02558, hi: 0.05352, kind: 'range' })
  assert.equal(r16.values.rss.v, 25.21)
  assert.equal(cc16.values.rss.v, 430.18)
  assert.equal(pi.values['cpu-tool'].v, 108)
  assert.equal(pi.version?.text, '1.0.4')
  assert.equal(cc16.version?.text, '2.1.288')
  // The plumbing's forced timeouts: in the data, and in the report's words.
  const plumbing = at(ps, 'theseus', 'plumbing')
  assert.equal(plumbing.values.pass.v, 1 / 3)
  assert.ok(plumbing.tags.some((t) => t.kind === 'exception' && t.text.startsWith('2 of 3')))
  assert.ok(plumbing.tags.some((t) => t.kind === 'note' && /10\.8 s/.test(t.text)))
  // A Theseus build the report does not state is not guessed.
  assert.equal(at(ps, 'theseus', 'b5-smoke').version, undefined)
  const { usable } = axes(ps)
  for (const id of ['rss', 'cpu-tool', 'cpu', 'container-mem', 'setup-s', 'usd']) assert.ok(usable.some((p) => p.id === id), id)
  // The efficiency report's median dollars, by harness over its runs.
  const json = JSON.parse(read('2026-10-06-harbor-efficiency-checks.json'))
  const trials = parseCsv(read('2026-10-06-harbor-efficiency-checks.csv')).filter((r) => r.task === 'fix-git' && r.arm === 'theseus')
  const ds = trials.map((r) => Number(r.dollars)).sort((x, y) => x - y)
  assert.equal(ds.length, json.summary.fixgit_dollars.theseus.n + 2) // its seven solved trials and the two forced timeouts
})

test("the worth spike's paragraph ran three of the four tasks; the async interrupt's latency", () => {
  const w = pointsOf('worth')
  const thb = at(w, 'theseus-batching')
  assert.equal(thb.tasks, 3)
  assert.ok(thb.tags.some((t) => t.kind === 'subset' && t.text === 'ran 3 of the set’s 4 tasks'))
  assert.ok(!at(w, 'theseus').tags.some((t) => t.kind === 'subset'))
  const a = pointsOf('async')
  assert.deepEqual(a.map((p) => [p.harness, p.n]), [['theseus', 2], ['claude-code', 2]])
  assert.equal(at(a, 'theseus').values['respond-s'].v, (46.4 + 49.2) / 2)
  assert.equal(at(a, 'claude-code').values['respond-s'].v, (146.6 + 162.8) / 2)
  // Theseus's turn-phase split was measured for Theseus alone in the spike: no axis pretends otherwise.
  assert.ok(axes(a).usable.some((p) => p.id === 'respond-s'))
})

test("every note rests on its report's exact words", () => {
  const notes = [
    ...Object.values(EFFORT),
    ...TASK_SETS.flatMap((s) => [
      ...s.conditions,
      ...Object.values(s.versions).flatMap((m) => Object.values(m)),
      ...Object.values(s.notes).flatMap((m) => Object.values(m).flat()),
    ]),
  ]
  assert.ok(notes.length > 20)
  for (const n of notes) assert.ok(squash(read(`${n.report}.md`)).includes(squash(n.quote)), `"${n.quote}" is in ${n.report}`)
})

test("every figure of every report gives the table of its numbers", () => {
  let figures = 0
  for (const f of files.filter((x) => x.endsWith('.json'))) {
    for (const spec of JSON.parse(read(f)).figures ?? []) {
      figures++
      const t = figureTable(spec)
      assert.ok(t.rows.length > 0, `${f} ${spec.name} has rows`)
      for (const r of t.rows) {
        assert.equal(r.length, t.columns.length, `${f} ${spec.name}`)
        for (const c of r) assert.ok(!/\[object Object\]|undefined|NaN/.test(c), `${f} ${spec.name}: ${c}`)
      }
      // Each figure's SVG, light and dark, is beside it.
      const stem = f.slice(0, -5)
      assert.ok(files.includes('img') && readdirSync(new URL(`img/${stem}/`, DIR)).includes(`${spec.name}-dark.svg`), `${stem}/${spec.name}`)
    }
  }
  assert.ok(figures >= 50)
  const tb2 = JSON.parse(read('2026-10-04-terminal-bench-first-full-run.json')).figures
  const pass = figureTable(tb2.find((x: { name: string }) => x.name === 'pass-rates'))
  assert.deepEqual(pass.rows[0].slice(0, 5), ['Per trial (n = 178)', 'A. Theseus plain', '71.9%', '64.9%', '78.0%'])
})

test("the reader: every picture becomes its image, and every link goes somewhere", () => {
  for (const name of mds) {
    const md = readerMarkdown(read(`${name}.md`))
    assert.ok(!md.includes('<picture>'), name)
    assert.ok(!md.includes('<img'), name)
  }
  assert.match(readerMarkdown('a\n<picture>\n  <source media="x" srcset="img/r/f-dark.svg">\n  <img alt="The [x] chart." src="img/r/f.svg" width="720">\n</picture>\nb'), /a\n!\[The x chart\.\]\(img\/r\/f\.svg\)\nb/)
  assert.deepEqual(linkTo('2026-10-03-harbor-worth-spike.md'), { kind: 'report', name: '2026-10-03-harbor-worth-spike', hash: undefined })
  assert.deepEqual(linkTo('2026-10-04-terminal-bench-first-full-run.csv'), { kind: 'data', ext: 'csv', name: '2026-10-04-terminal-bench-first-full-run' })
  assert.deepEqual(linkTo('../../bench/README.md'), { kind: 'repo', path: 'bench/README.md' })
  assert.deepEqual(linkTo('#the-question'), { kind: 'anchor', hash: 'the-question' })
  assert.equal(linkTo('mailto:x').kind, 'web') // any scheme is the web's, opened in a new tab
})

test('the CSV reader: quotes, doubled quotes, commas and newlines inside a field', () => {
  assert.deepEqual(parseCsv('a,b,c\n1,"x, ""y""",\n"two\nlines",2,3\r\n'), [
    { a: '1', b: 'x, "y"', c: '' }, { a: 'two\nlines', b: '2', c: '3' },
  ])
})

const pt = (id: string, x: number, y: number): Point => ({
  id, set: 's', harness: 'theseus', run: id, runLabel: id, report: 'r', date: '2026-10-07', n: 10, tasks: 1, tags: [],
  values: { a: { v: x, n: 10, min: x, max: x }, b: { v: y, n: 10, min: y, max: y } },
})
const ax = (id: string, better: 'higher' | 'lower'): Property => ({ ...P('pass'), id, better })

test('the frontier: a point is dominated when another is as good on both axes and better on one, each its own way', () => {
  // Cost (lower) against pass rate (higher).
  const ps = [pt('cheap', 1, 0.5), pt('dear', 3, 0.9), pt('worse', 2, 0.4), pt('mid', 2, 0.7), pt('tie', 2, 0.7)]
  const placed = frontier(ps, ax('a', 'lower'), ax('b', 'higher'))
  const on = (id: string) => placed.find((p) => p.point.id === id)!
  assert.deepEqual(placed.filter((p) => p.frontier).map((p) => p.point.id), ['cheap', 'dear', 'mid', 'tie'])
  assert.equal(on('worse').dominatedBy?.id, 'cheap')
  assert.deepEqual(on('worse').dominators.map((p) => p.id), ['cheap', 'mid', 'tie']) // not 'dear': dearer, so no better on both
  // The line keeps to the better side: across at the old pass rate, then up.
  assert.deepEqual(staircase(placed, ax('a', 'lower'), ax('b', 'higher')), [[1, 0.5], [2, 0.5], [2, 0.7], [3, 0.7], [3, 0.9]])
  // Both lower (cost against time): the slower cheap point and the quicker dear one.
  const both = frontier([pt('p', 1, 9), pt('q', 5, 2), pt('r', 6, 9)], ax('a', 'lower'), ax('b', 'lower'))
  assert.deepEqual(both.map((p) => p.frontier), [true, true, false])
  assert.deepEqual(staircase(both, ax('a', 'lower'), ax('b', 'lower')), [[1, 9], [5, 9], [5, 2]])
  // Both higher: up the left, then across.
  const hi = frontier([pt('p', 1, 9), pt('q', 5, 2), pt('r', 0.5, 1)], ax('a', 'higher'), ax('b', 'higher'))
  assert.deepEqual(hi.map((p) => p.frontier), [true, true, false])
  assert.deepEqual(staircase(hi, ax('a', 'higher'), ax('b', 'higher')), [[1, 9], [1, 2], [5, 2]])
  // Higher x, lower y.
  const mixed = frontier([pt('p', 1, 1), pt('q', 5, 4), pt('r', 4, 6)], ax('a', 'higher'), ax('b', 'lower'))
  assert.deepEqual(mixed.map((p) => p.frontier), [true, true, false])
  assert.deepEqual(staircase(mixed, ax('a', 'higher'), ax('b', 'lower')), [[1, 1], [1, 4], [5, 4]])
  // A point missing either property is not placed.
  const missing = pt('m', 1, 1); delete (missing.values as Record<string, unknown>).b
  assert.equal(frontier([missing, pt('p', 1, 1)], ax('a', 'lower'), ax('b', 'lower')).length, 1)
})

test('the Wilson interval', () => {
  const w = wilson(128, 178)
  assert.ok(Math.abs(w.lo - 0.648985) < 1e-5 && Math.abs(w.hi - 0.77996) < 1e-5)
  assert.deepEqual(wilson(0, 0), { lo: 0, hi: 1 })
  assert.ok(wilson(1, 1).lo > 0.2 && wilson(1, 1).hi === 1)
})
