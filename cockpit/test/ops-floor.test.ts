// The model menu's floor: since the restricted-origin rule went (2026-11-21) it greys a profile only when it is not
// configured, whatever the session's mark and the profile's origin.
import test from 'node:test'
import assert from 'node:assert/strict'
import { greyOf } from '../src/ops/floor.ts'

test('no mark and no origin greys a configured profile; a profile that is not configured is grey', () => {
  assert.equal(greyOf('sensitive', 'kimi', { name: 'kimi' }), null)
  assert.equal(greyOf('private', 'x', { name: 'x' }), null)
  assert.equal(greyOf(undefined, 'x', { name: 'x' }), null, 'no mark: open')
  assert.equal(greyOf('open', 'gone', undefined), '`gone` is not a configured profile')
})
