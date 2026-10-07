import { test } from 'node:test'
import assert from 'node:assert/strict'
import { importedWords } from '../src/lib/sessionwords.ts'

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
