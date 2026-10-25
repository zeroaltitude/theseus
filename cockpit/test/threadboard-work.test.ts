// The Threads lens over the one work set (theseus-brcs4): threadWork lists each thread's plans and steps with their
// attached tasks, a plan's n of m, and under a counter's filter only the kept cards with their ancestors as context.
import test from 'node:test'
import assert from 'node:assert/strict'
import type { ConfirmRequest, ThreadAsk, WorkView } from '../src/protocol.gen/index.ts'
import { allCounters, boardRows, contextOf, shown, type OpsRow } from '../src/ops/board.ts'
import { queueOf } from '../src/ops/queue.ts'
import { OWNER_CALLS, type OwnerCalls } from '../src/ops/state.ts'
import type { Bucket } from '../src/ops/address.ts'
import { threadWork, type ThreadWork, type WorkLine } from '../src/lib/threadboard.ts'
import { confirm, group, HOUR, MIN, NOW, record, view } from './ops-fixtures.ts'

/** Two threads and Home: a plan with nested steps, a task that carries a plan with a doing step and a job, a to-do
 *  backlog, a suspended step, three asks on one task, a loose ask, a conversation at rest and a wake in no thread. */
function fixture() {
  const H = { thread: 'harbour-survey' }
  const views: WorkView[] = [
    view('exe_hv', 'conversation', 'running', { session_id: 'ses_hv', ...H }),
    view('exe_ak', 'task', 'needs_you', { session_id: 'ses_ak', parent: 'exe_hv', ...H }, 'needs_you', NOW - 30 * MIN),
    view('exe_tk', 'task', 'running', { session_id: 'ses_tk', parent: 'exe_hv', ...H }),
    view('act_jb', 'job', 'running', { session_id: 'ses_tk', parent: 'exe_tk', ...H, title: 'cargo test -p quays' }),
    view('exe_rs', 'conversation', 'ready', { session_id: 'ses_rs', ...H }),
    view('exe_td', 'conversation', 'running', { session_id: 'ses_td', thread: 'tide-tables' }),
    view('exe_tt', 'task', 'running', { session_id: 'ses_tt', parent: 'exe_td', thread: 'tide-tables', title: 'Tabulate the tides' }),
    view('wak_h', 'wake', 'waiting', { session_id: 'ses_hv', title: 'check the gauge', next_at_ms: NOW + 2 * HOUR }),
  ]
  const o = { origin: { session: 'ses_hv' } as never }
  const records = [
    record('tsk_plan', { title: 'Survey the quays', state: 'in_progress', ...o }),
    record('tsk_s1', { title: 'Measure pier one', state: 'done', parent: 'tsk_plan', updated_at_ms: NOW - 30 * MIN, ...o }),
    record('tsk_s2', { title: 'Chart the shoals', state: 'accepted', parent: 'tsk_plan', ...o }),
    record('tsk_s3', { title: 'Sound the channel', state: 'accepted', parent: 'tsk_s2', ...o }),
    record('tsk_s4', { title: 'Mark the buoys', state: 'in_progress', parent: 'tsk_s2', ...o }),
    record('tsk_sus', { title: 'Paint the beacon', state: 'suspended', parent: 'tsk_plan', ...o }),
    record('tsk_tk', { title: 'Lay the cable', state: 'in_progress', session: 'ses_tk', ...o }),
    record('tsk_do', { title: 'Pull the line', state: 'in_progress', parent: 'tsk_tk', origin: { session: 'ses_tk' } as never }),
    record('tsk_b1', { title: 'Order buoys', state: 'accepted', ...o }),
    record('tsk_b2', { title: 'Order rope', state: 'accepted', ...o }),
  ]
  const confirms: ConfirmRequest[] = [
    confirm('act_c1', { session_id: 'ses_ak' }), confirm('act_c2', { session_id: 'ses_ak', tool: 'fs.delete' }), confirm('act_c3', { session_id: 'ses_ak', tool: 'fs.move' }),
  ]
  const loose = { ask_id: 'ask_1', session_id: 'ses_elsewhere', question: 'Which harbour?', options: [], message: 'survey it', expires_ms: NOW + HOUR } as unknown as ThreadAsk
  const queue = queueOf({ confirms, views, records, asks: [{ ask: loose, heard_ms: NOW - 5 * MIN }] })
  const groups = [group('harbour-survey'), group('tide-tables')]
  const rows = boardRows({ views, tasks: [], sessions: [], records, queue, now: NOW, past: false })
  return { rows, groups }
}

const lineOf = (w: ThreadWork[], id: string): WorkLine => {
  const l = w.flatMap((t) => t.lines).find((x) => x.row.id === id)
  assert.ok(l, `${id} is listed`)
  return l
}
const slugs = (w: ThreadWork[]) => w.map((t) => t.group?.slug ?? 'home')
const idsOf = (t: ThreadWork | undefined) => (t?.lines ?? []).map((l) => l.row.id)

test('the shape: a thread lists its plans and steps with their tasks on the same line, and Home holds the threadless work', () => {
  const { rows, groups } = fixture()
  const w = threadWork(rows, groups)
  assert.deepEqual(slugs(w), ['harbour-survey', 'tide-tables', 'home'])
  const all = w.flatMap((t) => t.lines.map((l) => l.row.id))
  assert.equal(new Set(all).size, all.length, 'every card is listed once')
  assert.equal(all.length, rows.length, 'and every card of the work set is listed')
  const harbour = w[0]
  assert.ok(idsOf(harbour).includes('tsk_plan') && idsOf(harbour).includes('tsk_s3'), 'the plan and its nested step are under the thread')
  assert.ok(!idsOf(w[1]).includes('tsk_plan'), 'the other thread does not list it')
  const home = w[2]
  assert.ok(idsOf(home).includes('wak_h'), 'a wake in no thread is Home’s')
  assert.ok(idsOf(home).includes('a:ask_1'), 'so is the loose ask')
  // The task that carries a record is one line: the task is on the card, never beside it.
  const cable = lineOf(w, 'ses_tk')
  assert.equal(cable.row.recordId, 'tsk_tk')
  assert.equal(cable.task, cable.row)
  assert.equal(w.flatMap((t) => t.lines).filter((l) => l.row.recordId === 'tsk_tk').length, 1)
  assert.equal(lineOf(w, 'tsk_do').depth, cable.depth + 1, 'its doing step is nested under it')
  assert.equal(lineOf(w, 'act_jb').depth, cable.depth + 1, 'and its job')
  // A step nobody holds a task for says who carries it.
  assert.equal(lineOf(w, 'tsk_s3').task, undefined)
  assert.equal(lineOf(w, 'tsk_s3').carriedBy, 'carried by exe_hv')
})

test('progress: a plan’s n of m and its words, a nested plan’s subtree counted once', () => {
  const { rows, groups } = fixture()
  const w = threadWork(rows, groups)
  // Survey the quays: s1 done, s3 to do, s4 doing (its carrier, the chat, is working), suspended paused.
  const plan = lineOf(w, 'tsk_plan').progress!
  assert.equal(plan.total, 4, 'four leaf steps below it, the nested plan’s two once each')
  assert.equal(plan.done, 1)
  assert.equal(plan.words, '1 working · 1 paused · 1 to do · 1 done')
  const inner = lineOf(w, 'tsk_s2').progress!
  assert.deepEqual([inner.done, inner.total, inner.words], [0, 2, '1 working · 1 to do'])
  assert.equal(lineOf(w, 'ses_tk').progress!.total, 1, 'a task carrying a plan counts its own step')
  assert.equal(lineOf(w, 'tsk_s3').progress, undefined, 'a step has no n of m')
})

test('Q1: a card with three asks is one line with one needs mark; the loose ask is one more', () => {
  const { rows, groups } = fixture()
  assert.equal(rows.find((r) => r.id === 'ses_ak')!.asks, 3)
  const w = threadWork(rows, groups)
  assert.equal(w.flatMap((t) => t.lines).filter((l) => l.row.id === 'ses_ak').length, 1)
  assert.equal(lineOf(w, 'ses_ak').needs, 1)
  assert.equal(lineOf(w, 'a:ask_1').needs, 1)
  assert.equal(w[0].needs, 1, 'the thread’s mark counts the card once')
  // The setting decides: asked per ask, the same card would be three.
  const per: OwnerCalls = { ...OWNER_CALLS, needsPerCard: false }
  assert.equal(lineOf(threadWork(rows, groups, { calls: per }), 'ses_ak').needs, 3)
})

test('Q2: a to-do step is inside its plan’s n of m and in no paused·queued', () => {
  const { rows, groups } = fixture()
  const w = threadWork(rows, groups)
  const todo = lineOf(w, 'tsk_s3')
  assert.equal(todo.row.bucket, 'todo')
  assert.equal(lineOf(w, 'tsk_s2').progress!.by.todo, 1)
  assert.equal(lineOf(w, 'tsk_s2').progress!.by.paused, undefined)
  const kept = new Set(shown(rows, { bucket: 'paused' }, '', { now: NOW, spentToday: new Set(), records: [] }).rows.map((r) => r.id))
  assert.ok(!kept.has('tsk_s3') && kept.has('tsk_sus'))
  const f = threadWork(rows, groups, { kept, context: contextOf(rows, kept) })
  assert.ok(!f.flatMap((t) => t.lines).some((l) => l.row.id === 'tsk_s3'), 'the lens does not list it under paused·queued')
  // With the call the other way it reads paused, in the same plan.
  const apart: OwnerCalls = { ...OWNER_CALLS, todoApart: false }
  const old = boardRows({ views: [], tasks: [], sessions: [], records: [record('tsk_p', { state: 'in_progress' }), record('tsk_x', { state: 'accepted', parent: 'tsk_p' })], now: NOW, past: true, calls: apart })
  assert.equal(threadWork(old, [], { calls: apart }).flatMap((t) => t.lines).find((l) => l.row.id === 'tsk_p')!.progress!.by.paused, 1)
})

const BUCKETS: readonly Bucket[] = ['needs', 'working', 'paused', 'still', 'done']
const cx = { now: NOW, spentToday: new Set<string>(), records: [] }

test('the filter: a thread is listed only when it holds a kept card, with its ancestors as context, and the counted cards equal the counter', () => {
  const { rows, groups } = fixture()
  const counts = allCounters(rows, NOW, 0)
  for (const b of BUCKETS) {
    const keptRows = shown(rows, { bucket: b }, '', cx).rows
    const kept = new Set(keptRows.map((r) => r.id))
    const w = threadWork(rows, groups, { kept, context: contextOf(rows, kept) })
    for (const t of w) assert.ok(t.lines.some((l) => !l.context), `${b}: ${t.group?.slug ?? 'home'} holds a kept card`)
    const lines = w.flatMap((t) => t.lines)
    for (const l of lines) assert.ok(l.context || kept.has(l.row.id), `${b}: ${l.row.id} is kept or faint`)
    // Every kept card is listed once, and a listed ancestor sits above its kept child.
    assert.deepEqual(lines.filter((l) => !l.context).map((l) => l.row.id).sort(), [...kept].sort(), `${b}: all the kept cards, once`)
    const counted = lines.filter((l) => !l.context && l.row.counted)
    const n = b === 'needs' ? counted.reduce((m, l) => m + l.needs, 0) : counted.length
    assert.equal(n, counts[b], `${b}: the counted cards listed equal the counter (${counts[b]})`)
  }
  const needs = threadWork(rows, groups, { kept: new Set(['tsk_s4']), context: new Set() })
  assert.deepEqual(slugs(needs), ['harbour-survey'], 'a thread with no kept card is not listed, Home included')
  assert.deepEqual(idsOf(needs[0]), ['ses_hv', 'tsk_plan', 'tsk_s2', 'tsk_s4'], 'a kept step is never drawn without the plans it sits under')
  assert.deepEqual(needs[0].lines.map((l) => l.context), [true, true, true, false])
})

test('a parent cycle is listed, once each, instead of vanishing from the lens (theseus-brcs4 F4)', () => {
  const o = { origin: { session: 'ses_hv' } as never }
  const views: WorkView[] = [view('exe_hv', 'conversation', 'running', { session_id: 'ses_hv', thread: 'harbour-survey' })]
  const records = [
    record('tsk_c1', { title: 'Hoist the first', state: 'in_progress', parent: 'tsk_c2', ...o }),
    record('tsk_c2', { title: 'Hoist the second', state: 'in_progress', parent: 'tsk_c1', ...o }),
  ]
  const rows = boardRows({ views, tasks: [], sessions: [], records, now: NOW, past: false })
  const groups = [group('harbour-survey')]
  const lines = threadWork(rows, groups).flatMap((t) => t.lines)
  assert.equal(lines.length, rows.length, 'no row is dropped: the Graph draws every item whatever its tree')
  const ids = lines.map((l) => l.row.id)
  assert.deepEqual([...new Set(ids)].sort(), [...ids].sort(), 'and none is listed twice')
  for (const id of ['tsk_c1', 'tsk_c2']) assert.ok(ids.includes(id), `${id} is listed`)
})
