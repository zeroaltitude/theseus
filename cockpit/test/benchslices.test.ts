// The frontier's slices (theseus-s5qwv), over the real `docs/benchmarks/`: every arm the project measured on one task
// set, gathered from the manifest instead of from a single report's table. Before this the page offered seven chips and
// its default drew four arms, while `index.json` held eighteen rows on that one task set.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { TASK_SETS, axes, frontier, readIndex, runs, setsFrom, unequal } from '../src/lib/bench.ts'
import { readManifest } from '../src/lib/benchindex.ts'
import { frontierSets, isSlice, setReports, slicePoints, undrawn, type Slice } from '../src/lib/benchslices.ts'

const DIR = new URL('../../docs/benchmarks/', import.meta.url)
const read = (f: string) => readFileSync(new URL(f, DIR), 'utf8')
const names = readdirSync(DIR)
const manifest = readManifest(read('index.json'))

const stems = (ext: string) => names.filter((f) => f.endsWith(ext)).map((f) => f.slice(0, -ext.length))
const load = (ext: string) => new Map(stems(ext).map((n) => [n, read(`${n}${ext}`)]))
const files = { csv: load('.csv'), json: load('.json'), md: load('.md') }

const datasets = stems('.json').map((report) => {
  try { return { report, ui: (JSON.parse(files.json.get(report)!) as { ui?: unknown }).ui } } catch { return { report } }
})
const generated = setsFrom(datasets)
const sets = frontierSets(manifest.rows, generated, TASK_SETS)
const list = runs(stems('.md').filter((n) => n !== 'README'), readIndex(read('README.md')))
const slices = sets.filter(isSlice)
const open = sets[0] as Slice

test('the manifest makes a slice for every task set it measured, and the five built-ins still stand', () => {
  assert.equal(manifest.reason, null)
  // Seven task sets in the real manifest; the page offered two generated sets and five built-ins before this.
  assert.equal(slices.length, 7)
  for (const b of TASK_SETS) assert.ok(sets.some((s) => s.id === b.id), `the built-in ${b.id} is still offered`)
  // Every id is distinct and address-safe: the set lives in `?set=`.
  const ids = sets.map((s) => s.id)
  assert.equal(new Set(ids).size, ids.length)
  for (const id of ids) assert.match(id, /^[a-z0-9:-]+$/)
})

test('the page opens on the newest, most-measured slice: Terminal-Bench 2.0, high, all 89 tasks', () => {
  assert.ok(isSlice(open))
  assert.equal(open.benchmark, 'terminal-bench@2.0')
  assert.equal(open.tier, 'high')
  assert.equal(open.taskCount, 89)
  assert.equal(new Set(open.members.map((m) => m.row.arm)).size, 8)
})

test('a report that cites an earlier run’s figures is the same point cited again, not a second dot', () => {
  // Nineteen rows on this task set, eleven of them measurements: Claude Code's 0.752809 is one run, re-cited twice.
  const rows = manifest.rows.filter((r) => r.benchmark === 'terminal-bench@2.0' && r.tier === 'high' && r.tasks === 89)
  assert.equal(rows.length, 19)
  assert.equal(open.members.length, 11)
  const cc = open.members.filter((m) => m.row.arm === 'claude-code')
  assert.equal(cc.length, 1)
  assert.equal(cc[0].row.date, '2026-10-18', 'the member is kept at the run that measured it')
  assert.deepEqual(cc[0].alsoIn.sort(), ['2026-10-21-terminal-bench-rerun-high', '2026-10-25-terminal-bench-codex-high'])
  // No measurement is dropped: every row is either a member or cited by one.
  const held = new Set(open.members.flatMap((m) => [m.row.report, ...m.alsoIn]))
  for (const r of rows) assert.ok(held.has(r.report), `${r.report} is accounted for`)
})

test('every member draws exactly one point, and the default slice draws eleven over eight harnesses', () => {
  const ps = slicePoints(open, files)
  assert.equal(ps.length, 11)
  assert.equal(new Set(ps.map((p) => p.harness)).size, 8)
  assert.equal(new Set(ps.map((p) => p.id)).size, 11, 'each point has its own id')
  // Each point's pass rate is the one the manifest states for its member, whether it was folded or stated.
  for (const m of open.members) {
    const p = ps.find((x) => x.report === m.row.report && x.harness === m.row.arm)
    assert.ok(p, `${m.row.report}:${m.row.arm} drew a point`)
    assert.ok(Math.abs(p.values.pass.v - m.row.pass.value) < 1e-6, `${m.row.variant} folds to the figure the manifest states`)
  }
})

test('a point whose table cannot name its arm is still drawn, from the manifest’s own figures', () => {
  const ps = slicePoints(open, files)
  // 2026-10-21's table holds one arm, spelled `theseus-routed`, while the manifest names two theseus variants in that
  // report: the table cannot be split by name, so the rule that resolves it is the one measured arm of that report.
  const t68 = ps.find((p) => p.run.includes('theseus-68'))
  assert.ok(t68, 'theseus-68 is drawn')
  assert.ok(Math.abs(t68.values.pass.v - 0.771536) < 1e-5)
  // The four newest single-arm runs have no `arm` column at all; their arm is the manifest's.
  for (const r of ['2026-10-26-terminal-bench-aider-high-k1', '2026-10-27-terminal-bench-openhands-high-k1']) {
    assert.ok(!files.csv.get(r)!.split('\n')[0].split(',').includes('arm'), `${r} names no arm in its table`)
    assert.ok(ps.some((p) => p.report === r), `${r} is drawn anyway`)
  }
})

test('nothing is silently mixed: a member that did not share the slice’s conditions is marked and says why', () => {
  const ps = slicePoints(open, files)
  const codex = ps.find((p) => p.harness === 'codex')!
  assert.ok(unequal(codex), 'Codex CLI ran another model and is marked')
  assert.ok(codex.tags.some((t) => /GPT-6-Astra/i.test(t.text)))
  // An older run keeps its date in its own words, so a reader never reads it as part of today's measurement.
  // Every point says when it was measured, the newest included, so no two runs are read as one.
  for (const p of ps) assert.ok(p.tags.some((t) => /^measured \d{4}-\d{2}-\d{2}/.test(t.text)), `${p.harness} names its date`)
  const old = ps.find((p) => p.report === '2026-10-18-terminal-bench-v1-high')!
  assert.ok(old.tags.some((t) => t.text.includes("before this set's newest run of 2026-10-27")))
  // The mark is kept for a condition that differed, so it still means something: age alone does not earn it.
  const plain = ps.filter((p) => !unequal(p))
  assert.ok(plain.length > 0 && plain.length < ps.length, 'the mark tells points apart')
  assert.ok(plain.some((p) => p.date < '2026-10-27'), 'an older run that shared the conditions is not marked')
})

test('a point is named with its harness and the date of the run that measured it, and no more', () => {
  const ps = slicePoints(open, files)
  // The manifest's `harness` line runs to sixty characters ("Theseus as shipped (routed), frozen 1.0.3 …"): it is the
  // tip's and the table's, never a label drawn beside a dot.
  // A bare date, or `date, qualifier` where one slice holds two of an arm's runs on that day. Never the manifest's
  // `harness` line. And whatever the form, no two points of one harness may carry the same name: a label that does
  // not tell two dots apart is the defect theseus-1thr3 was filed for.
  for (const p of ps) assert.match(p.runLabel, /^\d{4}-\d{2}-\d{2}(, \S.*)?$/, p.harness)
  for (const h of new Set(ps.map((p) => p.harness))) {
    const mine = ps.filter((p) => p.harness === h)
    assert.equal(new Set(mine.map((p) => p.runLabel)).size, mine.length, `${h} names each of its runs distinctly`)
  }
  assert.equal(new Set(ps.filter((p) => p.harness === 'theseus').map((p) => p.runLabel)).size, 3)
  // OpenClaw ran twice on 2026-10-27, on two models, under one variant: the model is what tells them apart.
  const oc = ps.filter((p) => p.harness === 'openclaw' && p.date === '2026-10-27').map((p) => p.runLabel).sort()
  assert.deepEqual(oc, ['2026-10-27, claude-sonnet-5-5', '2026-10-27, gpt-6-astra'])
  // Where one slice holds two runs of an arm on one day, the variant tells them apart rather than nothing doing it.
  const same = slices.find((s) => s.id === 'terminal-bench-untiered-14')!
  const two = slicePoints(same, files).filter((p) => p.harness === 'theseus' && p.date.startsWith('2026-10-10'))
  assert.equal(two.length, 2)
  assert.equal(new Set(two.map((p) => p.runLabel)).size, 2, 'two runs of one arm on one day are told apart')
  for (const p of two) assert.match(p.runLabel, /^2026-10-10, theseus-/)
})

test('every slice is drawable on axes its own points measured, not only on the chart’s default pair', () => {
  for (const s of slices) {
    const ps = slicePoints(s, files)
    assert.equal(ps.length, s.members.length, `${s.id} draws one point a member`)
    assert.ok(ps.length >= 2, `${s.id} draws a comparison`)
    const { usable } = axes(ps)
    assert.ok(usable.length >= 2, `${s.id} measured two properties`)
    // The 10-13 runs and Harbor priced no trial, so `usd` — the default x — is not theirs to open on.
    const px = usable.find((u) => u.id === 'usd') ?? usable[0]
    const py = usable.find((u) => u !== px)!
    assert.ok(frontier(ps, px, py).some((p) => p.frontier), `${s.id} has a frontier on ${px.id} x ${py.id}`)
  }
  assert.deepEqual(
    slices.filter((s) => !axes(slicePoints(s, files)).usable.some((u) => u.id === 'usd')).map((s) => s.id).sort(),
    ['harbor-untiered-1', 'terminal-bench-2-0-untiered-89'],
    'and those two are the only sets that priced no trial',
  )
})

test('every benchmark the page published is in the pulldown: drawn, or named with why it is not', () => {
  const missing = undrawn(sets, list, manifest.skipped)
  const named = new Set(missing.flatMap((m) => m.runs))
  const drawn = new Set(sets.flatMap(setReports))
  for (const r of list) assert.ok(drawn.has(r.name) || named.has(r.name), `${r.name} is drawn or named`)
  for (const m of missing) assert.ok(m.why, `${m.suite} says why`)
  // Bug Hunt RS scores bugs fixed a trial, not a pass rate: the manifest's own words, not a guess.
  const bh = missing.find((m) => m.suite === 'bughunt-rs')!
  assert.match(bh.why, /no `metrics.pass_rate`/)
  for (const s of ['gate-bench', 'memory-exam', 'retrieval', 'context']) {
    assert.ok(missing.some((m) => m.suite === s), `${s} is named`)
  }
})

test('a slice absorbs the hand-written words of a `ui` block it spans, and that set is not offered twice', () => {
  // The two `ui` blocks in the real datasets are the 10-18 v1 high and max runs, each inside a slice.
  assert.deepEqual(generated.map((g) => g.id).sort(), ['tb-v1-high', 'tb-v1-max'])
  for (const g of generated) {
    assert.ok(!sets.some((s) => s.id === g.id), `${g.id} is not offered beside the slice that holds it`)
    assert.ok(g.axes, `${g.id} names its own axes`)
    const holder = slices.find((s) => s.aliases.includes(g.id))
    assert.ok(holder, `${g.id} is held by a slice`)
    assert.ok(holder.members.some((m) => m.row.report === g.report), 'the slice spans its report')
    assert.equal(holder.about, g.about, 'its words are kept')
    assert.deepEqual(holder.conditions, g.conditions)
  }
})
