// The nouns' forms on the Graph and the Board (theseus-hl0r1): the step's glyph, a plan's progress, the task as a badge,
// escalation in place, folds and the cap, and the counter filter through them. Pure: rows in, data out.
import test from 'node:test'
import assert from 'node:assert/strict'
import type { TaskRecord, WorkView } from '../src/protocol.gen/index.ts'
import { boardRows, type OpsRow } from '../src/ops/board.ts'
import { formsOf, progressOf, stepLook, STEP_GLYPH, type StepWord } from '../src/ops/forms.ts'
import { labelHeight, layoutRows, NODE_CAP, type Graph, type LaneMeta } from '../src/ops/graph.ts'
import { queueOf } from '../src/ops/queue.ts'
import { confirm, NOW, record, view } from './ops-fixtures.ts'

const LANES: LaneMeta[] = [{ key: 'harbour-survey', title: 'Harbour survey', needs: 0, head: 'ses_home' }]
const MIN = 60_000
const rec = (id: string, state: string, over: Partial<TaskRecord> = {}) => record(id, { state, updated_at_ms: NOW - 5 * MIN, ...over } as Partial<TaskRecord>)

/** A thread, a plan with nested steps carried by a running task (with a job), a suspended step, a step with an ask on
 *  it, a loose ask, a to-do backlog and a wake due today. */
function fixture() {
  const views: WorkView[] = [
    view('exe_home', 'conversation', 'ready', { session_id: 'ses_home', thread: 'harbour-survey' }),
    view('exe_side', 'conversation', 'ready', { session_id: 'ses_side', thread: 'harbour-survey' }),
    view('exe_t2', 'task', 'running', { session_id: 'ses_t2', title: 'Survey the quay', thread: 'harbour-survey', parent: 'exe_home', moved_to: 'sonnet' }),
    view('act_j1', 'job', 'running', { session_id: 'ses_t2', parent: 'exe_t2', thread: 'harbour-survey', title: 'cargo test -p quay' }),
    view('wak_r1', 'wake', 'waiting', { session_id: 'ses_home', title: 'check the gauge', next_at_ms: NOW + 2 * 3_600_000 }),
  ]
  const records: TaskRecord[] = [
    rec('tsk_plan', 'in_progress', { title: 'Survey plan', session: 'ses_t2' }),
    rec('tsk_s1', 'in_progress', { title: 'Measure pier one', parent: 'tsk_plan' }),
    rec('tsk_s2', 'accepted', { title: 'Measure pier two', parent: 'tsk_plan' }),
    rec('tsk_s3', 'done', { title: 'Order tape', parent: 'tsk_plan' }),
    rec('tsk_sub', 'accepted', { title: 'Write it up', parent: 'tsk_plan' }),
    rec('tsk_n1', 'accepted', { title: 'Draft the chart', parent: 'tsk_sub' }),
    rec('tsk_n2', 'done', { title: 'Pick the scale', parent: 'tsk_sub' }),
    rec('tsk_susp', 'suspended', { title: 'Resume the dredge' }),
    rec('tsk_blk', 'blocked', { title: 'Wait for the harbourmaster' }),
    rec('tsk_ask', 'in_progress', { title: 'Buy the permit' }),
    rec('tsk_fail', 'failed', { title: 'Reach the tide office' }),
    rec('tsk_ab', 'abandoned', { title: 'Paint the buoy' }),
    rec('tsk_td1', 'accepted', { title: 'Backlog one' }),
    rec('tsk_td2', 'proposed', { title: 'Backlog two' }),
  ]
  const queue = queueOf({
    confirms: [confirm('act_c1', { session_id: 'ses_nowhere' })], views,
    asks: [{ ask: { ask_id: 'ask_9', session_id: 'ses_nowhere', question: 'Which tide?', options: [], message: 'm', expires_ms: NOW + 3_600_000 } as never, heard_ms: NOW }],
  })
  return { views, records, queue }
}

const rowsOfFixture = (extra: { records?: TaskRecord[]; queue?: ReturnType<typeof queueOf> } = {}): OpsRow[] => {
  const f = fixture()
  return boardRows({ views: f.views, tasks: [], sessions: [], records: extra.records ?? f.records, queue: extra.queue ?? f.queue, now: NOW })
}
const lay = (rows: readonly OpsRow[], prev: Graph | null = null, opts = {}) => layoutRows(rows, LANES, prev, { now: NOW, ...opts })
const node = (g: Graph, id: string) => g.nodes.find((n) => n.id === id)

test('the step glyph is the one its bucket names, for each of the eight states; to do is ink-dim and not faded', () => {
  const rows = rowsOfFixture()
  const look = (id: string) => { const r = rows.find((x) => x.id === id)!; return stepLook(r.bucket, r.record?.state ?? r.state) }
  const want: Record<string, [StepWord, string]> = {
    tsk_td1: ['todo', '☐'], tsk_s1: ['doing', '◐'], tsk_blk: ['blocked', '■'], tsk_susp: ['paused', '⏸'],
    tsk_s3: ['done', '☑'], tsk_ab: ['abandoned', '⊘'], tsk_fail: ['failed', '✗'],
  }
  for (const [id, [word, glyph]] of Object.entries(want)) { assert.equal(look(id).word, word, id); assert.equal(look(id).glyph, glyph, id) }
  assert.equal(stepLook('needs', 'accepted').glyph, '●', 'a step that needs the owner')
  assert.deepEqual(Object.values(STEP_GLYPH), ['☐', '◐', '■', '●', '⏸', '☑', '⊘', '✗'])
  const todo = look('tsk_td1')
  assert.equal(todo.tone, 'ink-dim', 'to do is ink-dim')
  assert.equal(todo.faded, false, 'to do is not faded')
  assert.equal(look('tsk_susp').faded, false, 'a suspended step is open, not faded')
  for (const id of ['tsk_s3', 'tsk_ab', 'tsk_fail']) assert.equal(look(id).faded, true, `${id}: only ended work is faded`)
  assert.equal(look('tsk_s1').tone, 'live', 'a step being done wears the live tone')
})

test('progressOf: n of m and the per-bucket summary; a nested plan counts its subtree once', () => {
  const rows = rowsOfFixture()
  const p = progressOf(rows, 'ses_t2')
  // Leaves: s1 doing, s2 to do, s3 done, n1 to do, n2 done. The nested plan `tsk_sub` is not a unit of its own.
  assert.equal(p.m, 5)
  assert.equal(p.n, 2)
  assert.equal(p.counts.working, 1)
  assert.equal(p.counts.todo, 2)
  assert.equal(p.summary, '1 working · 2 to do · 2 done')
  const sub = progressOf(rows, 'tsk_sub')
  assert.deepEqual([sub.n, sub.m], [1, 2])
  assert.equal(progressOf(rows, 'tsk_td1').m, 0, 'a step with no children has nothing to count')
})

test('the attached task is a badge, not a node: its glyph, model chip and progress line hang on the plan it carries out', () => {
  const rows = rowsOfFixture()
  const g = lay(rows)
  assert.ok(!g.nodes.some((n) => n.id === 'tsk_plan'), 'the record is not a second node beside its task')
  assert.equal(g.nodes.filter((n) => n.form?.badge).length, 1, 'one carried task, one badge')
  const plan = node(g, 'ses_t2')!
  assert.equal(plan.form!.form, 'plan', 'the card the task carries out is a plan')
  const b = plan.form!.badge!
  assert.equal(b.kind, 'task')
  assert.equal(b.word, 'task')
  assert.equal(b.glyph, '◐')
  assert.equal(b.model, 'sonnet')
  assert.ok(b.running)
  assert.equal(g.nodes.filter((n) => n.id === 'ses_t2' || n.record?.id === 'tsk_plan').length, 1)
  assert.ok(node(g, 'act_j1'), 'the job stays a pill beside the card that runs it')
  assert.equal(node(g, 'act_j1')!.form!.form, 'job')
  // Every row is a node, once: the parity the counters hold.
  const ids = g.nodes.map((n) => n.id)
  assert.equal(new Set(ids).size, ids.length)
})

test('a claim draws as "worked by ‹title›" on the step it holds', () => {
  const f = fixture()
  const rows = boardRows({ views: f.views, tasks: [], sessions: [], queue: f.queue, now: NOW,
    records: [...f.records.filter((r) => r.id !== 'tsk_s1'), rec('tsk_s1', 'in_progress', { title: 'Measure pier one', parent: 'tsk_plan', claim: { session: 'ses_t2', execution: 'exe_t2', until_ms: NOW + 10 * MIN } as never })] })
  const badge = formsOf(rows, NOW).get('tsk_s1')!.badge!
  assert.equal(badge.kind, 'claim')
  assert.equal(badge.by, 'Survey the quay')
})

test('escalation in place: a step with a child listed under it is a plan, with the same id, history and slot', () => {
  const f = fixture()
  const before = boardRows({ views: f.views, tasks: [], sessions: [], records: f.records, queue: f.queue, now: NOW })
  const g0 = lay(before)
  const was = node(g0, 'tsk_ask')!
  assert.equal(was.form!.form, 'step')
  assert.ok(was.form!.progress === undefined)
  const kid = rec('tsk_ask1', 'accepted', { title: 'Fill in the form', parent: 'tsk_ask', created_at_ms: NOW - 2 * 3_600_000 })
  const after = boardRows({ views: f.views, tasks: [], sessions: [], records: [...f.records, kid], queue: f.queue, now: NOW })
  const g1 = lay(after, g0)
  const is = node(g1, 'tsk_ask')!
  assert.equal(is.form!.form, 'plan', 'it morphed')
  assert.equal(is.id, was.id, 'the same id')
  assert.deepEqual([is.x, is.y], [was.x, was.y], 'the same slot: it does not jump')
  assert.equal(is.startedMs, was.startedMs, 'the same history')
  assert.equal(is.record!.id, was.record!.id)
  assert.equal(is.record!.created_at_ms, was.record!.created_at_ms)
  assert.deepEqual(is.form!.progress && [is.form!.progress.n, is.form!.progress.m], [0, 1], 'drawn from its first listed child')
  assert.ok(!is.fresh, 'not a new card: no fade-in')
  // The selection names the id, and still finds the card.
  assert.equal(g1.nodes.find((n) => n.id === 'tsk_ask' || n.record?.id === 'tsk_ask')!.id, 'tsk_ask')
})

test('folding: a plan opens while it is open and folds when it ends; a folded plan is one node toward the cap', () => {
  const f = fixture()
  const open = lay(boardRows({ views: f.views, tasks: [], sessions: [], records: f.records, queue: f.queue, now: NOW }))
  assert.equal(node(open, 'tsk_sub')!.open, true, 'a plan with open work is open')
  assert.ok(node(open, 'tsk_n1'), 'its children are drawn')
  const ended = f.records.map((r) => (r.id === 'tsk_sub' || r.id === 'tsk_n1' ? { ...r, state: 'done' } as TaskRecord : r))
  const done = lay(boardRows({ views: f.views, tasks: [], sessions: [], records: ended, queue: f.queue, now: NOW }))
  assert.equal(node(done, 'tsk_sub')!.open, false, 'a plan folds when it ends')
  assert.ok(!node(done, 'tsk_n1') && !node(done, 'tsk_n2'), 'its children stand in it')
  // The owner's fold wins either way.
  const unfolded = lay(boardRows({ views: f.views, tasks: [], sessions: [], records: ended, queue: f.queue, now: NOW }), null, { folds: new Map([['tsk_sub', true]]) })
  assert.ok(node(unfolded, 'tsk_n1'), 'opened by the owner')
  const refolded = lay(boardRows({ views: f.views, tasks: [], sessions: [], records: f.records, queue: f.queue, now: NOW }), null, { folds: new Map([['tsk_sub', false]]) })
  assert.ok(!node(refolded, 'tsk_n1'), 'folded by the owner')
  assert.equal(node(refolded, 'tsk_sub')!.form!.progress!.summary, '1 to do · 1 done')

  // A plan of 30 recent children, folded, is one node: the cap folds nothing for it.
  const kids = Array.from({ length: 30 }, (_, i) => rec(`tsk_k${i}`, 'done', { title: `Leaf ${i}`, parent: 'tsk_big', updated_at_ms: NOW - 2 * MIN }))
  const bigRows = boardRows({ views: [], tasks: [], sessions: [], records: [rec('tsk_big', 'done', { title: 'Big plan', updated_at_ms: NOW - 2 * MIN }), ...kids], now: NOW })
  const small = lay(bigRows, null, { cap: 5 })
  assert.equal(small.nodes.length, 1, 'the folded plan alone')
  assert.equal(small.folded, 0, 'the cap folded none of its children: they were never counted')
  assert.ok(!small.nodes.some((n) => n.kind === 'folded'))
  assert.equal(NODE_CAP, 400, 'the cap is as it was')
  // Open, the same plan does hit the cap.
  const wide = lay(bigRows, null, { cap: 5, folds: new Map([['tsk_big', true]]) })
  assert.equal(wide.folded, 30, 'open, its children count against the cap')
})

test('a folded plan holding a needs card still draws that child and says "● 1 needs you"', () => {
  const f = fixture()
  const records = f.records.map((r) => (r.id === 'tsk_n1' ? { ...r, state: 'waiting_human' } as TaskRecord : r))
  const rows = boardRows({ views: f.views, tasks: [], sessions: [], records, queue: f.queue, now: NOW })
  const g = lay(rows, null, { folds: new Map([['tsk_sub', false]]) })
  assert.equal(node(g, 'tsk_sub')!.open, false)
  assert.ok(node(g, 'tsk_n1'), 'the card that needs the owner is drawn anyway')
  assert.ok(!node(g, 'tsk_n2'), 'the rest stand in the plan')
  const p = node(g, 'tsk_sub')!.form!.progress!
  assert.equal(p.needs, 1)
  assert.ok(p.summary.startsWith('1 needs you'))
})

test('the counter filter through the forms: a collapsed plan holding a kept card opens, an empty lane shrinks to its head, kept nodes keep their slots', () => {
  const f = fixture()
  const rows = boardRows({ views: f.views, tasks: [], sessions: [], records: f.records, queue: f.queue, now: NOW })
  const lanes: LaneMeta[] = [...LANES, { key: 'tide-tables', title: 'Tide tables', needs: 0 }]
  const folds = new Map([['tsk_sub', false], ['ses_t2', false]])
  const full = layoutRows(rows, lanes, null, { now: NOW, folds })
  assert.ok(!node(full, 'tsk_n1'), 'collapsed without a filter')
  const keep = new Set(['tsk_n1', 'tsk_sub', 'ses_t2', 'ses_home'])
  const kept = layoutRows(rows, lanes, full, { now: NOW, folds, keep, filter: keep })
  assert.ok(node(kept, 'tsk_n1'), 'the plan holding a kept card opens for the filter')
  assert.equal(node(kept, 'tsk_sub')!.open, true)
  assert.equal(node(kept, 'ses_t2')!.open, true)
  for (const id of ['ses_home', 'ses_t2']) assert.deepEqual([node(kept, id)!.x, node(kept, id)!.y], [node(full, id)!.x, node(full, id)!.y], `${id} keeps its slot`)
  assert.ok(!node(kept, 'tsk_s1'), 'what is not kept is left out')
  const empty = kept.lanes.find((l) => l.key === 'tide-tables')!
  assert.equal(empty.rows, 0, 'a lane with no kept card draws no row')
  assert.equal(empty.height, labelHeight(empty), 'and shrinks to its gutter label, never below it (theseus-6ln2q)')
  // theseus-hl0r1 F3: retiring something narrows what is drawn (`keep`), and must not unfold the owner's plan. Only
  // the counter's own set (`filter`) does that, so a drawn set with no filter leaves a folded plan folded.
  const retired = layoutRows(rows, lanes, null, { now: NOW, folds: new Map([['tsk_sub', false]]), keep: new Set(rows.map((r) => r.id)) })
  assert.equal(node(retired, 'tsk_sub')!.open, false, 'retiring something does not unfold the owner\u2019s plan')
})

test('the thread’s conversation is its lane’s head; a loose ask and a task with no record keep their forms', () => {
  const g = lay(rowsOfFixture())
  assert.equal(node(g, 'ses_home')!.head, true)
  assert.equal(node(g, 'ses_side')!.head, undefined, 'a second conversation in the thread is not its head (theseus-hl0r1 F1)')
  assert.equal(node(g, 'ses_home')!.form!.form, 'anchor')
  assert.equal(node(g, 'ses_home')!.form!.progress!.m > 0, true, 'its plan-mode n/m')
  const ask = g.nodes.find((n) => n.kind === 'ask')!
  assert.equal(ask.form!.form, 'ask')
  assert.ok(ask.needs)
  assert.equal(node(g, 'tsk_td1')!.form!.form, 'step')
})

test('the Board’s plan card: its direct children with their glyphs, and the same progress as the Graph', () => {
  const rows = rowsOfFixture()
  const f = formsOf(rows, NOW).get('ses_t2')!
  assert.equal(f.form, 'plan')
  assert.deepEqual(f.kids!.map((k) => [k.id, k.look.glyph]), [['tsk_s1', '◐'], ['tsk_s2', '☐'], ['tsk_s3', '☑'], ['tsk_sub', '☐']])
  assert.deepEqual(f.progress, progressOf(rows, 'ses_t2'), 'the Board and the Graph read one progress')
  const step = formsOf(rows, NOW).get('tsk_td1')!
  assert.equal(step.form, 'step')
  assert.equal(step.kids, undefined, 'a step has no children list')
})

test('an ended card standing in its folded plan does not also join the lane’s “✓ N done” fold (theseus-hl0r1 F6)', () => {
  const f = fixture()
  const stale = NOW - 2 * 3_600_000
  const records = f.records.map((r) => (r.id === 'tsk_n1' || r.id === 'tsk_n2' ? { ...r, state: 'done', updated_at_ms: stale } as TaskRecord : r))
  const rows = boardRows({ views: f.views, tasks: [], sessions: [], records, queue: f.queue, now: NOW })
  const g = lay(rows, null, { folds: new Map([['tsk_sub', false]]) })
  assert.equal(node(g, 'tsk_sub')!.open, false, 'the plan is folded by the owner')
  assert.ok(!node(g, 'tsk_n1') && !node(g, 'tsk_n2'), 'its children stand in it')
  assert.ok(!g.nodes.some((n) => n.id === 'done:harbour-survey'), 'and not in a second “✓ N done” fold of the lane')
  assert.ok(!g.edges.some((e) => e.source.startsWith('done:') || e.target.startsWith('done:')), 'no edge resolves to a done fold')
})
