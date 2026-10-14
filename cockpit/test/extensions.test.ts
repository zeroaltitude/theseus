// The Extensions card's pure parts (`src/lib/extensions.ts`), run by `npm test` with node's own runner.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { networkWords, proposalsNotLoaded, shortDigest, usageWords } from '../src/lib/extensions.ts'

test('a digest is named by its first six characters, and a network in the CLI words', () => {
  assert.equal(shortDigest('3f2a1c9e0b'), '3f2a1c')
  assert.equal(networkWords([]), 'no network')
  assert.equal(networkWords(['api.wordlist.invalid:443']), 'network to api.wordlist.invalid:443')
})

test('the proposals shown are every one but the version that runs', () => {
  const e = (name: string, digest: string, state: string) => ({ name, digest, state }) as any
  const list = {
    extensions: [e('wc', 'd2', 'acked'), e('wc', 'd1', 'replaced'), e('other', 'd9', 'proposed')],
    loaded: [{ name: 'wc', digest: 'd2' } as any],
  }
  assert.deepEqual(proposalsNotLoaded(list).map((p) => p.digest), ['d1', 'd9'])
  assert.equal(proposalsNotLoaded({ extensions: [e('wc', 'd1', 'proposed')] } as any).length, 1)
})

test('an extension\'s use reads as the CLI reads it', () => {
  const u = { window_days: 14, calls: 34, failures: 0, p50_ms: 41, p95_ms: 90 }
  assert.equal(usageWords(u), 'used 34 times in 14 days, 0 failures, p50 41 ms, p95 90 ms')
  assert.equal(usageWords({ ...u, calls: 1, failures: 1, truncated: true }), 'used 1 time in 14 days, 1 failure, p50 41 ms, p95 90 ms (at least)')
})
