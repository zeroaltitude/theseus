// A notified call's score (`src/lib/scores.ts`, M5 step 24), run by `npm test` with node's own runner.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { scoresOf, scoreWords } from '../src/lib/scores.ts'

const judged = (call: string, risky: number, outcome = 'answered', pack = 'security.v1') =>
  ({ at_unix_ms: 1, kind: 'judge.call', session_id: 's', turn_id: 't', data: {
    id: `jdg_${call}`, pack, mode: 'shadow', outcome: { outcome }, context: { call },
    answers: [{ question: 'risky', band: { value: risky } }, { question: 'destructive', band: { value: 0.9 } }],
  } }) as any

test('a push scores its call as it lands, and the row, once written, is the record', () => {
  const push = { method: 'judge.scored', params: { correlation_id: 'act_1', percent: 12, mode: 'shadow', judgment: 'jdg_x' } }
  const live = scoresOf([], [push, { method: 'tool.ended', params: {} }])
  assert.equal(scoreWords(live.get('act_1')!), 'risk 12% (shadow)')
  const rows = [judged('act_1', 0.874), judged('act_2', 0.5, 'failed'), judged('act_3', 0.9, 'answered', 'security.v3')]
  const both = scoresOf(rows, [push])
  assert.equal(scoreWords(both.get('act_1')!), 'risk 87% (shadow)')
  assert.equal(both.get('act_2'), undefined, 'a failed judgment has no score')
  assert.equal(both.get('act_3'), undefined, 'only security.v1 scores a notice')
})
