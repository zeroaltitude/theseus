// The proposals panel where two rows meet: the filing lock and one-call batches (theseus-6o8q8) with the topic
// Reject, Never and the reject-topics fix (theseus-648f8). A merge of the two is green whichever side of
// `answer()` it keeps, so these pin the places where taking the wrong side silently drops a feature.
// Invented people and topics.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { OntologyPersonProposals } from '@protocol'
import { rejectCalls } from '../src/lib/people.ts'

const row = (key: string, judgments: string[], as_person?: string): OntologyPersonProposals =>
  ({ key, name: key, judgments, as_person } as unknown as OntologyPersonProposals)

test('a reject files the picked topics as well as the rows', () => {
  const calls = rejectCalls([row('a', ['j1']), row('c', ['j2', 'j3'], 'Marlo Quill')], ['t1', 't2'])
  assert.deepEqual(calls, [{ params: { judgments: ['t1', 't2', 'j1', 'j2', 'j3'] }, n: 5 }])
  assert.deepEqual(rejectCalls([], ['t1']), [{ params: { judgments: ['t1'] }, n: 1 }])
  assert.deepEqual(rejectCalls([], []), [])
})

test('the panel hands its picked topics to the reject as it does to the accept', () => {
  const src = readFileSync(new URL('../src/components/People.tsx', import.meta.url), 'utf8')
  assert.match(src, /acceptCalls\(gs, topics\) : rejectCalls\(gs, topics\)/)
  assert.equal(src.match(/rejectCalls\(/g)?.length, 1, 'one reject path, through answer()')
})

test('the panel keeps the Never buttons, the unnamed-topic words and the filing lock together', () => {
  const src = readFileSync(new URL('../src/components/People.tsx', import.meta.url), 'utf8')
  assert.match(src, /<BulkActions /)
  assert.match(src, /<PersonNever /)
  assert.match(src, /<TopicActions /)
  assert.match(src, /<NeversList /)
  assert.match(src, /a new topic \(unnamed\)/)
  assert.match(src, /disabled=\{pickOff\(progress, !bulkable\(p\)\)\}/)
  assert.match(src, /<ShipsWheel /)
})
