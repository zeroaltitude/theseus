// The AWS card's durability row (`src/lib/durability.ts`), run by `npm test`: each state's tone and words, and the
// error shown when there is one (theseus-9ai1). Invented account and deployment.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { AwsDurabilityStatus } from '@protocol'
import { durabilityView } from '../src/lib/durability.ts'

const NOW = 1_800_000_000_000
const d = (state: string, error?: string): AwsDurabilityStatus => ({
  state,
  bucket: 'theseus-111122223333-us-west-2',
  prefix: 'durability/theseus-lab/',
  table: 'theseus-durability',
  lag_ms: 0,
  shipped_to_position: 1234,
  last_shipped_unix_ms: NOW - 3 * 60_000,
  segments: 2, tails: 1, blobs: 0, rows: 1500, bytes: 2_097_152,
  ...(error === undefined ? {} : { error }),
})

test('caught up is good, and says what it shipped, when, and where', () => {
  const v = durabilityView(d('caught_up'), NOW)
  assert.equal(v.tone, 'ok')
  assert.equal(v.label, 'caught up')
  assert.equal(v.error, undefined)
  assert.equal(v.shipped, 'to position 1,234, 3 min ago')
  assert.equal(v.lag, 'nothing unshipped')
  assert.equal(v.counts, '2 segments, 1 tail, 0 blobs, 1,500 rows, 2,097,152 bytes')
  assert.equal(v.where, 's3://theseus-111122223333-us-west-2/durability/theseus-lab/ and the table theseus-durability')
})

test('waiting and shipping are neutral; a waiting tender says why, and its lag', () => {
  const w = d('waiting', 'for the WAL\'s sync: position 1240 is written, not yet synced')
  w.oldest_unshipped_unix_ms = NOW - 150_000
  w.lag_ms = 150_000
  const v = durabilityView(w, NOW)
  assert.equal(v.tone, 'idle')
  assert.equal(v.error, 'for the WAL\'s sync: position 1240 is written, not yet synced')
  assert.equal(v.lag, '2 min: the oldest record not yet shipped was written 2 min ago')
  assert.equal(durabilityView(d('shipping'), NOW).tone, 'idle')
  const fresh = d('waiting', 'for the start to settle')
  fresh.shipped_to_position = 0
  delete fresh.last_shipped_unix_ms
  assert.equal(durabilityView(fresh, NOW).shipped, 'nothing shipped yet')
})

test('failing is a warning and stopped is bad, each with its error', () => {
  const f = durabilityView(d('failing', 's3 PutObject: AccessDenied'), NOW)
  assert.equal(f.tone, 'wait')
  assert.equal(f.error, 's3 PutObject: AccessDenied')
  assert.match(f.label, /failing/)
  const s = durabilityView(d('stopped', 'the cursor is past the WAL\'s end'), NOW)
  assert.equal(s.tone, 'fault')
  assert.equal(s.error, 'the cursor is past the WAL\'s end')
  assert.match(s.label, /stopped/)
})

test('a state this build does not know is said as it is, and neutral', () => {
  const v = durabilityView(d('rebalancing'), NOW)
  assert.equal(v.label, 'rebalancing')
  assert.equal(v.tone, 'idle')
})
