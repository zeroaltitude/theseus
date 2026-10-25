// The Operations page's counters, Board and Graph read one work set (theseus-odj0m): every card once, with its bucket and
// whether it counts. For each counter, the number it shows equals the counted cards a click on it keeps, the counted
// nodes the Graph draws normally for that click, and the counted cards in the Board's column, under today's answers
// to the owner's calls (`OWNER_CALLS`) and under the recommended ones (`RECOMMENDED`).
import test from 'node:test'
import assert from 'node:assert/strict'
import type { ConfirmRequest, ThreadAsk, WorkView } from '../src/protocol.gen/index.ts'
import { allCounters, boardRows, columnsOf, contextOf, shown, type OpsRow } from '../src/ops/board.ts'
import { layoutRows, type LaneMeta } from '../src/ops/graph.ts'
import { queueOf } from '../src/ops/queue.ts'
import { OWNER_CALLS, RECOMMENDED, type OwnerCalls } from '../src/ops/state.ts'
import type { Bucket } from '../src/ops/address.ts'
import { confirm, HOUR, MIN, NOW, record, view } from './ops-fixtures.ts'

const LANES: LaneMeta[] = [{ key: 'harbour-survey', title: 'Harbour survey', needs: 0 }]

/** A thread's conversation, a plan with nested steps, a task that carries a plan with a doing step and a job, a
 *  suspended step, two asks on one task, a loose router ask, a to-do backlog, a wake due today, and a conversation at rest. */
function fixture() {
  const T = { thread: 'harbour-survey' }
  const views: WorkView[] = [
    view('exe_hv', 'conversation', 'running', { session_id: 'ses_hv', ...T }),
    view('exe_ak', 'task', 'needs_you', { session_id: 'ses_ak', parent: 'exe_hv', ...T }, 'needs_you', NOW - 30 * MIN),
    view('exe_tk', 'task', 'running', { session_id: 'ses_tk', parent: 'exe_hv', ...T }),
    view('act_jb', 'job', 'running', { session_id: 'ses_tk', parent: 'exe_tk', ...T, title: 'cargo test -p quays' }),
    view('wak_t', 'wake', 'waiting', { session_id: 'ses_hv', title: 'check the gauge', next_at_ms: NOW + 2 * HOUR }),
    view('exe_rs', 'conversation', 'ready', { session_id: 'ses_rs', ...T }),
  ]
  const records = [
    record('tsk_plan', { title: 'Survey the quays', state: 'in_progress', origin: { session: 'ses_hv' } as never }),
    record('tsk_s1', { title: 'Measure pier one', state: 'done', parent: 'tsk_plan', updated_at_ms: NOW - 30 * MIN, origin: { session: 'ses_hv' } as never }),
    record('tsk_s2', { title: 'Chart the shoals', state: 'accepted', parent: 'tsk_plan', origin: { session: 'ses_hv' } as never }),
    record('tsk_s3', { title: 'Sound the channel', state: 'accepted', parent: 'tsk_s2', origin: { session: 'ses_hv' } as never }),
    record('tsk_sus', { title: 'Paint the beacon', state: 'suspended', parent: 'tsk_plan', origin: { session: 'ses_hv' } as never }),
    record('tsk_blk', { title: 'Wait for the permit', state: 'blocked', parent: 'tsk_plan', origin: { session: 'ses_hv' } as never }),
    record('tsk_tk', { title: 'Lay the cable', state: 'in_progress', session: 'ses_tk', origin: { session: 'ses_hv' } as never }),
    record('tsk_do', { title: 'Pull the line', state: 'in_progress', parent: 'tsk_tk', origin: { session: 'ses_tk' } as never }),
    record('tsk_b1', { title: 'Order buoys', state: 'accepted', origin: { session: 'ses_hv' } as never }),
    record('tsk_b2', { title: 'Order rope', state: 'accepted', origin: { session: 'ses_hv' } as never }),
  ]
  const confirms: ConfirmRequest[] = [confirm('act_c1', { session_id: 'ses_ak' }), confirm('act_c2', { session_id: 'ses_ak', tool: 'fs.delete' })]
  const loose = { ask_id: 'ask_1', session_id: 'ses_elsewhere', question: 'Which harbour?', options: [], message: 'survey it', expires_ms: NOW + HOUR } as unknown as ThreadAsk
  const queue = queueOf({ confirms, views, records, asks: [{ ask: loose, heard_ms: NOW - 5 * MIN }] })
  return { views, records, queue }
}

const CALLS: ReadonlyArray<[string, OwnerCalls]> = [['the defaults', OWNER_CALLS], ['the recommended answers', RECOMMENDED]]
const BUCKETS: readonly Bucket[] = ['needs', 'working', 'paused', 'still', 'done']
const COLUMN = { needs: 'needs', working: 'working', paused: 'paused', still: 'scheduled', done: 'done' } as const
const cx = { now: NOW, spentToday: new Set<string>(), records: [] }

/** What a counter shows, from the rows it counts: one per card, or under Q1 B one per ask. */
const weight = (b: Bucket, calls: OwnerCalls) => (r: OpsRow): number => (b === 'needs' && !calls.needsPerCard ? r.asks : 1)
const sum = (rs: readonly OpsRow[], w: (r: OpsRow) => number) => rs.reduce((n, r) => n + w(r), 0)

for (const [name, calls] of CALLS) {
  test(`each counter is the counted cards a click keeps, the Graph's normal nodes and the Board's column, under ${name}`, () => {
    const { views, records, queue } = fixture()
    const rows = boardRows({ views, tasks: [], sessions: [], records, queue, now: NOW, past: false, calls })
    const counts = allCounters(rows, NOW, 0, calls)
    for (const b of BUCKETS) {
      const w = weight(b, calls)
      const kept = shown(rows, { bucket: b }, '', cx).rows
      const keptIds = new Set(kept.map((r) => r.id))
      const counter = counts[b === 'still' ? 'still' : b]
      assert.equal(counter, sum(kept.filter((r) => r.counted), w), `${b}: the counter is the counted kept cards`)
      // The Graph: a click keeps these and their context; the rest is hidden (Q4 A) or outlined (Q4 B).
      const ctx = contextOf(rows, keptIds)
      const g = layoutRows(rows, LANES, null, { now: NOW, future: true, keep: calls.hideRest ? new Set([...keptIds, ...ctx]) : undefined })
      const byId = new Map(rows.map((r) => [r.id, r]))
      const normal = g.nodes.filter((n) => keptIds.has(n.id)).map((n) => byId.get(n.id)!)
      assert.equal(sum(normal.filter((r) => r.counted), w), counter, `${b}: the counted nodes the Graph draws normally`)
      if (calls.hideRest) for (const n of g.nodes) assert.ok(keptIds.has(n.id) || ctx.has(n.id), `${b}: ${n.id} is neither kept nor context, and is drawn`)
      // The Board: its column, counted cards.
      const col = columnsOf(rows, NOW)
      const cards = b === 'done' ? col.done : col[COLUMN[b] as 'needs']
      assert.equal(sum(cards.filter((r) => r.counted), w), counter, `${b}: the counted cards in the Board's column`)
    }
  })
}

test('every graph node is a row of the work set, and every record is a row or rides on one', () => {
  const { views, records, queue } = fixture()
  const rows = boardRows({ views, tasks: [], sessions: [], records, queue, now: NOW, past: false })
  const g = layoutRows(rows, LANES, null, { now: NOW, future: true })
  const ids = new Set(rows.map((r) => r.id))
  for (const n of g.nodes) assert.ok(ids.has(n.id), `${n.id} is a row`)
  assert.equal(g.nodes.length, rows.length, 'every row is a node (with "+ future" on)')
  for (const t of records) assert.ok(rows.some((r) => r.recordId === t.id), `${t.id} is on a row`)
})

test('the needs counter under today’s answer is the Needs-you list’s length; under Q1 A it is the cards', () => {
  const { views, records, queue } = fixture()
  const b = boardRows({ views, tasks: [], sessions: [], records, queue, now: NOW, past: false })
  assert.equal(queue.length, 3, 'two approvals on one task and a loose router ask')
  assert.equal(allCounters(b, NOW, 0).needs, queue.length)
  const a = boardRows({ views, tasks: [], sessions: [], records, queue, now: NOW, past: false, calls: RECOMMENDED })
  assert.equal(allCounters(a, NOW, 0, RECOMMENDED).needs, 3, 'the task with two asks, the loose ask, and the blocked step')
  assert.equal(b.find((r) => r.id === 'ses_ak')!.asks, 2)
  assert.equal(b.find((r) => r.id === 'a:ask_1')!.kind, 'ask')
})

// ---- the owner's calls, proved by tests that pass the setting and fail without the code (join fixes of theseus-odj0m) ----

test('Q3: agents only uncounts what is drawn working, never a card in Needs: a doing step with a goal-change ask and a job that needs the owner stay counted', () => {
  const T = { thread: 'harbour-survey' }
  const views: WorkView[] = [
    view('exe_hv', 'conversation', 'running', { session_id: 'ses_hv', ...T }),
    view('act_jb', 'job', 'running', { session_id: 'ses_hv', parent: 'exe_hv', ...T, title: 'cargo test -p quays' }),
  ]
  const records = [
    record('tsk_g', { title: 'Chart the shoals', state: 'in_progress', origin: { session: 'ses_hv' } as never, proposal: { objective: 'chart the reefs too', by: 'ses_hv', card: 'act_px', base_version: 1, at_ms: NOW - 3 * MIN } as never }),
  ]
  const confirms: ConfirmRequest[] = [confirm('act_cj', { session_id: 'ses_nowhere', execution_id: 'act_jb' })]
  const queue = queueOf({ confirms, views, records })
  const rows = boardRows({ views, tasks: [], sessions: [], records, queue, now: NOW, past: false, calls: RECOMMENDED })
  const step = rows.find((r) => r.id === 'tsk_g')!
  const job = rows.find((r) => r.id === 'act_jb')!
  assert.deepEqual([step.kind, step.asks, step.bucket, step.column, step.counted], ['step', 1, 'needs', 'needs', true], 'a doing step carrying a goal-change ask, its carrier working')
  assert.deepEqual([job.kind, job.asks, job.bucket, job.column, job.counted], ['job', 1, 'needs', 'needs', true], 'a job that needs the owner')
  const needs = columnsOf(rows, NOW).needs
  assert.deepEqual(needs.map((r) => r.id).sort(), ['act_jb', 'tsk_g'], 'both sit in the Needs column')
  assert.equal(allCounters(rows, NOW, 0, RECOMMENDED).needs, needs.length, 'the needs counter is the Needs column, not one less (the 20:01 mismatch)')
})

test('Q1 A: a task with two asks is one needs card, and Q1 B counts both asks', () => {
  const T = { thread: 'harbour-survey' }
  const views: WorkView[] = [view('exe_ak', 'task', 'running', { session_id: 'ses_ak', ...T })]
  const queue = queueOf({ confirms: [confirm('act_c1', { session_id: 'ses_ak' }), confirm('act_c2', { session_id: 'ses_ak', tool: 'fs.delete' })], views })
  const count = (calls: OwnerCalls) => allCounters(boardRows({ views, tasks: [], sessions: [], queue, now: NOW, past: false, calls }), NOW, 0, calls).needs
  assert.equal(count({ ...RECOMMENDED, needsPerCard: false }), 2, 'Q1 B: one per ask')
  assert.equal(count(RECOMMENDED), 1, 'Q1 A: one per card, however many questions it holds')
  assert.equal(count({ ...OWNER_CALLS, needsPerCard: true }), 1, 'the one switch alone makes it so')
})

test('Q3: agents only drops the job and the steps an agent is already doing from Working, and nothing else', () => {
  const { views, records, queue } = fixture()
  const offCalls = { ...RECOMMENDED, agentsOnly: false }
  const build = (calls: OwnerCalls) => boardRows({ views, tasks: [], sessions: [], records, queue, now: NOW, past: false, calls })
  const off = build(offCalls)
  const on = build(RECOMMENDED)
  // The job, the step a running task is doing, and the plan its conversation is carrying out: all drawn working, none a second agent.
  const dropped = ['act_jb', 'tsk_do', 'tsk_plan']
  for (const id of dropped) {
    assert.equal(on.find((r) => r.id === id)!.bucket, 'working', `${id} is still drawn working`)
    assert.equal(off.find((r) => r.id === id)!.counted, true, `${id} counts when agents-only is off`)
    assert.equal(on.find((r) => r.id === id)!.counted, false, `${id} is not counted again`)
  }
  const counted = (rows: OpsRow[]) => rows.filter((r) => r.counted && r.bucket === 'working').map((r) => r.id).sort()
  assert.deepEqual(counted(off).filter((id) => !counted(on).includes(id)), [...dropped].sort(), 'exactly those three leave the count')
  assert.deepEqual(counted(on), ['ses_hv', 'ses_tk'], 'the agents themselves, the running conversation and task, still count')
  const working = (rows: OpsRow[], calls: OwnerCalls) => allCounters(rows, NOW, 0, calls).working
  assert.equal(working(off, offCalls) - working(on, RECOMMENDED), dropped.length, 'the Working counter falls by exactly those')
})
