// The Extensions card's pure parts (`src/lib/extensions.ts`), run by `npm test` with node's own runner.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { handsWords, networkWords, proposalsNotLoaded, shortDigest } from '../src/lib/extensions.ts'

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

test('an extension\'s hands read as the CLI says them', () => {
  const h = { usd_cap: 2, backends: ['lambda'], max_group: 20, today_usd: 0.08, spent_usd: 1.2, running: 3 }
  assert.equal(handsWords(h), 'up to 20 hands at a time on lambda · $0.08 of $2.00 today · $1.20 in all · 3 running')
  assert.equal(handsWords({ ...h, max_group: 1, backends: ['lambda', 'fargate'] }).slice(0, 39), 'up to 1 hand at a time on lambda or far')
})
