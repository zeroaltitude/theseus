// The Graph's state treatment (theseus-g00vv): a suspended record is open, drawn paused (⏸) and not faded; a node the
// filters leave out wears a dashed outline, never opacity.
import test from 'node:test'
import assert from 'node:assert/strict'
import { opsLayout } from '../src/ops/graph.ts'
import { bucketGlyph, bucketOfNode, look } from '../src/ops/state.ts'
import { NOW, record } from './ops-fixtures.ts'

test('a suspended record is drawn open, with ⏸, not faded', () => {
  const g = opsLayout([], [record('tsk_s', { state: 'suspended', title: 'Resume the survey' })], [], null, { now: NOW })
  const n = g.nodes.find((x) => x.id === 'tsk_s')!
  const b = bucketOfNode(n, NOW)
  assert.equal(b, 'paused')
  assert.equal(bucketGlyph(b, n.state), '⏸')
  assert.equal(look(b, false).faded, false)
})

test('a filtered-out node gets the dashed treatment, not the opacity class', () => {
  const g = opsLayout([], [record('tsk_s', { state: 'suspended' }), record('tsk_d', { state: 'done', updated_at_ms: NOW - 60_000 })], [], null, { now: NOW })
  const open = look(bucketOfNode(g.nodes.find((x) => x.id === 'tsk_s')!, NOW), true)
  assert.deepEqual(open, { faded: false, filteredOut: true }, 'filtered out and open: outlined, not faded')
  assert.equal(look(bucketOfNode(g.nodes.find((x) => x.id === 'tsk_d')!, NOW), false).faded, true, 'ended: faded')
})
