// Theseus is named on the chart whether or not it is dominated, under every key it is measured as (theseus-hx03b),
// and `family()` answers as the manifest writer's does (`bench/report/manifest.py`).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync, readdirSync } from 'node:fs'
import { spawnSync } from 'node:child_process'
import { armsOf, family, frontier, HARNESSES, harness, property, sameArm, showsName, theseusOf, type Point } from '../src/lib/bench.ts'

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

test("the Verdict's Theseus is every placed point of the family but a listed ablation", () => {
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

// theseus-4zea0: an arm `HARNESSES` lists (the paragraph ablation) is not Theseus, in the Verdict or the legend.
const both = () => frontier([pt('theseus', 4, 0.6), pt('theseus-batching', 3, 0.65), pt('claude-code', 1, 0.9)], property('usd')!, property('pass')!)

test("the Verdict's Theseus is not a listed ablation", () => {
  assert.deepEqual(theseusOf(both()).map((p) => p.point.harness), ['theseus'])
})

test('the legend has a key for the paragraph ablation, beside Theseus', () => {
  assert.deepEqual(armsOf(both()), ['theseus', 'theseus-batching', 'claude-code'])
})

test('one key is one arm: the colour, the legend and the Verdict class it alike', () => {
  const keys = ['theseus', 'theseus-routed', 'theseus-rt', 'theseus-batching', 'claude-code', 'pi', 'unlisted-arm']
  const placed = frontier(keys.map((k, i) => pt(k, i + 1, 0.9 - i / 20)), property('usd')!, property('pass')!)
  const arms = armsOf(placed)
  for (const p of placed) {
    const k = p.point.harness, arm = sameArm(k)
    assert.ok(arms.includes(arm), k)
    if (HARNESSES.some((h) => h.key === k)) assert.equal(arm, k, k) // a key the table lists is its own arm
    assert.equal(harness(arm).slot, harness(k).slot, k) // the legend's swatch is the mark's colour
    assert.equal(harness(arm).name, harness(k).name, k)
    assert.equal(theseusOf([p]).length === 1, arm === 'theseus', k)
  }
  assert.equal(new Set(arms.map((a) => harness(a).slot).filter((s) => s !== null)).size, arms.filter((a) => harness(a).slot !== null).length)
})

// theseus-4zea0 join fix: `showsName` is the ONE place deliberately left on `family`, and this is what that buys.
// It is a labelling policy, not a classification — the name drawn is `harness(key).name`, which honours the listing —
// so the ablation keeps a name in a crowd, and the name it keeps is its own. If someone routes `showsName` through
// `sameArm`, the first assertion fails and they are made to read the doc comment that says why.
test('a listed ablation keeps its name in a crowd, and the name it keeps is its own', () => {
  // `pi` is cheapest and best, so it takes the frontier and leaves the ablation and a slotted rival both dominated.
  const placed = frontier([pt('theseus', 4, 0.6), pt('theseus-batching', 3, 0.65), pt('claude-code', 2, 0.7), pt('pi', 0.5, 0.95)],
    property('usd')!, property('pass')!)
  const by = (k: string) => placed.find((p) => p.point.harness === k)!
  assert.equal(by('theseus-batching').frontier, false)
  assert.equal(by('claude-code').frontier, false)
  assert.equal(showsName(by('theseus-batching'), true), true) // crowded, slot 2, dominated: still named
  assert.equal(showsName(by('claude-code'), true), false) // a slotted rival in a crowd reads from the legend
  assert.equal(harness('theseus-batching').name, 'Theseus + paragraph') // never the bare "Theseus"
  assert.notEqual(harness('theseus-batching').name, harness('theseus').name)
})
