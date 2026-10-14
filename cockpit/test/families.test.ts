// The tool families panel's words (`src/lib/families.ts`), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { DeferFamily, DeferListResult, DeferPlace } from '@protocol'
import { orderFamilies, placeWords, reachWords, usageWords } from '../src/lib/families.ts'

const fam = (family: string, loaded: boolean, over: Partial<DeferFamily> = {}): DeferFamily => ({
  family, summary: '', tools: [`${family}_one`], tokens: 0, loaded, why: '', used_turns: 0, searched: 0, missed: 0, unused_turns: 0, ...over,
})
const place: DeferPlace = {
  place: 'dm', turns: 25, tokens_all: 4000, tokens_sent: 900,
  families: [fam('aws', false, { missed: 3, searched: 2 }), fam('web', false), fam('files', true, { used_turns: 20 }), fam('memory', true, { unused_turns: 9 })],
}

test('loaded families come first, then the ones with misses, then by name', () => {
  assert.deepEqual(orderFamilies(place).map((f) => f.family), ['files', 'memory', 'aws', 'web'])
})

test('a family’s use in words, with its searches, misses and a loaded family nothing used', () => {
  assert.equal(usageWords(fam('files', true, { used_turns: 1 })), 'used in 1 turn')
  assert.equal(usageWords(fam('aws', false, { searched: 2, missed: 3 })), 'used in 0 turns · 2 searches · 3 missed')
  assert.equal(usageWords(fam('memory', true, { unused_turns: 9 })), 'used in 0 turns · loaded, never used')
})

test('a place’s summary counts the loaded families and the misses', () => {
  assert.equal(placeWords(place), '2 of 4 families loaded · tools ≈900 of 4000 tokens a request · 25 turns · 3 missed')
})

test('how the rest is reached follows the mechanism', () => {
  const d: DeferListResult = { enabled: true, mechanism: 'search', window_days: 14, hide_after_days: 14, load_share: 0.1, warmed: true, places: [] }
  assert.match(reachWords(d), /tool search/)
  assert.match(reachWords({ ...d, mechanism: 'find' }), /tools_find/)
  assert.match(reachWords({ ...d, enabled: false }), /every tool loads/)
})
