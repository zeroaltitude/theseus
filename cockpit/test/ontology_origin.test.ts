// The Ontology tree's origin filter (theseus-tq7s8, R7b): a category's origin and confidence read from the ledger, the filter
// that keeps what matches and its ancestors as context, and the address's `?origin=`. Pure parts of `src/lib/ontology.ts`,
// run by `npm test` with node alone. Invented categories and ledger rows.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { LedgerEntry, OntologyCategory } from '@protocol'
import { originCounts, originFilter, originOf, originParam, originsOf, parseOriginParam, treeOrder, UNKNOWN_ORIGIN } from '../src/lib/ontology.ts'

let pos = 0
const row = (kind: string, data: unknown, session: string | null = null): LedgerEntry =>
  ({ position: ++pos, at_unix_ms: 1_000 + pos, kind, session_id: session, turn_id: null, data })
const undoRec = (write: string, key: string, wrote: unknown, before: unknown = null) => ({ write, key, wrote, before })
const catRow = (id: string, origin: string, write?: string) =>
  row('ontology.category', { id, kind: id.split(':')[0], name: id.split(':')[1], parent: null, origin, who: 'x', via: 'y', ...(write ? { undo: undoRec(write, `onto:cat:${id}`, { id }) } : {}) })
const memRow = (session: string, write: string, category: string, confidence: number) =>
  row('ontology.membership', {
    kind: 'topic', categories: [category], who: 'x', via: 'machine',
    undo: undoRec(write, `onto:member:${session}:topic`, { session, kind: 'topic', members: [{ category, origin: 'jev', confidence, as_of_ms: 1 }] }),
  }, session)

const cat = (id: string, parent?: string): OntologyCategory => ({
  id, kind: id.split(':')[0], name: id.split(':')[1], parent, depth: 1, description: '', added_by: 'the operator', members: 0,
})

const ledger: LedgerEntry[] = [
  catRow('topic:harbor', 'operator'),
  catRow('topic:kelp', 'jev', 'j1'),
  memRow('ses_a', 'j1', 'topic:kelp', 0.82),
  catRow('topic:kelp', 'operator'),                       // a later change of the same id by another origin: the first row names the origin
  catRow('topic:tides', 'sweep', 's1'),                   // a write with no membership row: no confidence
  catRow('topic:imported', 'import'),
  catRow('topic:dreamt', 'dream', 'd1'),
]

// harbor (operator) > kelp (jev) > tides (sweep); a second root, reef (nothing in the ledger); imported; dreamt.
const tree = treeOrder([
  cat('topic:harbor'), cat('topic:kelp', 'topic:harbor'), cat('topic:tides', 'topic:kelp'),
  cat('topic:reef'), cat('topic:imported'), cat('topic:dreamt'),
])
const origins = originsOf(ledger)

test('a category’s origin is the origin of its first ledger row, and its confidence is the one its minting write gave it', () => {
  assert.equal(originOf(origins, 'topic:harbor').origin, 'operator')
  assert.equal(originOf(origins, 'topic:kelp').origin, 'jev', 'the later operator row does not rename the origin')
  assert.equal(originOf(origins, 'topic:kelp').confidence, 0.82)
  assert.equal(originOf(origins, 'topic:tides').origin, 'sweep')
  assert.equal(originOf(origins, 'topic:tides').confidence, null, 'no membership row for its write: no confidence is invented')
  assert.equal(originOf(origins, 'topic:harbor').confidence, null, 'an operator’s category carries no confidence')
})

test('a category the ledger has no row for reads as unknown, never as an origin it was not given', () => {
  assert.deepEqual(originOf(origins, 'topic:reef'), { origin: UNKNOWN_ORIGIN, confidence: null })
  assert.equal(origins.has('topic:reef'), false)
})

test('with no origin picked every row shows and none is context', () => {
  const out = originFilter(tree, origins, new Set())
  assert.deepEqual(out.map((r) => r.category.id), tree.map((r) => r.category.id))
  assert.ok(out.every((r) => !r.context))
})

test('the filter keeps the matches and their ancestors, the ancestors marked as context', () => {
  const out = originFilter(tree, origins, new Set(['sweep']))
  assert.deepEqual(out.map((r) => [r.category.id, r.depth, r.context]), [
    ['topic:harbor', 1, true],
    ['topic:kelp', 2, true],
    ['topic:tides', 3, false],
  ])
})

test('a matching ancestor is a match, not context, and a match below it keeps its place', () => {
  const out = originFilter(tree, origins, new Set(['operator', 'sweep']))
  assert.deepEqual(out.map((r) => [r.category.id, r.context]), [
    ['topic:harbor', false],
    ['topic:kelp', true],
    ['topic:tides', false],
  ])
})

test('unknown is an option, so a category with no ledger row is not hidden from every filter', () => {
  const out = originFilter(tree, origins, new Set([UNKNOWN_ORIGIN]))
  assert.deepEqual(out.map((r) => r.category.id), ['topic:reef'])
})

test('every category is in exactly one origin’s count, so nothing is left out of the chips', () => {
  const c = originCounts(tree, origins)
  assert.deepEqual([...c].sort(), [['dream', 1], ['import', 1], ['jev', 1], ['operator', 1], ['sweep', 1], ['unknown', 1]])
  assert.equal([...c.values()].reduce((a, b) => a + b, 0), tree.length)
})

test('the address holds the picked origins in the chart’s order, and a word that is no origin is dropped', () => {
  assert.equal(originParam(new Set(['sweep', 'operator', UNKNOWN_ORIGIN])), 'operator,sweep,unknown')
  assert.equal(originParam(new Set()), null)
  assert.deepEqual([...parseOriginParam('sweep,operator,bogus,,jev')].sort(), ['jev', 'operator', 'sweep'])
  assert.deepEqual([...parseOriginParam(null)], [])
  assert.deepEqual([...parseOriginParam('')], [])
})

test('the view draws the origin and confidence columns and the filter from these functions, and keeps the picks in the address', () => {
  const v = readFileSync(new URL('../src/views/Ontology.tsx', import.meta.url), 'utf8')
  assert.match(v, /originsOf\(ledger\)/)
  assert.match(v, /originFilter\(g\.rows, origins, wanted\)/)
  assert.match(v, /parseOriginParam\(params\.get\('origin'\)\)/)
  assert.match(v, /p\.set\('origin', v\)/)
  assert.match(v, /confidenceWords\(made\.confidence\)/)
})
