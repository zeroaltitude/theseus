// The Benchmarks page opens on each task set's default axes (`DEFAULT_AXES`, src/lib/bench.ts). A task set without
// them threw a blank-page TypeError on `DEFAULT_AXES[set.id][0]` (`/benchmarks?set=arms`, theseus-ldmwj). Every task
// set, on its real published run, must have both axes, each a real property its points measured on at least two points.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { axes, DEFAULT_AXES, points, property, SET_IDS, TASK_SETS } from '../src/lib/bench.ts'

const DIR = new URL('../../docs/benchmarks/', import.meta.url)
const read = (f: string) => readFileSync(new URL(f, DIR), 'utf8')

for (const set of TASK_SETS) {
  test(`task set ${set.id} opens on two default axes it measured`, () => {
    const pair = DEFAULT_AXES[set.id]
    assert.ok(pair, `DEFAULT_AXES has no entry for the task set "${set.id}"`)
    assert.equal(pair.length, 2)
    const [x, y] = pair
    assert.notEqual(x, y, 'the two axes must differ')
    const ps = points(set, read(`${set.report}.csv`), read(`${set.report}.json`), read(`${set.report}.md`), set.report.slice(0, 10))
    assert.ok(ps.length > 0, `${set.id} has no points`)
    const usable = axes(ps).usable.map((p) => p.id)
    for (const id of [x, y]) {
      assert.ok(property(id), `"${id}" is not a property`)
      assert.ok(usable.includes(id), `"${id}" is not measured by two or more points of ${set.id} (usable: ${usable.join(', ')})`)
    }
  })
}

test('DEFAULT_AXES names no task set that does not exist', () => {
  assert.deepEqual(Object.keys(DEFAULT_AXES).sort(), TASK_SETS.map((s) => s.id).sort())
})

test('SET_IDS (the type behind DEFAULT_AXES) lists exactly the task sets', () => {
  assert.deepEqual([...SET_IDS].sort(), TASK_SETS.map((s) => s.id).sort())
})
