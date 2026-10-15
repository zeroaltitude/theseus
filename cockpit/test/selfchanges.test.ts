// The self card's pure parts (`src/lib/selfchanges.ts`), run by `npm test` with node's own runner.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { kindWords, numbersLine, revertableMerge, revertedAlready, stateWords, switchLine } from '../src/lib/selfchanges.ts'

const state = (over: Record<string, unknown>) => ({ mode: 'act', halted: true, gate: 'halted', ...over }) as any

test('the switch reads off, halted or running, and says who moved it', () => {
  assert.deepEqual(stateWords(state({ mode: 'off' })), { label: 'off', tone: 'idle' })
  assert.deepEqual(stateWords(state({ never_resumed: true })), { label: 'halted (never resumed)', tone: 'wait' })
  assert.deepEqual(stateWords(state({ halted: false })), { label: 'running', tone: 'ok' })
  assert.equal(switchLine(state({ never_resumed: true })), "mode act; halted until the owner's first resume")
  assert.equal(switchLine(state({ by: 'the CLI', why: 'a look first' })), 'mode act; halted by the CLI: a look first')
  assert.equal(switchLine(state({ mode: 'off', halted: false, by: 'the CLI' })), 'mode off: nothing self-directed runs; released by the CLI')
})

test('a row reads its numbers in a line and its self kind without the prefix', () => {
  assert.equal(numbersLine(null), '')
  assert.equal(numbersLine({ cost_usd: 1.25, fixed: 4 }), 'cost_usd 1.25 · fixed 4')
  assert.equal(kindWords({ kind: 'self.joined' }), 'joined')
  assert.equal(kindWords({ kind: 'pack.mode' }), 'pack.mode')
})

const MERGE = '1a2b3c4d5e6f7a8b9c0d1a2b3c4d5e6f7a8b9c0d'

test('a self-join row is vetoable by its merge, and only until a later row says it was undone', () => {
  assert.equal(revertableMerge({ kind: 'self.joined', undo: `theseus self revert ${MERGE}` }), MERGE)
  assert.equal(revertableMerge({ kind: 'self.installed', undo: `theseus self revert ${MERGE}` }), undefined)
  assert.equal(revertableMerge({ kind: 'self.joined', undo: 'git revert -m 1 abc' }), undefined)
  assert.equal(revertableMerge({ kind: 'self.joined' }), undefined)
  const done = { kind: 'self.vetoed', what: `self-join ${MERGE.slice(0, 12)} vetoed and reverted as ffffffffffff` }
  assert.equal(revertedAlready([done], MERGE), true)
  assert.equal(revertedAlready([{ kind: 'self.joined', what: `joined as ${MERGE.slice(0, 12)}` }], MERGE), false)
  assert.equal(revertedAlready([], MERGE), false)
})
