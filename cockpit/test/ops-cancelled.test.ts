// A cancelled task has ended (theseus-emqbm). The daemon reads a cancel as a `suspended` record, and the page once took
// that for open work, so three tasks cancelled twenty days ago sat in paused·queued and in its tile. A cancel is the
// owner's own stop, and the page has no verb that resumes it: it belongs in Done, never in the open buckets.
import test from 'node:test'
import assert from 'node:assert/strict'
import { boardRows, columnsOf, countersOf, shown, type OpsRow } from '../src/ops/board.ts'
import { bucketOfCard, bucketOfNode, bucketOfWork, openView } from '../src/ops/state.ts'
import { HOUR, info, MIN, NOW, record, view } from './ops-fixtures.ts'

const DAY = 24 * HOUR
const cx = { now: NOW, spentToday: new Set<string>(), records: [] }

/** The owner's page: three tasks cancelled 19 to 21 days ago (the board no longer holds their views; `task.list` does),
 *  each with the suspended record a cancel leaves; one task the owner paused; one that ran out of budget, whose record
 *  is suspended too and which a raised budget would resume; one cancelled ten minutes ago, its view still held. */
function fixture() {
  const tasks = [
    info(1, 'cancelled', { updated_at_ms: NOW - 19 * DAY }),
    info(2, 'cancelled', { updated_at_ms: NOW - 20 * DAY }),
    info(3, 'cancelled', { updated_at_ms: NOW - 21 * DAY }),
    info(9, 'budget_exhausted', { updated_at_ms: NOW - 2 * DAY }),
  ]
  const views = [
    view('exe_p', 'task', 'running', { session_id: 'ses_p', paused: true }),
    view('exe_cx', 'task', 'cancelled', { session_id: 'ses_cx', ended_at_ms: NOW - 10 * MIN }),
  ]
  const records = [
    record('tsk_c1', { session: 'ses_t1', state: 'suspended' }),
    record('tsk_c2', { session: 'ses_t2', state: 'suspended' }),
    record('tsk_c3', { session: 'ses_t3', state: 'suspended' }),
    record('tsk_b9', { session: 'ses_t9', state: 'suspended' }),
    record('tsk_cx', { session: 'ses_cx', state: 'suspended' }),
  ]
  return { views, tasks, records }
}
const ids = (rs: readonly OpsRow[]) => rs.map((r) => r.id).sort()

test('a task cancelled weeks ago is ended: in Done, not paused·queued, and in no open counter', () => {
  const { views, tasks, records } = fixture()
  const rows = boardRows({ views, tasks, sessions: [], records, now: NOW, past: false })
  for (const id of ['ses_t1', 'ses_t2', 'ses_t3']) {
    const r = rows.find((x) => x.id === id)!
    assert.deepEqual([r.bucket, r.column], ['ended', 'done'], `${id}: cancelled is terminal`)
  }
  const cols = columnsOf(rows, NOW)
  assert.deepEqual(ids(cols.paused), ['ses_p', 'ses_t9'], 'paused·queued holds the owner’s pause and the budget stop, and no cancel')
  assert.deepEqual(ids(cols.older.filter((r) => ['ses_t1', 'ses_t2', 'ses_t3'].includes(r.id))), ['ses_t1', 'ses_t2', 'ses_t3'], 'the three wait in Done, older')
  const c = countersOf(rows, NOW, 0)
  assert.equal(c.paused, 2, 'the tile counts the two that are paused, not the three cancelled')
  assert.equal(c.done, 1, 'done today is the task cancelled ten minutes ago: twenty days ago is not today')
  assert.deepEqual(ids(shown(rows, { bucket: 'paused' }, '', cx).rows), ['ses_p', 'ses_t9'], 'the filter keeps what the tile counts')
})

test('a cancel ended it whether the board still holds its view or only task.list does', () => {
  const { views, tasks, records } = fixture()
  const rows = boardRows({ views, tasks, sessions: [], records, now: NOW, past: false })
  const live = rows.find((r) => r.id === 'ses_cx')!
  assert.deepEqual([live.bucket, live.column], ['ended', 'done'], 'with its view')
  const rec = records.find((t) => t.id === 'tsk_cx')!
  const v = views.find((x) => x.id === 'exe_cx')!
  assert.equal(bucketOfWork(v, NOW, rec), 'ended', 'the view and its record')
  assert.equal(openView(v, rec), false, 'the Threads lens does not hold it open either')
  assert.equal(bucketOfNode({ kind: 'task', state: 'cancelled', view: v, record: rec }, NOW), 'ended', 'nor does the Graph')
  assert.equal(bucketOfCard({ kind: 'task', task: { section: 'done', paused: false, view: v }, record: rec }, NOW), 'ended', 'the card’s one path')
})

test('a record suspended for another reason stays open: out of budget, and a step suspended with no execution of its own', () => {
  const { views, tasks, records } = fixture()
  const rows = boardRows({ views, tasks, sessions: [], records, now: NOW, past: false })
  const budget = rows.find((r) => r.id === 'ses_t9')!
  assert.deepEqual([budget.bucket, budget.column], ['paused', 'paused'], 'a raised budget resumes it')
  const step = record('tsk_step', { state: 'suspended', origin: { session: 'ses_home' } as never })
  const withStep = boardRows({ views, tasks, sessions: [], records: [...records, step], now: NOW, past: false })
  assert.equal(withStep.find((r) => r.id === 'tsk_step')!.bucket, 'paused')
})
