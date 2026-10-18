// The progress line's eight cases, the inputs and outputs of the Rust golden's (contract C1, theseus-ypnxt.6).
import test from 'node:test'
import assert from 'node:assert/strict'
import type { WorkView } from '../src/protocol.gen/WorkView.ts'
import { lineUsd, progressLine } from '../src/lib/progressLine.ts'

const START = 1_000_000
const base = { id: 'exe_a', kind: 'task', position: 1, at_ms: START, root: 'ses_r', title: 'a task', state: 'running', started_at_ms: START } as unknown as WorkView
const cost = { spent_usd: 0.4, limit_usd: 2 }
const progress = { steps_done: 2, steps_total: 5, at_ms: START }
const forecast = { usd_p50: 1, usd_p90: 2, min_p50: 10, min_p90: 20, n: 4 }
const at = (min: number) => START + min * 60_000
const three = { ...base, progress, cost }
const four = { ...three, forecast }

test('the eight cases', () => {
  assert.equal(progressLine(base, at(0)), undefined)
  assert.equal(progressLine({ ...base, cost }, at(0)), '$0.40 of $2')
  assert.equal(progressLine(three, at(0)), 'step 3 of 5 · $0.40 of $2')
  assert.equal(progressLine(four, at(4)), 'step 3 of 5 · $0.40 of $2 · ~6 min left')
  assert.equal(progressLine(four, at(15)), 'step 3 of 5 · $0.40 of $2 · ~5 min left')
  assert.equal(progressLine(four, at(25)), 'step 3 of 5 · $0.40 of $2 · past its usual time')
  assert.equal(progressLine({ ...four, paused: true }, at(4)), 'paused · step 3 of 5 · $0.40 of $2 · ~6 min left')
  assert.equal(progressLine({ ...base, progress: { ...progress, steps_done: 5, steps_total: 5 }, forecast: { ...forecast, n: 0 } }, at(4)), 'step 5 of 5')
})

test('the edges: no plan, a past flag, hours, and dollars as the Rust function writes them', () => {
  assert.equal(progressLine({ ...base, progress: { ...progress, steps_total: 0 } }, at(0)), undefined)
  assert.equal(progressLine({ ...base, forecast: { ...forecast, past: true } }, at(1)), 'past its usual time')
  assert.equal(progressLine({ ...base, forecast: { ...forecast, min_p50: 150 } }, at(0)), '~3 h left')
  assert.deepEqual([0.0042, 0.4, 2, 25, 10.02].map(lineUsd), ['$0.0042', '$0.40', '$2', '$25', '$10.02'])
})
