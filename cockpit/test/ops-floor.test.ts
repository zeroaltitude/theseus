// The model menu's floor: since the restricted-origin rule went (2026-11-21) it greys a profile only when it is not
// configured, whatever the session's mark and the profile's origin.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync } from 'node:fs'
import { greyOf } from '../src/ops/floor.ts'

test('no mark and no origin greys a configured profile; a profile that is not configured is grey', () => {
  assert.equal(greyOf('sensitive', 'kimi', { name: 'kimi' }), null)
  assert.equal(greyOf('private', 'x', { name: 'x' }), null)
  assert.equal(greyOf(undefined, 'x', { name: 'x' }), null, 'no mark: open')
  assert.equal(greyOf('open', 'gone', undefined), '`gone` is not a configured profile')
})

test('no help on the page still promises the retired origin rule: a label keeps no model away', () => {
  // The model menu, the inspector and the graph said a private or sensitive label kept a conversation to models of US
  // origin, on the same screen that offers every configured model. protocol.gen is generated from the daemon's types.
  const dir = new URL('../src/', import.meta.url)
  const all = readdirSync(dir, { recursive: true }).map(String).filter((f) => /\.tsx?$/.test(f) && !f.startsWith('protocol.gen'))
  const said = all.filter((f) => /US[- ]origin|origin only|floor keeps/i.test(readFileSync(new URL(f, dir), 'utf8')))
  assert.deepEqual(said, [], 'files that still promise models of US origin')
})
