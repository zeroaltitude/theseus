import test from 'node:test'
import assert from 'node:assert/strict'
import { editParams, slugBad, slugOf } from '../src/ops/sheets.ts'

test('a slug from a title, and the slugs the daemon takes', () => {
  assert.equal(slugOf('  Tide Tables, 2nd pass! '), 'tide-tables-2nd-pass')
  assert.ok(!slugBad('tide-tables') && !slugBad('a1') && slugBad('') && slugBad('Tide') && slugBad('a--b') && slugBad('-a') && slugBad('a b'))
})

test('an edit sends only what changed, and nothing when nothing did', () => {
  const was = { title: 'Harbour', goal: null }
  assert.equal(editParams('h', was, { title: 'Harbour', goal: '' }), null)
  assert.deepEqual(editParams('h', was, { title: ' Harbour survey ', goal: '' }), { slug: 'h', title: 'Harbour survey' })
  assert.deepEqual(editParams('h', { title: 'H', goal: 'old' }, { title: 'H', goal: 'new' }), { slug: 'h', goal: 'new' })
})
