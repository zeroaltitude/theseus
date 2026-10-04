// Jev's judgments as the cockpit reads them (`src/lib/judgment.ts`, M5 23b), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { bars, judged, line, marksOf, packStats, quantile } from '../src/lib/judgment.ts'

const row = (at: number, data: Record<string, unknown>) =>
  ({ position: at, at_unix_ms: at, kind: 'judge.call', session_id: 'ses_a', turn_id: 'turn_a', data }) as any

const answered = (id: string, cls: string, ms: number, extra: Record<string, unknown> = {}) => row(1, {
  id, pack: 'loop.v1', version: 1, mode: 'shadow', outcome: { outcome: 'answered' }, cost_micros: 90,
  timing: { total_ms: ms }, context: { class: cls }, disagrees: false,
  answers: [{
    question: 'work_state',
    answer: { type: 'choice', choice: 'complete', confidence: 0.93, probabilities: [['complete', 0.93], ['progressing', 0.07]] },
    band: { band: 'act', top: { kind: 'choice', value: 'complete' }, value: 0.93 },
  }],
  ...extra,
})

test('a judgment in short says what Jev answered and whether it agrees', () => {
  assert.equal(line(judged(answered('jdg_1', 'reply', 400))), 'Jev (shadow): complete 0.93 · agrees')
  assert.equal(line(judged(answered('jdg_2', 'reply', 400, { disagrees: true }))), 'Jev (shadow): complete 0.93 · disagrees')
  assert.equal(line(judged(answered('jdg_3', 'reply', 400, { model_drift: true }))), 'Jev (shadow): complete 0.93 · drift, not acted on')
  const failed = judged(row(2, { id: 'jdg_4', pack: 'loop.v1', mode: 'shadow', outcome: { outcome: 'failed', class: 'timeout' } }))
  assert.equal(line(failed), 'Jev (shadow): failed · timeout')
  const skipped = judged(row(3, { id: 'jdg_5', pack: 'loop.v1', mode: 'shadow', outcome: { outcome: 'skipped', reason: 'shed' } }))
  assert.equal(line(skipped), 'Jev (shadow): skipped · shed')
})

test('a pack counts its calls and cost, and times them by class over the ones that reached Jev', () => {
  const js = [
    answered('a', 'reply', 100), answered('b', 'reply', 300), answered('c', 'tools', 900, { disagrees: true }),
    row(4, { id: 'd', pack: 'loop.v1', version: 1, mode: 'shadow', outcome: { outcome: 'skipped', reason: 'shed' }, context: { class: 'reply' }, timing: { total_ms: 0 } }),
  ].map(judged)
  const [p] = packStats(js)
  assert.deepEqual(
    { pack: p.pack, calls: p.calls, answered: p.answered, skipped: p.skipped, disagrees: p.disagrees, cost: p.costMicros },
    { pack: 'loop.v1', calls: 4, answered: 3, skipped: 1, disagrees: 1, cost: 270 },
  )
  assert.deepEqual(p.classes, [{ cls: 'reply', calls: 2, p50: 100, p95: 300 }, { cls: 'tools', calls: 1, p50: 900, p95: 900 }])
  assert.equal(quantile([], 0.5), null)
  assert.equal(quantile([5, 1, 3, 2, 4], 0.5), 3)
})

test('an answer is bars, its lean marked', () => {
  assert.deepEqual(bars((answered('a', 'reply', 1).data as any).answers[0]), [
    { label: 'complete', p: 0.93, chosen: true }, { label: 'progressing', p: 0.07, chosen: false },
  ])
  const noul = bars({ answer: { type: 'noul', noul: 0.2 } })
  assert.deepEqual(noul.map((b) => [b.label, b.chosen]), [['yes', false], ['no', true]])
  assert.ok(Math.abs(noul[1].p - 0.8) < 1e-9)
})

test('a trace names each judgment its turn dispatched, and the loop it judged', () => {
  const trace = {
    name: 'turn', kind: 'turn', start_us: 0, children: [
      { name: 'loop 0', kind: 'loop', start_us: 10, children: [] },
      { name: 'judge', kind: 'mark', start_us: 90, attrs: { pack: 'loop.v1', point: 'loop_end', mode: 'shadow', judgment: 'jdg_1', loop: 0 } },
    ],
  }
  assert.deepEqual(marksOf(trace), [{ judgment: 'jdg_1', pack: 'loop.v1', point: 'loop_end', mode: 'shadow', loop: 0, at: 90 }])
  assert.deepEqual(marksOf(null), [])
})
