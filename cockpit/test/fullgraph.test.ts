// The full-window graph's address and keys (theseus-uu3b2).
import test from 'node:test'
import assert from 'node:assert/strict'
import { fullKey, parseFull, writeFull } from '../src/lib/fullgraph.ts'

test('Esc leaves full screen and an unrelated key does not', () => {
  assert.equal(fullKey({ key: 'Escape' }, true), 'leave')
  assert.equal(fullKey({ key: 'Escape' }, false), null)
  assert.equal(fullKey({ key: 'x' }, true), null)
  assert.equal(fullKey({ key: 'f' }, false), 'enter')
  assert.equal(fullKey({ key: 'f' }, true), 'leave')
  assert.equal(fullKey({ key: 'f', ctrlKey: true }, false), null)
})

test('the address round-trips', () => {
  const base = new URLSearchParams('task=t1&taskgraph=1')
  const on = writeFull(base, true)
  assert.equal(parseFull(on), true)
  assert.equal(on.get('task'), 't1')
  assert.equal(on.get('taskgraph'), '1')
  const off = writeFull(on, false)
  assert.equal(parseFull(off), false)
  assert.equal(off.toString(), base.toString())
  assert.equal(parseFull(new URLSearchParams('graph=huge')), false)
})

test('a key typed in a field does nothing', () => {
  assert.equal(fullKey({ key: 'f', target: { tagName: 'INPUT' } }, false), null)
  assert.equal(fullKey({ key: 'f', target: { tagName: 'textarea' } }, false), null)
  assert.equal(fullKey({ key: 'f', target: { tagName: 'DIV', isContentEditable: true } }, false), null)
  assert.equal(fullKey({ key: 'f', target: { tagName: 'DIV' } }, false), 'enter')
})
