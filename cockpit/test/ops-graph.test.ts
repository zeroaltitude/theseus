// The Graph lens's layout (theseus-jid7t.4): lanes per thread, needs-you first, and a home lane; columns by depth;
// positions sticky by id under live changes; ended work folded at an hour; the 400-node cap folding subtrees; the
// three kinds of edge; jobs beside their session; "+ future" nodes only when asked.
import test from 'node:test'
import assert from 'node:assert/strict'
import type { WorkView } from '../src/protocol.gen/index.ts'
import { COL_W, GUTTER, HOME, NODE_CAP, opsLayout, viewsOf, type Graph, type LaneMeta } from '../src/ops/graph.ts'
import { boardRows } from '../src/ops/board.ts'
import { board, HOUR, MIN, NOW, record, view } from './ops-fixtures.ts'

const LANES: LaneMeta[] = [{ key: 'harbour-survey', title: 'Harbour survey', needs: 0 }, { key: 'tide-tables', title: 'Tide tables', needs: 0 }]
const lay = (views: readonly WorkView[], prev: Graph | null = null, records = [] as ReturnType<typeof record>[], opts = {}) => opsLayout(views, records, LANES, prev, { now: NOW, ...opts })
const where = (g: Graph) => Object.fromEntries(g.nodes.map((n) => [n.id, `${n.x},${n.y}`]))
const node = (g: Graph, id: string) => g.nodes.find((n) => n.id === id)

test('lanes: one per live thread, needs-you first, then home for the rest; columns by depth', () => {
  const g = lay(board().views)
  assert.deepEqual(g.lanes.map((l) => l.key), ['harbour-survey', 'tide-tables', HOME], 'harbour-survey holds the question: it leads')
  const quiet = opsLayout(board().views.filter((v) => v.thread !== 'harbour-survey'), [], LANES, null, { now: NOW })
  assert.deepEqual(quiet.lanes.map((l) => l.key), ['harbour-survey', 'tide-tables', HOME], 'an empty live lane is still a lane (a drop target)')
  const needsLater = opsLayout(board().views, [], [...LANES].reverse(), null, { now: NOW })
  assert.equal(needsLater.lanes[0].key, 'harbour-survey', 'the lane that needs the owner comes first, whatever the order given')
  assert.equal(node(g, 'ses_home')!.lane, 'harbour-survey')
  assert.equal(node(g, 'ses_home')!.col, 0, 'a conversation anchors its lane at the left')
  assert.equal(node(g, 'ses_t1')!.lane, 'harbour-survey')
  assert.equal(node(g, 'ses_t3')!.lane, HOME, 'work in no thread is in the home lane')
  assert.equal(node(g, 'ses_tides')!.x, GUTTER)
  assert.equal(node(g, 'act_j1')!.x, GUTTER + COL_W, 'one column right of its session')
  assert.ok(!g.nodes.some((n) => n.id === 'act_q1'), 'a question is a badge, not a node')
  assert.equal(node(g, 'ses_t1')!.asks, 1, 'its question is a badge on its owner')
  assert.ok(node(g, 'ses_t1')!.needs)
})

test('rows by start time within a column', () => {
  const views = [
    view('exe_c', 'conversation', 'ready', { session_id: 'ses_c', thread: 'tide-tables' }),
    view('exe_b', 'task', 'running', { session_id: 'ses_b', parent: 'exe_c', thread: 'tide-tables', started_at_ms: NOW - 10 * MIN }),
    view('exe_a', 'task', 'running', { session_id: 'ses_a', parent: 'exe_c', thread: 'tide-tables', started_at_ms: NOW - 50 * MIN }),
  ]
  const g = lay(views)
  assert.ok(node(g, 'ses_a')!.y < node(g, 'ses_b')!.y, 'the earlier start is higher')
  assert.equal(node(g, 'ses_a')!.col, 1)
})

test('positions are sticky by id: a node added, one changed, one removed, and no other node moves', () => {
  const first = lay(board().views)
  const before = where(first)
  // Added: a new task under the running conversation.
  const added = lay([...board().views, view('exe_new', 'task', 'running', { session_id: 'ses_new', parent: 'exe_tides', thread: 'tide-tables', started_at_ms: NOW - 3 * HOUR })], first)
  for (const [id, at] of Object.entries(before)) assert.equal(where(added)[id], at, `${id} moved when a node was added`)
  assert.ok(node(added, 'ses_new')!.fresh, 'the new node fades in')
  assert.ok(!node(added, 'ses_tides')!.fresh)
  // Even though it started first, it appends: a live change never reshuffles.
  assert.ok(node(added, 'ses_new')!.y >= node(added, 'act_j1')!.y)
  // Changed: a task's state moves on, and a lane comes to need the owner.
  const changed = lay(board().views.map((v) => (v.id === 'exe_t2' ? { ...v, state: 'needs_you', attention: { ...v.attention, level: 'needs_you' } } as WorkView : v)), added)
  for (const [id, at] of Object.entries(where(added))) if (id !== 'ses_new') assert.equal(where(changed)[id], at, `${id} moved when a state changed`)
  assert.deepEqual(changed.lanes.map((l) => l.key), added.lanes.map((l) => l.key), 'the lanes keep their order')
  // Removed: the job ends and leaves; its hole stays.
  const removed = lay(board().views.filter((v) => v.id !== 'act_j1'), first)
  for (const [id, at] of Object.entries(where(removed))) assert.equal(at, before[id], `${id} moved when a node left`)
  // A new lane goes below; afresh, the order is needs-you first again.
  const more = opsLayout(board().views, [], [...LANES, { key: 'chart-room', title: 'Chart room', needs: 3 }], first, { now: NOW })
  assert.equal(more.lanes.at(-1)!.key, 'chart-room')
  for (const [id, at] of Object.entries(before)) assert.equal(where(more)[id], at, `${id} moved when a lane came`)
})

test('ended work older than an hour folds into one “✓ N done” per lane, which opens', () => {
  const views = [
    view('exe_c', 'conversation', 'ready', { session_id: 'ses_c', thread: 'tide-tables' }),
    view('exe_old1', 'task', 'done', { session_id: 'ses_old1', parent: 'exe_c', thread: 'tide-tables', ended_at_ms: NOW - 61 * MIN }, 'idle'),
    view('exe_old2', 'task', 'failed', { session_id: 'ses_old2', parent: 'exe_c', thread: 'tide-tables', ended_at_ms: NOW - 3 * HOUR }, 'idle'),
    view('exe_new', 'task', 'done', { session_id: 'ses_new', parent: 'exe_c', thread: 'tide-tables', ended_at_ms: NOW - 59 * MIN }, 'idle'),
    view('exe_ask', 'task', 'failed', { session_id: 'ses_ask', parent: 'exe_c', thread: 'tide-tables', ended_at_ms: NOW - 3 * HOUR }, 'needs_you'),
  ]
  const g = lay(views)
  const done = node(g, 'done:tide-tables')!
  assert.equal(done.kind, 'done')
  assert.equal(done.title, '✓ 2 done')
  assert.deepEqual([...done.members!].sort(), ['ses_old1', 'ses_old2'])
  assert.ok(node(g, 'ses_new'), 'ended under an hour ago: still its own node')
  assert.ok(node(g, 'ses_ask'), 'a failure that asks for the owner never folds')
  assert.ok(g.edges.some((e) => e.source === 'ses_c' && e.target === 'done:tide-tables'), 'its edges go to the fold')
  const open = lay(views, g, [], { expanded: new Set(['tide-tables']) })
  assert.ok(node(open, 'ses_old1') && node(open, 'ses_old2') && !node(open, 'done:tide-tables'))
})

test('past the cap, subtrees fold, the deepest and oldest first, and never one that needs the owner', () => {
  const views: WorkView[] = [view('exe_c', 'conversation', 'running', { session_id: 'ses_c', thread: 'tide-tables' })]
  // 60 parents under the conversation, each with 8 children: 541 nodes.
  for (let p = 0; p < 60; p++) {
    views.push(view(`exe_p${p}`, 'task', 'running', { session_id: `ses_p${p}`, parent: 'exe_c', thread: 'tide-tables', started_at_ms: NOW - HOUR + p * MIN }))
    for (let k = 0; k < 8; k++) views.push(view(`exe_p${p}k${k}`, 'task', p === 0 && k === 3 ? 'needs_you' : 'running', { session_id: `ses_p${p}k${k}`, parent: `exe_p${p}`, thread: 'tide-tables' }, p === 0 && k === 3 ? 'needs_you' : undefined))
  }
  const g = lay(views)
  assert.ok(g.nodes.length <= NODE_CAP, `${g.nodes.length} nodes`)
  assert.ok(g.folded > 0)
  const folds = g.nodes.filter((n) => n.kind === 'folded')
  assert.ok(folds.length > 0)
  assert.equal(folds[0].title, '+8 folded')
  assert.ok(node(g, 'ses_p0k3'), 'the one that needs the owner stays')
  assert.ok(!node(g, 'fold:ses_p0'), 'its subtree never folds')
  assert.ok(node(g, 'fold:ses_p1'), 'the oldest subtree folds first')
  assert.ok(!node(g, 'fold:ses_p59'), 'the newest stays open while the cap allows')
  assert.ok(node(g, 'ses_c') && node(g, 'ses_p59k0'))
  assert.equal(g.folded, folds.reduce((n, f) => n + f.members!.length, 0))
})

test('edges: parent solid and flowing while the child runs; deps dashed (“waits on”); retry_of dotted', () => {
  const b = board()
  const views = [...b.views, view('exe_t9', 'task', 'running', { session_id: 'ses_t9', parent: 'exe_home', thread: 'harbour-survey', retry_of: 'exe_t5' })]
  const records = [record('tsk_a', { session: 'ses_t2', deps: ['tsk_b'] }), record('tsk_b', { session: 'ses_t3' }), record('tsk_step', { parent: 'tsk_a' })]
  const g = lay(views, null, records)
  const e = (kind: string, s: string, t: string) => g.edges.find((x) => x.kind === kind && x.source === s && x.target === t)
  assert.ok(e('parent', 'ses_tides', 'act_j1')?.running, 'a running job’s edge flows')
  assert.equal(e('parent', 'ses_home', 'wak_r1'), undefined, 'a wake is a future node: off by default')
  assert.ok(e('deps', 'ses_t2', 'ses_t3'), 'a record’s deps join the tasks the records belong to: waits on')
  assert.ok(e('retry', 'ses_t9', 'ses_t5'), 'a retry points at what it retries')
  assert.ok(e('parent', 'ses_t2', 'tsk_step'), 'a step hangs from its task')
  assert.equal(node(g, 'tsk_step')!.kind, 'step')
  assert.equal(node(g, 'tsk_a'), undefined, 'a record of a task is that task’s node, not a second one')
  assert.equal(node(g, 'ses_t2')!.record?.id, 'tsk_a')
  assert.ok(!e('parent', 'ses_t1', 'ses_t1'))
})

test('jobs hang beside their session; “+ future” adds wakes, routines, watches and projects', () => {
  const g = lay(board().views)
  assert.equal(node(g, 'act_j1')!.kind, 'job')
  assert.equal(node(g, 'act_j1')!.parent, 'ses_tides')
  assert.ok(!g.nodes.some((n) => n.kind === 'wake' || n.kind === 'project'))
  const f = lay(board().views, null, [record('tsk_proj', { kind: 'project', title: 'Harbour wall' })], { future: true })
  assert.equal(node(f, 'wak_r1')!.kind, 'wake')
  assert.equal(node(f, 'tsk_proj')!.kind, 'project')
  assert.equal(node(f, 'tsk_proj')!.lane, HOME)
})

test('the time machine’s graph: views made from the tasks and sessions as they stood', () => {
  const b = board()
  const sessions = b.sessions.map((s) => ({ ...s, execution_id: `exe_${s.session_id.slice(4)}` }))
  const rows = boardRows({ views: [], tasks: b.tasks, sessions, now: NOW })
  const vs = viewsOf(rows)
  const t2 = vs.find((v) => v.session_id === 'ses_t2')!
  assert.equal(t2.kind, 'task')
  assert.equal(t2.parent, 'exe_home')
  assert.equal(vs.find((v) => v.session_id === 'ses_t4')!.state, 'done')
  const g = opsLayout(vs, [], LANES, null, { now: NOW })
  assert.ok(g.edges.some((e) => e.source === 'ses_home' && e.target === 'ses_t2'))
})
