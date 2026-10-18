// The progress line's eight cases, the inputs and outputs of the Rust golden's (contract C1, theseus-ypnxt.6), and the
// golden file itself (the join of progress-words, theseus-ypnxt.2).
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
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

type Golden = { case: string; elapsed_min: number; expect: string | null; paused?: boolean
  cost?: { spent: number; limit: number }; progress?: { done: number; total: number }
  forecast?: { min_p50: number; min_p90: number; n: number } }

test('the Rust golden file, case by case', () => {
  const url = new URL('../../crates/theseus-protocol/src/work/golden/progress_line.json', import.meta.url)
  const cases: Golden[] = JSON.parse(readFileSync(url, 'utf8'))
  assert.equal(cases.length, 8)
  for (const c of cases) {
    const v = {
      ...base,
      paused: c.paused ?? false,
      ...(c.cost && { cost: { spent_usd: c.cost.spent, limit_usd: c.cost.limit } }),
      ...(c.progress && { progress: { steps_done: c.progress.done, steps_total: c.progress.total, at_ms: START } }),
      ...(c.forecast && { forecast: { ...forecast, ...c.forecast } }),
    } as WorkView
    assert.equal(progressLine(v, at(c.elapsed_min)), c.expect ?? undefined, c.case)
  }
})

test('the edges: no plan, a past flag, hours, and dollars as the Rust function writes them', () => {
  assert.equal(progressLine({ ...base, progress: { ...progress, steps_total: 0 } }, at(0)), undefined)
  assert.equal(progressLine({ ...base, forecast: { ...forecast, past: true } }, at(1)), 'past its usual time')
  assert.equal(progressLine({ ...base, forecast: { ...forecast, min_p50: 150 } }, at(0)), '~3 h left')
  // As progress_line.rs's `left`: under half a minute is `<1 min`, and hours are rounded once (89.6 min is 1 h).
  assert.equal(progressLine({ ...base, forecast }, at(9.8)), '~<1 min left')
  assert.equal(progressLine({ ...base, forecast: { ...forecast, min_p50: 89.6, min_p90: 120 } }, at(0)), '~1 h left')
  // An ended task, or one not started yet, has no time part (the CLI and Discord show none).
  assert.equal(progressLine({ ...base, state: 'done', progress: { ...progress, steps_done: 5, steps_total: 5 }, forecast }, at(30)), 'step 5 of 5')
  assert.equal(progressLine({ ...base, started_at_ms: 0, forecast } as WorkView, at(4)), undefined)
  assert.deepEqual([0.0042, 0.4, 2, 25, 10.02].map(lineUsd), ['$0.0042', '$0.40', '$2', '$25', '$10.02'])
})

test('a task a checkpoint moved says where it runs, last (the Rust golden of the moved cases)', () => {
  const url = new URL('../../crates/theseus-protocol/src/work/golden/progress_line_moved.json', import.meta.url)
  const cases: (Golden & { moved_to: string })[] = JSON.parse(readFileSync(url, 'utf8'))
  assert.equal(cases.length, 3)
  for (const c of cases) {
    const v = {
      ...base,
      paused: c.paused ?? false,
      moved_to: c.moved_to,
      ...(c.cost && { cost: { spent_usd: c.cost.spent, limit_usd: c.cost.limit } }),
      ...(c.progress && { progress: { steps_done: c.progress.done, steps_total: c.progress.total, at_ms: START } }),
      ...(c.forecast && { forecast: { ...forecast, ...c.forecast } }),
    } as WorkView
    assert.equal(progressLine(v, at(c.elapsed_min)), c.expect ?? undefined, c.case)
  }
  // A task on its base is the line it was.
  assert.equal(progressLine(four, at(4)), 'step 3 of 5 · $0.40 of $2 · ~6 min left')
})
