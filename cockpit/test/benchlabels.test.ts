// Theseus is named on the chart whether or not it is dominated, under every key it is measured as (theseus-hx03b),
// and `family()` answers as the manifest writer's does (`bench/report/manifest.py`).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { family, frontier, harness, property, showsName, theseusOf, type Point } from '../src/lib/bench.ts'

const DIR = new URL('../../docs/benchmarks/', import.meta.url)

const pt = (harness: string, x: number, y: number): Point => ({
  id: harness, set: 's', harness, run: 'r', runLabel: 'r', report: 'r', date: '2026-10-07', n: 10, tasks: 1, tags: [],
  values: { usd: { v: x, n: 10, min: x, max: x }, pass: { v: y, n: 10, min: y, max: y } },
})

test('family: Theseus and its variants are one arm, every other key its own', () => {
  for (const k of ['theseus', 'theseus-routed', 'theseus-rt', 'theseus-batching', 'theseus-68']) assert.equal(family(k), 'theseus', k)
  for (const k of ['claude-code', 'pi', 'theseusx', 'my-theseus', 'codex']) assert.equal(family(k), k, k)
})

test('a variant of Theseus is drawn as Theseus, in its slot, and keeps its own key', () => {
  const h = harness('theseus-routed')
  assert.equal(h.name, 'Theseus')
  assert.equal(h.slot, 0)
  assert.equal(h.key, 'theseus-routed')
  // The paragraph ablation is an arm the table lists, so it keeps its own name and slot.
  assert.deepEqual([harness('theseus-batching').name, harness('theseus-batching').slot], ['Theseus + paragraph', 2])
  assert.equal(harness('unlisted-arm').slot, null)
})

test('a dominated Theseus keeps its name, and a slotless rival is named too', () => {
  const placed = frontier([pt('theseus-routed', 3, 0.5), pt('claude-code', 1, 0.9), pt('pi', 0.5, 0.95), pt('aider', 9, 0.1)], property('usd')!, property('pass')!)
  const by = (k: string) => placed.find((p) => p.point.harness === k)!
  assert.equal(by('theseus-routed').frontier, false)
  assert.equal(showsName(by('theseus-routed')), true)
  assert.equal(showsName(by('aider')), true) // dominated, and slotless
  assert.equal(showsName(by('claude-code')), false) // dominated, in a slot: not named
  assert.equal(showsName(by('pi')), true) // on the frontier
  // Crowded, a slotless rival reads from the legend instead; the frontier's own points and Theseus's are always named.
  assert.equal(showsName(by('aider'), true), false)
  assert.equal(showsName(by('claude-code'), true), false)
  assert.equal(showsName(by('theseus-routed'), true), true)
  assert.equal(showsName(by('pi'), true), true)
})

test("the Verdict's Theseus is every placed point of the family", () => {
  const placed = frontier([pt('theseus-routed', 3, 0.5), pt('theseus', 4, 0.6), pt('claude-code', 1, 0.9)], property('usd')!, property('pass')!)
  assert.deepEqual(theseusOf(placed).map((p) => p.point.harness).sort(), ['theseus', 'theseus-routed'])
})

test('family agrees with manifest.py on every key the published summaries carry', () => {
  const keys = new Set(['theseus', 'theseus-routed', 'theseus-rt', 'theseus-68', 'theseus-batching', 'theseusx', 'claude-code', 'pi'])
  for (const f of readdirSync(DIR).filter((f) => f.endsWith('.json'))) {
    const d = JSON.parse(readFileSync(new URL(f, DIR), 'utf8')) as { summary?: unknown }
    if (d.summary && typeof d.summary === 'object') for (const k of Object.keys(d.summary)) keys.add(k)
  }
  assert.ok(keys.has('theseus-routed'))
  const list = [...keys]
  const py = spawnSync('python3', ['-I', '-c', 'import sys, json; sys.path.insert(0, sys.argv[1]); from manifest import family; print(json.dumps({k: family(k) for k in json.load(sys.stdin)}))',
    new URL('../../bench/report/', import.meta.url).pathname], { input: JSON.stringify(list), encoding: 'utf8' })
  assert.equal(py.status, 0, py.stderr)
  const oracle = JSON.parse(py.stdout) as Record<string, string>
  for (const k of list) assert.equal(family(k), oracle[k], k)
})
