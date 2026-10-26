// Models against harnesses (theseus-ycp9h): framing derived from the real datasets, the legend's blocks, and the
// planned arms that never get a value.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { points, setsFrom, TASK_SETS, type Point } from '../src/lib/bench.ts'
import { byFraming, framingOf, framingOfRow, modelLabel, plannedArms } from '../src/lib/benchframing.ts'
import { readManifest } from '../src/lib/benchindex.ts'
import { cardsOf } from '../src/lib/benchcards.ts'

const read = (f: string) => readFileSync(new URL(`../../docs/benchmarks/${f}`, import.meta.url), 'utf8')
const gen = (report: string, ui?: unknown) => setsFrom([{ report, ui: ui ?? JSON.parse(read(`${report}.json`)).ui }])[0]
const of = (report: string, set = gen(report)) => points(set, read(`${report}.csv`), read(`${report}.json`), read(`${report}.md`), report.slice(0, 10))
const frames = (ps: Point[]) => Object.fromEntries(ps.map((p) => [p.harness, framingOf(p)]))

test('the v1 sets: the rivals are framing A and routed Theseus is T', () => {
  for (const r of ['2026-10-18-terminal-bench-v1-high', '2026-10-18-terminal-bench-v1-max']) {
    const f = frames(of(r))
    assert.equal(f['theseus-routed'], 'T')
    for (const k of ['claude-code', 'opencode', 'pi']) assert.equal(f[k], 'A', k)
  }
})

test('the 10-25 codex report: codex is B, with its model in its label', () => {
  const ui = { set_id: 'codex-high', label: 'x', short: 'x', about: 'x' }
  const ps = of('2026-10-25-terminal-bench-codex-high', gen('2026-10-25-terminal-bench-codex-high', ui))
  const codex = ps.find((p) => p.harness === 'codex')!
  assert.equal(framingOf(codex), 'B')
  assert.equal(modelLabel(codex), 'GPT-6-Astra')
})

test('the harness-arms set: codex is B (a different model), the Claude-model harnesses A', () => {
  const set = TASK_SETS.find((s) => s.id === 'arms')!
  const ps = points(set, read(`${set.report}.csv`), read(`${set.report}.json`), read(`${set.report}.md`), set.report.slice(0, 10))
  const f = frames(ps)
  assert.equal(f.codex, 'B')
  for (const k of ['claude-code', 'opencode', 'pi', 'theseus']) assert.equal(f[k], 'A', k)
})

test('the legend’s blocks are non-empty only when they hold arms, controlled comparison first', () => {
  assert.deepEqual(byFraming(['a', 'b'], () => 'A').map((b) => b.framing), ['A'])
  const mixed = byFraming(['a', 'b', 'c'], (k) => (k === 'a' ? 'B' : k === 'b' ? 'T' : 'A'))
  assert.deepEqual(mixed.map((b) => b.framing), ['A', 'T', 'B'])
  assert.deepEqual(byFraming([], () => 'A'), [])
})

test('a manifest row is framed the same way, and a planned arm never gets a value', () => {
  const m = readManifest(read('index.json'))
  const f = (arm: string) => framingOfRow(m.series.find((s) => s.benchmark === 'terminal-bench@2.0' && s.tier === 'high' && s.arm === arm)!.newest)
  assert.equal(f('codex'), 'B')
  assert.equal(f('theseus'), 'T')
  assert.equal(f('pi'), 'A')
  // The harness-arms night run names Codex's own model in its model line.
  const night = m.series.find((x) => x.benchmark === 'terminal-bench' && x.arm === 'codex')!
  assert.equal(framingOfRow(night.newest), 'B')
  const planned = plannedArms(JSON.stringify({ version: 1, arms: [
    { key: 'fx', name: 'Theseus fixed-Invented', status: 'planned' }, { key: 'pi', name: 'Pi', status: 'measured' }, { key: 'z', status: 'planned' }] }))
  assert.deepEqual(planned, ['Theseus fixed-Invented'])
  assert.deepEqual(plannedArms('{'), [])
  assert.deepEqual(plannedArms(null), [])
  assert.deepEqual(plannedArms(JSON.stringify({ version: 2, arms: [] })), [])
  // No card line is ever made for an arm the manifest does not measure.
  const names = cardsOf(m, [], []).flatMap((c) => c.lines.map((l) => l.name)).join(' ')
  assert.ok(!names.includes('Invented'))
})
