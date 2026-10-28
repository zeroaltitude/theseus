// The Graph lens's layout (theseus-jid7t.4): lanes per thread, needs-you first, and a home lane; columns by depth;
// positions sticky by id under live changes; ended work folded at an hour; the 400-node cap folding subtrees; the
// three kinds of edge; jobs beside their session; "+ future" nodes only when asked.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { WorkView } from '../src/protocol.gen/index.ts'
import { COL_W, foldBacks, GUTTER, HOME, labelHeight, LANE_HEAD, LANE_LABEL_PAD, LANE_TITLE_LINE, NODE_CAP, opsLayout, shownTitle, stepNode, titleLines, viewsOf, wordsLines, type Graph, type LaneMeta } from '../src/ops/graph.ts'
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

test('a first layout that placed nothing (before the first read) keeps no lane order', () => {
  const empty = opsLayout([], [], [], null, { now: NOW })
  assert.deepEqual(empty.order, [HOME])
  const g = opsLayout(board().views, [], LANES, empty, { now: NOW })
  assert.deepEqual(g.lanes.map((l) => l.key), ['harbour-survey', 'tide-tables', HOME], 'needs-you first, home last, as a fresh layout')
  assert.ok(g.nodes.every((n) => !n.fresh), 'nothing fades in on the first read')
})

/** The 541-node graph of the cap test. */
function wide(): WorkView[] {
  const views: WorkView[] = [view('exe_c', 'conversation', 'running', { session_id: 'ses_c', thread: 'tide-tables' })]
  for (let p = 0; p < 60; p++) {
    views.push(view(`exe_p${p}`, 'task', 'running', { session_id: `ses_p${p}`, parent: 'exe_c', thread: 'tide-tables', started_at_ms: NOW - HOUR + p * MIN }))
    for (let k = 0; k < 8; k++) views.push(view(`exe_p${p}k${k}`, 'task', p === 0 && k === 3 ? 'needs_you' : 'running', { session_id: `ses_p${p}k${k}`, parent: `exe_p${p}`, thread: 'tide-tables' }, p === 0 && k === 3 ? 'needs_you' : undefined))
  }
  return views
}

test('an opened fold shows its subtree and the cap still holds', () => {
  const views = wide()
  const g = lay(views, null, [], { opened: new Set(['ses_p1']) })
  assert.ok(node(g, 'ses_p1k0') && node(g, 'ses_p1k7'), 'the opened root’s members are drawn')
  assert.ok(!node(g, 'fold:ses_p1'))
  assert.ok(g.nodes.length <= NODE_CAP, `${g.nodes.length} nodes`)
  assert.ok(node(g, 'fold:ses_p2'), 'another subtree folds instead')
  assert.ok(!g.cantOpen)
  // Everything opened cannot hold: it folds back, and says so.
  const all = lay(views, null, [], { opened: new Set(Array.from({ length: 60 }, (_, i) => `ses_p${i}`)) })
  assert.ok(all.nodes.length <= NODE_CAP && all.cantOpen === true, 'the cap is never raised')
  assert.ok(node(all, 'fold:ses_p1'))
})

test('an opened subtree survives a fold of its ancestor (join fix B)', () => {
  // root → mid → five leaves, and a second root with four leaves, over a cap of 10: opening mid keeps its leaves
  // drawn, and the other subtree folds instead; if nothing else can fold, the page says the opening cannot hold.
  const views: WorkView[] = [view('exe_c', 'conversation', 'running', { session_id: 'ses_c', thread: 'tide-tables' })]
  views.push(view('exe_r', 'task', 'running', { session_id: 'ses_r', parent: 'exe_c', thread: 'tide-tables', started_at_ms: NOW - HOUR }))
  views.push(view('exe_m', 'task', 'running', { session_id: 'ses_m', parent: 'exe_r', thread: 'tide-tables', started_at_ms: NOW - 50 * MIN }))
  for (let k = 0; k < 5; k++) views.push(view(`exe_mk${k}`, 'task', 'running', { session_id: `ses_mk${k}`, parent: 'exe_m', thread: 'tide-tables', started_at_ms: NOW - 40 * MIN }))
  views.push(view('exe_r2', 'task', 'running', { session_id: 'ses_r2', parent: 'exe_c', thread: 'tide-tables', started_at_ms: NOW - 30 * MIN }))
  for (let k = 0; k < 4; k++) views.push(view(`exe_r2k${k}`, 'task', 'running', { session_id: `ses_r2k${k}`, parent: 'exe_r2', thread: 'tide-tables', started_at_ms: NOW - 20 * MIN }))
  const g = lay(views, null, [], { cap: 10, opened: new Set(['ses_m']) })
  for (let k = 0; k < 5; k++) assert.ok(node(g, `ses_mk${k}`), `the opened mid's leaf ${k} is drawn`)
  assert.ok(!(node(g, 'fold:ses_r')?.members ?? []).some((m) => m.startsWith('ses_mk')), 'no fold of the root holds them')
  assert.ok(node(g, 'fold:ses_r2'), 'the other subtree folds instead')
  assert.ok(g.nodes.length <= 10 && !g.cantOpen, `${g.nodes.length} nodes`)
  // With nothing else to fold, the opening cannot hold: it folds back, and says so.
  const tight = lay(views.filter((v) => !v.id.startsWith('exe_r2')), null, [], { cap: 6, opened: new Set(['ses_m']) })
  assert.ok(tight.nodes.length <= 6 && tight.cantOpen === true, 'the cap is never raised')
})

test('the keys walk the graph’s nodes', () => {
  const g = lay(board().views)
  const ids = g.nodes.map((n) => n.id)
  const home = node(g, 'ses_home')!
  assert.equal(stepNode(g, null, 0, 1), 'ses_t1', 'from nothing: the first node that needs the owner')
  assert.equal(stepNode({ nodes: g.nodes.filter((n) => !n.needs) }, null, 0, 1) !== null, true, 'else the top-left one')
  const right = stepNode(g, home.id, 1, 0)!
  assert.equal(node(g, right)!.x, home.x + COL_W, 'l: the next column right')
  assert.equal(stepNode(g, right, -1, 0) !== null && node(g, stepNode(g, right, -1, 0)!)!.x, home.x, 'h: back left')
  assert.equal(stepNode(g, home.id, -1, 0), null, 'left of column 0 is nowhere: the page goes to the queue')
  const col = g.nodes.filter((n) => n.x === node(g, right)!.x && ['task', 'job', 'step', 'conversation'].includes(n.kind)).sort((a, b) => a.y - b.y)
  if (col.length > 1) {
    assert.equal(stepNode(g, col[0].id, 0, 1), col[1].id, 'j: the nearest below')
    assert.equal(stepNode(g, col[1].id, 0, -1), col[0].id, 'k: the nearest above')
  }
  assert.equal(stepNode(g, col.at(-1)!.id, 0, 1), null, 'nothing below the last')
  for (const k of [-1, 1]) for (const id of ids) { const to = stepNode(g, id, k, 0); if (to) assert.ok(!['done', 'folded'].includes(node(g, to)!.kind)) }
  // Folds are skipped.
  const fold = lay(wide())
  for (const n of fold.nodes) for (const [dx, dy] of [[0, 1], [0, -1], [1, 0], [-1, 0]]) { const to = stepNode(fold, n.id, dx, dy); if (to) assert.notEqual(node(fold, to)!.kind, 'folded') }
  assert.ok(ids.length > 0)
})

test('the keys walk from a step node and from a task’s record id (join fix A)', () => {
  // The page's selection can be a step's or a task's record id (`?sel=tsk_…`); the keys start from that node.
  const records = [record('tsk_a', { session: 'ses_t2' }), record('tsk_s1', { parent: 'tsk_a', created_at_ms: NOW - 50 * MIN }), record('tsk_s2', { parent: 'tsk_a', created_at_ms: NOW - 40 * MIN })]
  const g = lay(board().views, null, records)
  const s1 = node(g, 'tsk_s1')!, s2 = node(g, 'tsk_s2')!
  assert.equal(s1.kind, 'step')
  assert.equal(s1.x, s2.x, 'both steps sit in one column')
  const [top, low] = s1.y < s2.y ? [s1, s2] : [s2, s1]
  assert.equal(stepNode(g, top.id, 0, 1), low.id, 'j from a step: the next step below')
  assert.equal(node(g, 'ses_t2')!.record?.id, 'tsk_a')
  assert.equal(stepNode(g, 'tsk_a', 1, 0), stepNode(g, 'ses_t2', 1, 0), 'l from a task’s record id is l from its node')
  assert.equal(stepNode(g, 'tsk_a', 0, 1), stepNode(g, 'ses_t2', 0, 1), 'j from a task’s record id is j from its node')
})

test('redact masks a private or sensitive node’s title, not an open one’s', () => {
  const secret = { id: 'ses_harbour_7f3a2c', kind: 'task', title: 'Tide survey for the invented quay', data: 'sensitive' as const }
  assert.equal(shownTitle(secret, true), 'task 7f3a2c')
  assert.equal(shownTitle({ ...secret, data: 'private' }, true), 'task 7f3a2c')
  assert.equal(shownTitle(secret, false), secret.title, 'no redact: the title')
  assert.equal(shownTitle({ ...secret, data: 'open' }, true), secret.title)
  assert.equal(shownTitle({ ...secret, data: undefined }, true), secret.title)
  assert.equal(shownTitle({ title: 'Harbour survey', data: 'sensitive' }, true), 'a sensitive thread', 'a lane')
  assert.equal(shownTitle({ title: 'Harbour survey', data: 'private' }, true), 'a private thread')
})

test('an opening is not blamed when the graph is over its cap for another reason', () => {
  // Every subtree that could fold holds a needs-you leaf, so the graph stays over the cap with or without the opening.
  const views: WorkView[] = [view('exe_c', 'conversation', 'running', { session_id: 'ses_c', thread: 'tide-tables' })]
  for (let p = 0; p < 4; p++) {
    views.push(view(`exe_p${p}`, 'task', 'running', { session_id: `ses_p${p}`, parent: 'exe_c', thread: 'tide-tables', started_at_ms: NOW - HOUR + p * MIN }))
    for (let k = 0; k < 3; k++) views.push(view(`exe_p${p}k${k}`, 'task', k === 0 ? 'needs_you' : 'running', { session_id: `ses_p${p}k${k}`, parent: `exe_p${p}`, thread: 'tide-tables' }, k === 0 ? 'needs_you' : undefined))
  }
  const plain = lay(views, null, [], { cap: 6 })
  const opened = lay(views, null, [], { cap: 6, opened: new Set(['ses_p1']) })
  assert.ok(plain.nodes.length > 6, 'over the cap with nothing to fold')
  assert.ok(!opened.cantOpen, 'the opening is not the cause')
  assert.ok(opened.nodes.length <= plain.nodes.length)
})

test('opsLayout lays the graph out once', () => {
  const src = readFileSync(new URL('../src/ops/graph.ts', import.meta.url), 'utf8').replace(/\/\/.*$/gm, '')
  const at = src.indexOf('export function opsLayout')
  const body = src.slice(at, src.indexOf('\n}\n', at))
  assert.equal(body.split('layout(').length - 1, 1)
})

test('an opening that folds again while the graph stays over its cap says so', () => {
  // Subtree A can fold; each B holds a needs-you leaf and never folds. Over the cap with A held, and over it without.
  const views: WorkView[] = [view('exe_c', 'conversation', 'running', { session_id: 'ses_c', thread: 'tide-tables' })]
  views.push(view('exe_a', 'task', 'running', { session_id: 'ses_a', parent: 'exe_c', thread: 'tide-tables', started_at_ms: NOW - HOUR }))
  for (let k = 0; k < 3; k++) views.push(view(`exe_ak${k}`, 'task', 'running', { session_id: `ses_ak${k}`, parent: 'exe_a', thread: 'tide-tables' }))
  for (let p = 0; p < 4; p++) {
    views.push(view(`exe_b${p}`, 'task', 'running', { session_id: `ses_b${p}`, parent: 'exe_c', thread: 'tide-tables', started_at_ms: NOW - HOUR + (p + 1) * MIN }))
    for (let k = 0; k < 3; k++) views.push(view(`exe_b${p}k${k}`, 'task', k === 0 ? 'needs_you' : 'running', { session_id: `ses_b${p}k${k}`, parent: `exe_b${p}`, thread: 'tide-tables' }, k === 0 ? 'needs_you' : undefined))
  }
  const plain = lay(views, null, [], { cap: 6 })
  const opened = lay(views, null, [], { cap: 6, opened: new Set(['ses_a']) })
  assert.ok(opened.nodes.length > 6, 'over the cap')
  assert.ok(opened.nodes.some((n) => n.id === 'fold:ses_a'), 'A folded again')
  assert.deepEqual(opened.nodes.map((n) => n.id).sort(), plain.nodes.map((n) => n.id).sort(), 'what is drawn is what nothing opened draws')
  assert.equal(opened.cantOpen, true)
})

test('an opening offers its way back, and folding back lays out as if it were never opened (item 5)', () => {
  const views = [
    view('exe_c', 'conversation', 'ready', { session_id: 'ses_c', thread: 'tide-tables' }),
    view('exe_old1', 'task', 'done', { session_id: 'ses_old1', parent: 'exe_c', thread: 'tide-tables', ended_at_ms: NOW - 61 * MIN }, 'idle'),
    view('exe_old2', 'task', 'failed', { session_id: 'ses_old2', parent: 'exe_c', thread: 'tide-tables', ended_at_ms: NOW - 3 * HOUR }, 'idle'),
  ]
  const ids = (g: Graph) => g.nodes.map((n) => n.id).sort()
  // A lane's done fold.
  const base = lay(views)
  const folds = base.nodes.filter((n) => n.kind === 'done')
  assert.ok(folds.length > 0, 'the fixture folds something done')
  const lane = folds[0].lane
  const open = lay(views, base, [], { expanded: new Set([lane]) })
  const back = foldBacks(open, base, new Set([lane]), new Set())
  assert.deepEqual(back.lanes, [{ lane, n: folds[0].members!.length }])
  assert.deepEqual(back.roots, [])
  assert.deepEqual(foldBacks(base, base, new Set(), new Set()), { lanes: [], roots: [] }, 'nothing open: nothing offered')
  assert.deepEqual(foldBacks(base, base, new Set([lane]), new Set()).lanes, [], 'an opening that is not drawn open offers nothing')
  const folded = lay(views, open, [], {})
  assert.deepEqual(ids(folded), ids(base), 'the same nodes as never opened')
  // The fold node itself is placed afresh (the rows its members held stay holes, as for any node that left); the rest does not move.
  const rest = (g: Graph) => Object.fromEntries(Object.entries(where(g)).filter(([id]) => !id.startsWith('done:')))
  assert.deepEqual(rest(folded), rest(base), 'sticky slots of the rest unchanged')
  // An opened "+N folded" root.
  const many = wide()
  const closed = lay(many, null, [])
  const root = closed.nodes.find((n) => n.kind === 'folded')!.parent!
  const o = lay(many, closed, [], { opened: new Set([root]) })
  assert.deepEqual(foldBacks(o, closed, new Set(), new Set([root])).roots, [root])
  assert.deepEqual(ids(lay(many, o, [], {})), ids(closed), 'folding back gives the layout without the opening')
})

test('a fold opened inside an opened done lane has its own fold-back (item 5, join fix F1)', () => {
  const views: WorkView[] = [view('exe_c', 'conversation', 'ready', { session_id: 'ses_c', thread: 'tide-tables' })]
  for (let p = 0; p < 60; p++) {
    views.push(view(`exe_d${p}`, 'task', 'done', { session_id: `ses_d${p}`, parent: 'exe_c', thread: 'tide-tables', ended_at_ms: NOW - 2 * HOUR }, 'idle'))
    for (let k = 0; k < 8; k++) views.push(view(`exe_d${p}k${k}`, 'task', 'done', { session_id: `ses_d${p}k${k}`, parent: `exe_d${p}`, thread: 'tide-tables', ended_at_ms: NOW - 2 * HOUR }, 'idle'))
  }
  const ids = (g: Graph) => g.nodes.map((n) => n.id).sort()
  const base = lay(views)
  const ex = new Set(['tide-tables'])
  const lane = lay(views, null, [], { expanded: ex })
  const fold = lane.nodes.find((n) => n.kind === 'folded')
  assert.ok(fold, 'the opened lane is over the cap, so subtrees fold')
  const root = fold!.parent!
  const op = new Set([root])
  const g2 = lay(views, lane, [], { expanded: ex, opened: op })
  assert.ok(node(g2, root) && !node(g2, `fold:${root}`), 'the root is drawn open')
  assert.deepEqual(foldBacks(g2, base, ex, op, lay(views, null, [], { expanded: ex })).roots, [root], 'its fold-back is offered')
  assert.deepEqual(ids(lay(views, g2, [], { expanded: ex })), ids(lane), 'folding it back gives the opened lane without it')
})

test('GraphLens folds an opening back by deleting it from a copy of its set (item 5)', () => {
  const s = readFileSync(new URL('../src/ops/GraphLens.tsx', import.meta.url), 'utf8').replace(/^\s*\/\/.*$/gm, '')
  assert.match(s, /setExpanded\(\(s\) => \{ const t = new Set\(s\); t\.delete\(/)
  assert.match(s, /setOpened\(\(s\) => \{ const t = new Set\(s\); t\.delete\(/)
  assert.match(s, /foldBacks\(/)
  assert.match(s, /foldBacks\(g, base, expanded, opened, baseRoots\)/, 'the roots read the layout with the lanes opened (join fix F1)')
})

// ---- the graph is laid out from the work set (theseus-odj0m) ----

import { contextOf, shown } from '../src/ops/board.ts'
import { layoutRows } from '../src/ops/graph.ts'

test('the filter keeps records: a counter click keeps a matching step drawn normally, its parents as context', () => {
  // The plan is carried by a task in flight (ses_t2): a plan nobody works is paused·queued itself (Q9) and would be kept.
  const records = [record('tsk_pl', { state: 'in_progress', title: 'Survey', origin: { session: 'ses_t2' } as never }), record('tsk_su', { state: 'suspended', parent: 'tsk_pl', title: 'Paint' })]
  const rows = boardRows({ ...board(), records, now: NOW })
  const kept = new Set(shown(rows, { bucket: 'paused' }, '', { now: NOW, spentToday: new Set(), records }).rows.map((r) => r.id))
  assert.ok(kept.has('tsk_su'), 'the step is kept (shownIds is built from the one set)')
  const g = layoutRows(rows, LANES, null, { now: NOW })
  const ctx = contextOf(rows, kept)
  assert.ok(node(g, 'tsk_su') && kept.has('tsk_su') && !ctx.has('tsk_su'), 'drawn normally, not outlined')
  assert.ok(ctx.has('tsk_pl') && !kept.has('tsk_pl'), 'its plan is context: outlined, not counted')
})

test('Q4 A: the rest of the graph is left out, every slot kept as a hole, and a lane with no kept card shrinks to its label', () => {
  const rows = boardRows({ ...board(), now: NOW })
  const full = layoutRows(rows, LANES, null, { now: NOW })
  const kept = new Set(['ses_t2'])
  const keep = new Set([...kept, ...contextOf(rows, kept)])
  const cut = layoutRows(rows, LANES, full, { now: NOW, keep })
  assert.ok(cut.nodes.every((n) => keep.has(n.id)) && cut.nodes.length > 0 && cut.hidden === full.nodes.length - cut.nodes.length)
  for (const n of cut.nodes) assert.deepEqual(cut.slots[n.id], full.slots[n.id], `${n.id} keeps its slot`)
  assert.deepEqual(cut.slots, full.slots, 'every slot is kept, hidden ones too')
  assert.deepEqual(cut.bands, full.bands, 'so the layout the next change keeps is the full one')
  const emptied = cut.lanes.find((l) => l.key === 'harbour-survey')!
  assert.equal(emptied.rows, 0, 'a lane with no kept card draws no row')
  assert.equal(emptied.height, labelHeight(emptied), 'and shrinks to its gutter label, never below it (theseus-6ln2q)')
  assert.ok(cut.edges.every((e) => keep.has(e.source) && keep.has(e.target)))
  const again = layoutRows(rows, LANES, cut, { now: NOW })
  assert.equal(again.hidden, 0)
  assert.deepEqual(where(again), where(full), 'the filter cleared, nothing moved')
})

// theseus-6ln2q: the owner's own board — seven cold threads the retire bar has emptied, long wrapping titles, one
// live home lane. Every band used to be LANE_HEAD tall and every label overprinted the lane below it.
const COLD: LaneMeta[] = [
  { key: 'harbor-survey', title: 'Harbor survey', needs: 0, words: 'cold · 9d 19h ago · 22k ctx' },
  { key: 'theseus-development', title: 'Theseus development', needs: 0, words: 'cold · 10d 19h ago · 36k ctx' },
  { key: 'openclaw', title: 'OpenClaw', needs: 0, words: 'cold · 12d 13h ago · 23k ctx' },
  { key: 'theseus-design-oct-3-11', title: 'Theseus design, Oct 3-11', needs: 0, words: 'cold · 14d ago · 51k ctx' },
  { key: 'garden-notes', title: 'Garden notes', needs: 0, words: 'cold · 16d ago · 9k ctx' },
  { key: 'openclaw-plugins', title: 'OpenClaw plugins', needs: 0, words: 'cold · 18d ago · 14k ctx', sealed: true },
  { key: 'qx-lab', title: 'QX-LAB', needs: 2, words: 'cold · 20d ago · 31k ctx' },
]

test('a lane’s band holds its own gutter label, so no two lanes overlap (theseus-6ln2q)', () => {
  const rows = boardRows({ ...board(), now: NOW })
  // The retire bar's shape: nothing of the cold threads is kept, so each of their lanes draws no card at all.
  const keep = new Set(rows.filter((r) => r.id === 'ses_t2').map((r) => r.id))
  for (const opts of [{ now: NOW }, { now: NOW, keep }]) {
    const g = layoutRows(rows, [...COLD, ...LANES], null, opts)
    for (const l of g.lanes) {
      assert.ok(l.height >= labelHeight(l), `${l.key}: the band (${l.height}) holds its label (${labelHeight(l)})`)
      assert.ok(l.height >= LANE_HEAD, `${l.key}: and still holds its head`)
    }
    for (const [i, l] of g.lanes.entries()) {
      for (const m of g.lanes.slice(i + 1)) {
        assert.ok(l.y + l.height <= m.y, `${l.key} (${l.y}+${l.height}) ends before ${m.key} (${m.y}) begins`)
      }
    }
  }
  // The `keep` pass is the one that exercises the fix: without it every band carries a spare row and is 158 tall,
  // so `Math.max` never reaches for the label. Pin that the pass actually empties a lane (join fix).
  const cut = layoutRows(rows, [...COLD, ...LANES], null, { now: NOW, keep })
  const empty = cut.lanes.filter((l) => l.rows === 0)
  assert.ok(empty.length >= COLD.length, `${empty.length} lanes draw no card, so the label is what holds the band`)
  assert.ok(empty.every((l) => l.height === labelHeight(l)))
})

// The browser's own answers, measured in headless Chrome against the real faces at the gutter label's exact CSS
// (theseus-6ln2q): Cinzel at 15 px, bold, uppercase, tracking 0.12em, and JetBrains Mono at 10.5 px, both in the
// 180 px box. This is the oracle for `titleLines` and `wordsLines`: every row here is a height a browser drew.
// The first pass of the fix charged one width for every character and one line for the words, and it under-counted
// the last four titles and both of the long words rows, so the band came out short of its own label again.
const MEASURED_TITLE: ReadonlyArray<[string, number]> = [
  ['Harbor survey', 1], ['OpenClaw', 1], ['Garden notes', 1], ['QX-LAB', 1], ['Home · no thread', 1],
  ['WWWWW MMMMM', 1], ['IIIIIIIIIIIIIIIIIIII', 1], ['Supercalifragilisticexpialidocious', 1],
  ['Theseus development', 2], ['OpenClaw plugins', 2], ['Theseus design, Oct 3-11', 2],
  ['Workflow memory', 2], ['a sensitive thread', 2], ['Supercalifragilistic expialidocious', 2],
  ['Summer mammoth window', 3], ['Memory window mammoth warmth', 3],
]
const MEASURED_WORDS: ReadonlyArray<[string, number]> = [
  ['cold · 8d ago', 1], ['cold · 9d 19h ago · 22k ctx', 1], ['cold · 10d 19h ago · 36k ctx', 1],
  ['warm · 2h ago · 128k ctx · $12.40 today', 2],
  ['warm · 2d 19h ago · 128k ctx · $12.40 today · Opus', 2],
]

test('the gutter wraps a lane’s words the way the browser does (theseus-6ln2q)', () => {
  for (const [title, lines] of MEASURED_TITLE) assert.equal(titleLines(title), lines, `the browser draws “${title}” on ${lines}`)
  for (const [words, lines] of MEASURED_WORDS) assert.equal(wordsLines(words), lines, `the browser draws “${words}” on ${lines}`)
  assert.equal(titleLines(''), 1, 'a lane with no title still takes a line')
  assert.ok(titleLines('OpenClaw plugins', true) >= titleLines('OpenClaw plugins'), 'a seal leaves room for its lock')
  // A glyph the advance table has not measured is charged the widest one, so it can only make the band taller.
  assert.ok(titleLines('幸幸幸幸幸幸幸幸幸幸幸幸') >= 1)
  // The label's own sum: the inset twice, the title's lines, the words' lines and their margin, the badges.
  assert.equal(labelHeight({ key: 'k', title: 'OpenClaw', needs: 0 }), 32 + 19)
  assert.equal(labelHeight({ key: 'k', title: 'OpenClaw', needs: 0, words: 'cold · 8d ago' }), 32 + 19 + 4 + 15)
  assert.equal(labelHeight({ key: 'k', title: 'Theseus development', needs: 3, words: 'cold · 8d ago' }), 32 + 38 + 19 + 24)
  assert.equal(labelHeight({ key: 'k', title: 'Workflow memory', needs: 0, words: 'warm · 2h ago · 128k ctx · $12.40 today' }), 32 + 38 + 4 + 30,
    'a two-line title over two lines of words — the case the first pass drew 36 px short of')
  assert.equal(labelHeight({ key: 'k', title: 'OpenClaw', needs: 0 }, true), 32 + 19 + 24, 'an opened lane leaves room for its fold-back')
  // Redact can lengthen a short title, so a locked lane's band holds whichever of the two is taller.
  const locked = { key: 'k', title: 'QX-LAB', needs: 0, data: 'sensitive' as const }
  assert.equal(titleLines(shownTitle(locked, true)), 2, 'redact draws “a sensitive thread” on two lines')
  assert.equal(labelHeight(locked), 32 + 38, 'so the band holds two, not the one “QX-LAB” needs')
})

test('a band is never shorter than the label it carries, for any lane the page can build (theseus-6ln2q)', () => {
  // Every title the browser was measured on, against every words row, with and without a seal, a needs count and an
  // opened fold: the band must hold the label in all of them.
  for (const [title] of MEASURED_TITLE) {
    for (const [words] of MEASURED_WORDS) {
      for (const sealed of [false, true]) {
        for (const needs of [0, 3]) {
          const lane: LaneMeta = { key: 'k', title, needs, words, sealed }
          const g = layoutRows([], [lane], null, { now: NOW })
          assert.ok(g.lanes[0].height >= labelHeight(lane), `${title} / ${words}: band ${g.lanes[0].height} holds label ${labelHeight(lane)}`)
          assert.ok(labelHeight(lane) >= LANE_LABEL_PAD * 2 + LANE_TITLE_LINE, 'and never less than one line in its inset')
        }
      }
    }
  }
})

test('a filter chip reads the counter’s words and number', async () => {
  const { chipWords } = await import('../src/ops/board.ts')
  assert.equal(chipWords('paused', 4), 'paused·queued · 4')
})
