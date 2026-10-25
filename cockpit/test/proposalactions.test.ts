import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { OntologyPersonProposals, OntologyProposal } from '@protocol'
import { answerCall, answerMethod, confirmWords, neverWords, topicLabel, undoWords } from '../src/lib/proposalActions.ts'

const topic = { judgment: 'jdg_1', session_id: 'ses_a', session_title: 'Harbor notes', topic_name: 'Harbor', topic: 'topic:harbor', new_topic: false, confidence: 0.8, band: 'act', at_ms: 1 } as OntologyProposal
const unnamed = { ...topic, judgment: 'jdg_2', topic_name: undefined, topic: undefined, new_topic: true } as OntologyProposal
const person = { key: 'p:sable', name: 'Sable Thorn', judgments: ['jdg_3', 'jdg_4'] } as unknown as OntologyPersonProposals

test('a reject of topics sends reject_all with the judgments, a never sends ontology.proposal.never', () => {
  assert.equal(answerMethod('reject'), 'ontology.proposal.reject_all')
  assert.equal(answerMethod('never'), 'ontology.proposal.never')
  assert.deepEqual(answerCall('reject', ['jdg_1']), ['ontology.proposal.reject_all', { judgments: ['jdg_1'] }])
  assert.deepEqual(answerCall('never', ['jdg_3', 'jdg_4']), ['ontology.proposal.never', { judgments: ['jdg_3', 'jdg_4'] }])
})

test('an unnamed new topic reads as such, and its never is the session’s', () => {
  assert.equal(topicLabel(unnamed), 'a new topic (unnamed)')
  assert.equal(neverWords(unnamed), 'never propose a new topic for this session')
  assert.equal(topicLabel(topic), 'Harbor')
  assert.equal(neverWords(topic), 'never propose “Harbor” again')
})

test('the confirm words name the rows and say what a never keeps', () => {
  const never = confirmWords('never', [topic, unnamed], [person])
  assert.match(never, /4 proposals/)
  assert.match(never, /Harbor, a new topic in Harbor notes, Sable Thorn/)
  assert.match(never, /Undo it under nevers/)
  const reject = confirmWords('reject', [topic], [])
  assert.match(reject, /^Reject 1 proposal \(Harbor\)/)
  assert.match(reject, /may propose them again/)
})

test('an undo names the never it takes back', () => {
  assert.match(undoWords({ key: 'topic:harbor', kind: 'topic', name: 'Harbor', who: 'owner', at_ms: 1 }), /Undo the never on topic “Harbor”/)
})
