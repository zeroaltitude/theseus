// The people's pure parts (`src/lib/people.ts`), run by `npm test` with node's own runner. Invented people and topics.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { OntologyCategory, OntologyPersonProposals, OntologyProposal } from '@protocol'
import {
  handlesLine, matches, peopleOf, proposalKind, proposalWhat, pulldownGroups, rowAccept, rowAsk, rowBulkable, rowMeta, rowReject, selectAll,
  selectRows,
} from '../src/lib/people.ts'

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

test('a person Jev proposes (people.v1) is a person’s proposal, new or held, and bulkable', () => {
  const base = { session_id: 'ses_b', new_topic: false, band: 'act', at_ms: 1 }
  const fresh: OntologyProposal = {
    ...base, judgment: 'jdg_5', confidence: 0.93,
    person: { name: 'Wren Halloway', handles: ['slack:U0TIDE07'], role_line: 'Takes the gauge readings.', new: true },
  }
  const held: OntologyProposal = {
    ...base, judgment: 'jdg_6', confidence: 0.91, topic: 'person:orrin-vale', topic_name: 'Orrin Vale',
    person: { name: 'Orrin Vale', new: false },
  }
  assert.equal(proposalKind(fresh), 'person')
  assert.equal(proposalWhat(fresh), 'a new person, Wren Halloway (slack:U0TIDE07): Takes the gauge readings.')
  assert.equal(proposalWhat(held), 'Orrin Vale')
  assert.deepEqual(selectAll([fresh, held], 'person', 0.92), ['jdg_5'])
  assert.deepEqual(selectAll([fresh, held], 'topic'), [])
})

// theseus-fvyx: a proposed person's row, its words, its one accept (each new person's as the row's person) and reject,
// and select-all over rows, an ambiguous first name left out.
const row = (o: Partial<OntologyPersonProposals>): OntologyPersonProposals => ({
  key: 'name:wren halloway', name: 'Wren Halloway', new: true, as_person: 'Wren Halloway', judgments: ['jdg_1', 'jdg_2', 'jdg_3'],
  sessions: 2, confidence_min: 0.62, confidence_max: 0.95, bands: ['act', 'confirm'], at_ms: 1, ...o,
})

test('a person’s row says its sessions, proposals, range and bands, and the first names it holds', () => {
  const wren = row({ first_names: ['Wren'] })
  assert.equal(rowMeta(wren), 'new · 2 sessions · 3 proposals · 0.62–0.95 act/confirm')
  assert.equal(rowAsk(wren), 'with “Wren” as Wren Halloway?')
  const held = row({ key: 'person:orrin-vale', name: 'Orrin Vale', new: false, as_person: 'person:orrin-vale', judgments: ['jdg_9'], sessions: 1, confidence_min: 0.9, confidence_max: 0.9, bands: ['act'] })
  assert.equal(rowMeta(held), 'person:orrin-vale · 1 session · 1 proposal · 0.90 act')
  assert.equal(rowAsk(held), '')
})

test('a row is answered at once: every proposal, as its person', () => {
  const wren = row({ first_names: ['Wren'] })
  assert.deepEqual(rowAccept(wren), { min_confidence: 0, judgments: ['jdg_1', 'jdg_2', 'jdg_3'], as_person: 'Wren Halloway' })
  assert.deepEqual(rowReject(wren), { judgments: ['jdg_1', 'jdg_2', 'jdg_3'] })
})

test('an ambiguous first name is flagged and never selected in bulk', () => {
  const tern = row({ key: 'name:tern', name: 'Tern', as_person: 'Tern', ambiguous: ['Tern Ashby', 'Tern Mallow'], confidence_max: 0.99 })
  assert.ok(!rowBulkable(tern))
  assert.match(rowAsk(tern), /^ambiguous: a word of Tern Ashby, Tern Mallow/)
  const low = row({ key: 'name:pell', confidence_max: 0.5 })
  assert.deepEqual(selectRows([row({}), tern, low], 0.6), ['name:wren halloway'])
  assert.deepEqual(selectRows([row({}), tern, low]), ['name:wren halloway', 'name:pell'])
})
