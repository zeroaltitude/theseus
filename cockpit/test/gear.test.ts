// The gear turns while a job runs and stops with it (`src/ship/jobs.ts`, theseus-fdvz): a `tool.ended` or a verified
// cancel takes the job off at once, whatever a stale action list still says; a job still running keeps its gear.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { endsJob, jobsRunning } from '../src/ship/jobs.ts'

const act = (id: string, o: Record<string, unknown> = {}) => ({ correlation_id: id, tool: 'proc.run', dispatched_at_ms: 1000, settled_at_ms: null, ...o }) as any
const NONE = new Map<string, number>()

test('a started job turns the gear, from the push alone or the action list alone', () => {
  assert.deepEqual([...jobsRunning(new Set(['a']), new Set(), NONE, [])], ['a'])
  assert.deepEqual([...jobsRunning(new Set(), new Set(), NONE, [act('b')])], ['b'])
})

test('a stopped job leaves at once though the stale action list still shows it unsettled', () => {
  // tool.started then tool.ended: the push removed it from `live` and noted the end; the next poll is up to 5 s away.
  const run = jobsRunning(new Set(['b']), new Set(['a']), NONE, [act('a'), act('b')])
  assert.deepEqual([...run].sort(), ['b'], 'the stopped job is down, the running one keeps its gear')
  assert.equal(run.size, 1, 'a positive count: the gear is not cleared for everyone')
})

test('a verified cancel takes the job off before the end arrives', () => {
  const run = jobsRunning(new Set(), new Set(), new Map([['a', 5000]]), [act('a'), act('b')])
  assert.deepEqual([...run], ['b'])
})

test('a settled action takes its job off, as before', () => {
  assert.equal(jobsRunning(new Set(['a']), new Set(), NONE, [act('a', { settled_at_ms: 2000 })]).size, 0)
})

test('a background placeholder is not an end: the job goes on', () => {
  assert.equal(endsJob('background'), false)
  for (const s of ['ok', 'error', 'cancelled', 'declined']) assert.equal(endsJob(s), true, s)
})
