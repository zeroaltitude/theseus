// The inspector's report field says what it holds (theseus-emqbm). It showed the first line of a task's report under the
// bare word `report`, so the owner took one line for the whole thing and "could not find a report". The label now
// says which it is, a long report opens in full, and the transcript is a link beside `open`.
import test from 'node:test'
import assert from 'node:assert/strict'
import { boardRows } from '../src/ops/board.ts'
import { cardFacts, reportFacts } from '../src/ops/facts.ts'
import { board, NOW } from './ops-fixtures.ts'

const LINE = 'Forty moorings, six free.'
const LONG = `${LINE}\n\nThe east quay holds twenty, the west fourteen, and the spit six.\n\nNext: the buoys.`

test('a report that is one line is labelled complete', () => {
  assert.deepEqual(reportFacts(LINE, LINE), { label: 'report (complete)', line: LINE, more: false })
  assert.equal(reportFacts(LINE, `  ${LINE}\n\n`)?.label, 'report (complete)', 'trailing blank lines are not more report')
})

test('a longer report says it is only the first line, and offers the whole', () => {
  assert.deepEqual(reportFacts(LINE, LONG), { label: 'report (first line)', line: LINE, more: true, full: LONG })
})

test('a report not read yet, or with no text in its session, is never called complete', () => {
  assert.deepEqual(reportFacts(LINE, undefined), { label: 'report (first line)', line: LINE, more: false })
  assert.deepEqual(reportFacts(LINE, null), { label: 'report (first line)', line: LINE, more: false })
  assert.equal(reportFacts(undefined, LONG), undefined, 'no line, no field: the card keeps its "no report line"')
})

test('a done task’s card links its deck and its transcript beside open, and names no forum post it cannot reach', () => {
  const rows = new Map(boardRows({ ...board(), now: NOW }).map((r) => [r.id, r]))
  const done = cardFacts(rows.get('ses_t4')!, NOW)
  assert.deepEqual(done.links, [
    { label: 'open', href: '/session/ses_t4', title: 'its deck' },
    { label: 'transcript', href: '/session/ses_t4', title: 'everything it said and did, in its deck' },
  ])
  const wake = cardFacts(rows.get('wak_r1')!, NOW)
  assert.ok(wake.links?.every((l) => l.label !== 'transcript'), 'a wake has no transcript of its own')
})
