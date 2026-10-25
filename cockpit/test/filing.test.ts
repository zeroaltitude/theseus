// The proposals panel's lock while it files (`src/lib/filing.ts`, theseus-6o8q8), run by `npm test`; and the few calls
// its picks make (`src/lib/people.ts`). Invented people and topics.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { OntologyPersonProposals } from '@protocol'
import { advance, begin, end, filingWords, pickOff } from '../src/lib/filing.ts'
import { acceptCalls, rejectCalls } from '../src/lib/people.ts'

const row = (key: string, judgments: string[], as_person?: string): OntologyPersonProposals =>
  ({ key, name: key, judgments, as_person } as unknown as OntologyPersonProposals)

test('while filing, every pick control is off; at rest, only the ones off for their own reason', () => {
  assert.equal(pickOff(null), false)
  assert.equal(pickOff(null, true), true)
  assert.equal(pickOff(begin(12)), true)
  assert.equal(pickOff(begin(12), false), true)
})

test('progress counts from 0 to the total and never past it, and the end clears the lock', () => {
  let f = begin(3)
  assert.equal(filingWords(f), 'filing 0 of 3')
  f = advance(f)
  assert.equal(filingWords(f), 'filing 1 of 3')
  f = advance(f, 5)
  assert.deepEqual(f, { done: 3, total: 3 })
  f = end()
  assert.equal(f, null)
  assert.equal(pickOff(f), false)
  assert.equal(filingWords(f), '')
  assert.equal(advance(null), null)
})

test('the panel reads the lock from the hook’s progress for every pick control', () => {
  const src = readFileSync(new URL('../src/components/People.tsx', import.meta.url), 'utf8')
  assert.equal(src.match(/disabled=\{pickOff\(progress, /g)?.length, 2, 'both checkboxes')
  assert.match(src, /disabled=\{pickOff\(progress\)\}[^\n]*select all<\/button>/)
})

test('picked people go in as few accept_all calls as one as_person per call allows', () => {
  const rows = [row('a', ['j1', 'j2']), row('b', ['j3']), row('c', ['j4', 'j5'], 'Marlo Quill'), row('d', ['j6'], 'Ivo Test')]
  const calls = acceptCalls(rows, ['t1', 't2'])
  assert.equal(calls.length, 3)
  assert.deepEqual(calls[0], { params: { min_confidence: 0, judgments: ['t1', 't2', 'j1', 'j2', 'j3'] }, n: 5 })
  assert.equal(calls[1].params.as_person, 'Marlo Quill')
  assert.equal(calls[2].n, 1)
  assert.equal(calls.reduce((a, c) => a + c.n, 0), 8)
  assert.deepEqual(acceptCalls([], []), [])
})

test('rejects are one call, whatever the rows', () => {
  const calls = rejectCalls([row('a', ['j1']), row('c', ['j2', 'j3'], 'Marlo Quill')])
  assert.deepEqual(calls, [{ params: { judgments: ['j1', 'j2', 'j3'] }, n: 3 }])
  assert.deepEqual(rejectCalls([]), [])
})
