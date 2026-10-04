// The learning ledger as the cockpit reads it (`src/lib/learning.ts`, M5 25c), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { labelChoices, localDay } from '../src/lib/learning.ts'

test('a report is named by its local day', () => {
  assert.equal(localDay(new Date(2026, 9, 4, 23, 59)), '2026-10-04')
  assert.equal(localDay(new Date(2026, 0, 9, 0, 0)), '2026-01-09')
})

test('a yes-or-no answer offers true and false, a choice its options, in order', () => {
  assert.deepEqual(labelChoices({ type: 'noul', noul: 0.4 }), { bools: true, options: [] })
  assert.deepEqual(
    labelChoices({ type: 'choice', choice: 'complete', confidence: 0.9, probabilities: [['complete', 0.9], ['progressing', 0.1]] }),
    { bools: false, options: ['complete', 'progressing'] },
  )
  assert.deepEqual(labelChoices({ type: 'score', score: 1.2, probabilities: [0.1, 0.6, 0.3], confidence: 0.6 }), { bools: false, options: [] })
  assert.deepEqual(labelChoices(null), { bools: false, options: [] })
})
