// The floor contract (theseus-jid7t.6): the menu's greying agrees with the daemon's refusal table, row for row.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { greyOf } from '../src/ops/floor.ts'

interface Row { mark: 'open' | 'private' | 'sensitive'; origin: string; known: boolean; refused: boolean }
const table = JSON.parse(readFileSync(new URL('./fixtures/profile_floor.json', import.meta.url), 'utf8')) as { rows: Row[]; vouched_uncatalogued: Row[] }

test('greyOf agrees with every row of the floor’s table', () => {
  assert.equal(table.rows.length, 24)
  for (const r of table.rows) {
    const got = greyOf(r.mark, 'p', r.known ? { name: 'p', origin: r.origin, cleared: r.origin === 'us' } : undefined)
    assert.equal(got !== null, r.refused, `${r.mark} ${r.origin} known=${r.known}: ${got}`)
  }
})

test('a US profile the daemon does not clear (a vouch, an uncatalogued model) is grey on private and sensitive, never on open', () => {
  assert.equal(table.vouched_uncatalogued.length, 3)
  for (const r of table.vouched_uncatalogued) {
    const got = greyOf(r.mark, 'p', { name: 'p', origin: r.origin, cleared: false })
    assert.equal(got !== null, r.refused, `${r.mark}: ${got}`)
  }
  assert.equal(greyOf('private', 'x', { name: 'x', origin: 'us', cleared: false }), 'this conversation holds private content, and `x` is not cleared for it: the catalog does not list its model as of US origin')
  assert.equal(greyOf('sensitive', 'x', { name: 'x', origin: 'us' }), 'this conversation holds sensitive content, and `x` is not cleared for it: the catalog does not list its model as of US origin', 'no answer: fail closed')
  assert.equal(greyOf('open', 'x', { name: 'x', origin: 'us', cleared: false }), null)
})

test('the reasons are the daemon’s words, and an origin the daemon did not tell is not US', () => {
  assert.equal(greyOf('sensitive', 'kimi', { name: 'kimi', origin: 'cn' }), 'this conversation holds sensitive content, and `kimi` is not a model of US origin')
  assert.equal(greyOf('private', 'x', { name: 'x' }), 'this conversation holds private content, and `x` is not a model of US origin')
  assert.equal(greyOf(undefined, 'x', { name: 'x', origin: 'cn' }), null, 'no mark: open')
  assert.equal(greyOf('open', 'gone', undefined), '`gone` is not a configured profile')
})
