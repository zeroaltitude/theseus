import { test } from 'node:test'
import assert from 'node:assert/strict'
import { importedWords, syncWords } from '../src/lib/sessionwords.ts'

test('an import and an erase are named beside the owner\'s sessions', () => {
  assert.equal(importedWords({ imported: { sessions: 21151, erased: 3 } }), '21,151 · erased 3')
  assert.equal(importedWords({ imported: { sessions: 21151, erased: 0 } }), '21,151')
  assert.equal(importedWords({ imported: { sessions: 0, erased: 300 } }), '0 · erased 300')
})

test('a store with no import says nothing', () => {
  assert.equal(importedWords({ imported: { sessions: 0, erased: 0 } }), null)
  // An older daemon's health has no such block.
  assert.equal(importedWords({} as never), null)
})

test('the OpenClaw sync names its last run, the next, and a failed night', () => {
  const sync = { at_ms: 1, sessions_new: 3, sessions_appended: 12, messages: 1480, rejected: 1, people_usd: 0.314, next_ms: 2 }
  const when = (ms: number): string => `t${ms}`
  assert.deepEqual(syncWords({ imported: { sessions: 5, erased: 0, sync } }, when), {
    line: 'last ran t1 · 3 new, 12 appended, 1,480 messages, 1 refused, people $0.31 · next t2',
    warn: null,
  })
  const failed = syncWords({ imported: { sessions: 5, erased: 0, sync: { ...sync, warn: 'the last sync failed: no python3' } } }, when)
  assert.equal(failed?.warn, 'the last sync failed: no python3')
  // Before a run, and for an older daemon, nothing.
  assert.equal(syncWords({ imported: { sessions: 5, erased: 0 } }, when), null)
  assert.equal(syncWords({} as never, when), null)
})
