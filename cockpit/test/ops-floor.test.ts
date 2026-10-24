// The floor contract (theseus-jid7t.6): the menu's greying agrees with the daemon's refusal table, row for row.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { greyOf } from '../src/ops/floor.ts'

const table = JSON.parse(readFileSync(new URL('./fixtures/profile_floor.json', import.meta.url), 'utf8')) as { rows: { mark: 'open' | 'private' | 'sensitive'; origin: string; known: boolean; refused: boolean }[] }

test('greyOf agrees with every row of the floor’s table', () => {
  assert.equal(table.rows.length, 24)
  for (const r of table.rows) {
    const got = greyOf(r.mark, 'p', r.known ? { name: 'p', origin: r.origin } : undefined)
    assert.equal(got !== null, r.refused, `${r.mark} ${r.origin} known=${r.known}: ${got}`)
  }
})

test('the reasons are the daemon’s words, and an origin the daemon did not tell is not US', () => {
  assert.equal(greyOf('sensitive', 'kimi', { name: 'kimi', origin: 'cn' }), 'this conversation holds sensitive content, and `kimi` is not a model of US origin')
  assert.equal(greyOf('private', 'x', { name: 'x' }), 'this conversation holds private content, and `x` is not a model of US origin')
  assert.equal(greyOf(undefined, 'x', { name: 'x', origin: 'cn' }), null, 'no mark: open')
  assert.equal(greyOf('open', 'gone', undefined), '`gone` is not a configured profile')
})
