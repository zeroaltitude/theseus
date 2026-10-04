// The ontology view's pure parts (`src/lib/ontology.ts`), run by `npm test` with node's own runner: no dependency.
// Invented categories and sessions.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { OntologyCategory, OntologyKind, OntologyMembership } from '@protocol'
import { guidanceInPlay, guidanceWords, nothingPending, pendingOf, recordedOf, treeOrder } from '../src/lib/ontology.ts'

const cat = (id: string, name: string, parent?: string, guidance?: [number, string]): OntologyCategory => ({
  id, kind: id.split(':')[0], name, parent, depth: 1, description: '', added_by: 'the operator',
  guidance: guidance ? { category: id, text: 'be brief', version: guidance[0], digest: guidance[1], added_by: 'the operator' } : undefined,
})
const kind = (name: string, rule: string): OntologyKind => ({
  name, basis: name === 'topic' ? 'interpreted' : 'given', assigned_by: [], per_session: 'many', precedence: 1, rule, description: '', version: 1, added_by: 'seed',
})
const kinds = [kind('channel', 'intent_line'), kind('topic', 'chain')]
const member = (category: string, origin = 'operator'): OntologyMembership => ({ session_id: 'ses_a', kind: category.split(':')[0], category, origin, as_of_ms: 1 })

const cats = [
  cat('topic:harbor-tides', 'harbor-tides', 'topic:harbor'),
  cat('topic:zephyr', 'zephyr'),
  cat('channel:123', 'lab'),
  cat('topic:harbor', 'harbor', undefined, [1, 'aaaa']),
  cat('topic:harbor-tides-spring', 'spring', 'topic:harbor-tides'),
]

test('the tree is depth first: roots by name, each followed by its children, with its depth counted from its parents', () => {
  const rows = treeOrder(cats)
  assert.deepEqual(rows.map((r) => [r.category.id, r.depth]), [
    ['topic:harbor', 1],
    ['topic:harbor-tides', 2],
    ['topic:harbor-tides-spring', 3],
    ['channel:123', 1],
    ['topic:zephyr', 1],
  ])
})

test('a category whose parent is not listed stands as a root, and a cycle does not loop', () => {
  const rows = treeOrder([cat('topic:kelp', 'kelp', 'topic:gone'), cat('topic:a', 'a', 'topic:b'), cat('topic:b', 'b', 'topic:a')])
  assert.deepEqual(rows.map((r) => [r.category.id, r.depth]), [['topic:kelp', 1]])
})

const manifest = (memberships: unknown[], guidance: unknown[]) => ({ memberships, guidance })
const used = (category: string, origin = 'operator') => ({ kind: category.split(':')[0], category, origin, as_of_ms: 9 })

test('nothing is pending when the list and the manifest agree, as-of times aside', () => {
  const m = [member('topic:harbor'), member('channel:123', 'transport')]
  const p = pendingOf(m, cats, kinds, manifest([used('topic:harbor'), used('channel:123', 'transport')], [{ category: 'topic:harbor', version: 1, digest: 'aaaa' }]))
  assert.deepEqual(p, { added: [], removed: [], guidance: [] })
  assert.ok(nothingPending(p))
})

test('a membership added since the compile is pending, and so is one taken away', () => {
  const added = pendingOf([member('topic:harbor'), member('topic:zephyr')], cats, kinds, manifest([used('topic:harbor')], [{ category: 'topic:harbor', version: 1, digest: 'aaaa' }]))
  assert.deepEqual(added.added, ['topic:zephyr'])
  assert.deepEqual(added.removed, [])
  const removed = pendingOf([], cats, kinds, manifest([used('topic:zephyr')], []))
  assert.deepEqual(removed.removed, ['topic:zephyr'])
  assert.ok(!nothingPending(removed))
})

test('a guidance version changed since the compile is pending, for the category and for its chain', () => {
  const edited = cats.map((c) => (c.id === 'topic:harbor' ? cat('topic:harbor', 'harbor', undefined, [2, 'bbbb']) : c))
  // The session holds only the nested topic: the parent's guidance reaches it under `chain`.
  const p = pendingOf([member('topic:harbor-tides')], edited, kinds, manifest([used('topic:harbor-tides')], [{ category: 'topic:harbor', version: 1, digest: 'aaaa' }]))
  assert.deepEqual(p.added, [])
  assert.deepEqual(p.removed, [])
  assert.equal(p.guidance.length, 1)
  assert.equal(guidanceWords(p.guidance[0]), 'topic:harbor guidance v1 → v2')
  // The same version with another digest is a change too; guidance taken away is one.
  const swapped = pendingOf([member('topic:harbor-tides')], cats, kinds, manifest([used('topic:harbor-tides')], [{ category: 'topic:harbor', version: 1, digest: 'cccc' }]))
  assert.equal(swapped.guidance.length, 1)
  const gone = pendingOf([member('topic:harbor-tides')], cats.map((c) => (c.id === 'topic:harbor' ? cat('topic:harbor', 'harbor') : c)), kinds, manifest([used('topic:harbor-tides')], [{ category: 'topic:harbor', version: 1, digest: 'aaaa' }]))
  assert.equal(guidanceWords(gone.guidance[0]), 'topic:harbor guidance no longer applies (was v1)')
})

test('an intent_line kind takes no ancestors, and a manifest with neither field recorded none (the daemon leaves empty lists out)', () => {
  const play = guidanceInPlay([member('channel:123', 'transport')], [cat('channel:123', 'lab', undefined, [3, 'dddd'])], kinds)
  assert.deepEqual(play, [{ category: 'channel:123', version: 3, digest: 'dddd' }])
  const none = { added: ['topic:harbor'], removed: [], guidance: [{ category: 'topic:harbor', from: null, to: { category: 'topic:harbor', version: 1, digest: 'aaaa' } }] }
  assert.deepEqual(pendingOf([member('topic:harbor')], cats, kinds, { model: 'x' }), none)
  assert.deepEqual(pendingOf([member('topic:harbor')], cats, kinds, undefined), none)
  assert.deepEqual(pendingOf([], cats, kinds, { model: 'x' }), { added: [], removed: [], guidance: [] })
  assert.equal(recordedOf({ memberships: 'no', guidance: [null, 3, { category: 'topic:a', version: 2 }] }).guidance.length, 1)
})
