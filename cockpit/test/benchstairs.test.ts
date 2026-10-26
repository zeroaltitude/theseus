// The frontier's line under every pair of better directions (theseus-zhzif): the vertex lists were pinned on the
// pre-refactor `staircase` and must not move; the dashed legs are new.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { axisName, frontier, inverted, PROPERTIES, property, staircase, type Point, type Property } from '../src/lib/bench.ts'

const pt = (id: string, x: number, y: number): Point => ({
  id, set: 's', harness: 'pi', run: id, runLabel: id, report: 'r', date: '2026-10-07', n: 10, tasks: 1, tags: [],
  values: { a: { v: x, n: 10, min: x, max: x }, b: { v: y, n: 10, min: y, max: y } },
})
const ax = (id: string, better: 'higher' | 'lower'): Property => ({ ...property('pass')!, id, better })
type V = [number, number][]
const line = (ps: Point[], bx: 'higher' | 'lower', by: 'higher' | 'lower'): V => {
  const px = ax('a', bx), py = ax('b', by)
  return staircase(frontier(ps, px, py), px, py).line
}

// Three frontier points per direction pair, and a dominated one.
test('the line is the same under all four better-direction pairs (pinned before the refactor)', () => {
  assert.deepEqual(line([pt('p', 1, 1), pt('q', 2, 4), pt('r', 5, 9), pt('d', 6, 0.5)], 'lower', 'higher'), [[1, 1], [2, 1], [2, 4], [5, 4], [5, 9]])
  assert.deepEqual(line([pt('p', 1, 9), pt('q', 2, 4), pt('r', 5, 1), pt('d', 6, 9.5)], 'lower', 'lower'), [[1, 9], [2, 9], [2, 4], [5, 4], [5, 1]])
  assert.deepEqual(line([pt('p', 1, 9), pt('q', 3, 4), pt('r', 5, 1), pt('d', 0.5, 0.5)], 'higher', 'higher'), [[1, 9], [1, 4], [3, 4], [3, 1], [5, 1]])
  assert.deepEqual(line([pt('p', 1, 1), pt('q', 3, 4), pt('r', 5, 6), pt('d', 0.5, 9)], 'higher', 'lower'), [[1, 1], [1, 4], [3, 4], [3, 6], [5, 6]])
})

test('one point is a single vertex; a tie is one vertex', () => {
  assert.deepEqual(line([pt('p', 2, 3)], 'lower', 'higher'), [[2, 3]])
  assert.deepEqual(line([pt('p', 1, 5), pt('t', 3, 7), pt('u', 3, 7)], 'lower', 'higher'), [[1, 5], [3, 5], [3, 7]])
})

test('the legs run from the first point along x, and from the last along y, to the worse ends of the extent', () => {
  const ext = { x: [0, 10] as [number, number], y: [0, 20] as [number, number] }
  const legs = (ps: Point[], bx: 'higher' | 'lower', by: 'higher' | 'lower') => {
    const px = ax('a', bx), py = ax('b', by)
    return staircase(frontier(ps, px, py), px, py, ext).legs
  }
  // Lower x worse = 10, higher y worse = 0.
  assert.deepEqual(legs([pt('p', 1, 1), pt('r', 5, 9)], 'lower', 'higher'), [[[1, 1], [10, 1]], [[5, 9], [5, 0]]])
  // Both lower: worse x = 10, worse y = 20.
  assert.deepEqual(legs([pt('p', 1, 9), pt('r', 5, 1)], 'lower', 'lower'), [[[1, 9], [10, 9]], [[5, 1], [5, 20]]])
  // Both higher: worse x = 0, worse y = 0.
  assert.deepEqual(legs([pt('p', 1, 9), pt('r', 5, 1)], 'higher', 'higher'), [[[1, 9], [0, 9]], [[5, 1], [5, 0]]])
  // Higher x, lower y: worse x = 0, worse y = 20.
  assert.deepEqual(legs([pt('p', 1, 1), pt('r', 5, 6)], 'higher', 'lower'), [[[1, 1], [0, 1]], [[5, 6], [5, 20]]])
  // One point: no segments, both legs.
  assert.deepEqual(legs([pt('p', 2, 3)], 'lower', 'higher'), [[[2, 3], [10, 3]], [[2, 3], [2, 0]]])
  // No extent, no legs.
  const px = ax('a', 'lower'), py = ax('b', 'higher')
  assert.deepEqual(staircase(frontier([pt('p', 2, 3)], px, py), px, py).legs, [])
})

test('on every property the axis name ends better → (x) or better ↑ (y), and a lower-is-better axis runs backwards', () => {
  for (const p of PROPERTIES) {
    assert.match(axisName(p, 'x', false), /better →$/, p.id)
    assert.match(axisName(p, 'y', true), /\(log scale\)   better ↑$/, p.id)
    assert.equal(inverted(p), p.better === 'lower', p.id)
  }
  assert.equal(PROPERTIES.filter((p) => !inverted(p)).map((p) => p.id).join(), 'pass')
})
