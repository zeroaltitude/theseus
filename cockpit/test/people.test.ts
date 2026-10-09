// The people's pure parts (`src/lib/people.ts`), run by `npm test` with node's own runner. Invented people and topics.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { OntologyCategory, OntologyProposal } from '@protocol'
import { handlesLine, matches, peopleOf, pulldownGroups, selectAll } from '../src/lib/people.ts'

const cat = (id: string, name: string, handles: string[] = [], members = 0): OntologyCategory => ({
  id, kind: id.split(':')[0], name, depth: 1, description: '', added_by: 'import gull-2026-04', members, handles,
})
const cats = [
  cat('topic:harbor', 'harbor'),
  cat('person:marlo-quill', 'Marlo Quill', ['slack:U0GULL01', 'name:Marlo Quill', 'name:M. Quill'], 4),
  cat('person:500000000000000007', '@pell', ['discord:500000000000000007'], 9),
  cat('channel:200000000000000001', 'general'),
  cat('topic:kiln', 'kiln'),
]

test('the pulldown groups topics then people, by name, leaving out what the session holds and the given places', () => {
  const g = pulldownGroups(cats, new Set(['topic:kiln']))
  assert.deepEqual(g.map((x) => [x.label, x.items.map((c) => c.id)]), [
    ['topics', ['topic:harbor']],
    ['people', ['person:500000000000000007', 'person:marlo-quill']],
  ])
})

test('a search reads a person’s handles as well as names and ids, and drops an empty group', () => {
  assert.ok(matches(cats[1], 'gull01'))
  assert.ok(matches(cats[1], 'm. quill'))
  assert.ok(!matches(cats[1], 'pell'))
  const g = pulldownGroups(cats, new Set(), '5000000')
  assert.deepEqual(g.map((x) => [x.kind, x.items.map((c) => c.name)]), [['person', ['@pell']]])
})

test('a person’s line gives its ids first and its other names after', () => {
  assert.equal(handlesLine(cats[1]), 'slack:U0GULL01 · aka M. Quill')
  assert.equal(handlesLine(cats[0]), '')
})

test('the people panel orders by sessions, then name', () => {
  assert.deepEqual(peopleOf(cats).map((c) => c.name), ['@pell', 'Marlo Quill'])
})

test('select all takes the bulkable proposals of a kind at a confidence', () => {
  const p = (judgment: string, topic: string | undefined, confidence: number): OntologyProposal => ({
    judgment, session_id: 'ses_a', topic, new_topic: !topic, confidence, band: 'confirm', at_ms: 1,
  })
  const ps = [p('jdg_1', 'person:marlo-quill', 0.9), p('jdg_2', 'topic:harbor', 0.95), p('jdg_3', undefined, 0.99), p('jdg_4', 'person:pell', 0.4)]
  assert.deepEqual(selectAll(ps, 'person', 0.5), ['jdg_1'])
  assert.deepEqual(selectAll(ps, null), ['jdg_1', 'jdg_2', 'jdg_4'])
})
