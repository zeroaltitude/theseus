// The Operations page's one state model (theseus-g00vv): the Board, the Graph, the Threads lens and the counters name the
// same open work, WORKING counts work in flight, and only ended work is faded. One shared fixture; the owner's rules:
// waiting counts as open; opacity marks the ended alone; a conversation at rest is not working.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { WorkView } from '../src/protocol.gen/index.ts'
import { bucketOf, bucketOfNode, bucketOfRecord, bucketOfSession, isEnded, isEndedState, isOpen, isRunning, look, FADED, FILTERED_OUT, bucketGlyph } from '../src/ops/state.ts'
import { boardRows, countersOf, shown } from '../src/ops/board.ts'
import { targetOf } from '../src/ops/palette.ts'
import { bulkVerbs } from '../src/ops/selection.ts'
import { nodeIdOf, opsLayout, viewsOf } from '../src/ops/graph.ts'
import { groupThreads } from '../src/lib/threadboard.ts'
import { group, HOUR, info, MIN, NOW, record, session, view } from './ops-fixtures.ts'

const T = { thread: 'harbour-survey' }
const doing = (what: string) => ({ what, label: what, since_ms: NOW, turn: 1, loop_index: 0 }) as WorkView['doing']

function fixture() {
  const views: WorkView[] = [
    view('exe_r1', 'task', 'running', { session_id: 'ses_r1', ...T }),
    view('exe_w1', 'task', 'waiting', { session_id: 'ses_w1', doing: doing('job'), ...T }),
    view('wak_d1', 'wake', 'waiting', { session_id: 'ses_h', next_at_ms: NOW + 2 * HOUR, ...T }),
    view('exe_c1', 'task', 'cancelled', { session_id: 'ses_c1', ended_at_ms: NOW - 10 * MIN, ...T }),
    view('exe_h', 'conversation', 'ready', { session_id: 'ses_h', ...T }),
    view('exe_d1', 'task', 'done', { session_id: 'ses_d1', ended_at_ms: NOW - 20 * MIN, ...T }),
    view('exe_f1', 'task', 'failed', { session_id: 'ses_f1', dismissed: true, ended_at_ms: NOW - 30 * MIN, ...T }),
  ]
  const records = [record('tsk_c1', { session: 'ses_c1', state: 'suspended' })]
  return { views, records }
}
const OPEN = ['ses_r1', 'ses_w1', 'wak_d1', 'ses_c1']

test('the Board, the Graph and the Threads lens name the same open work', () => {
  const { views, records } = fixture()
  const rows = boardRows({ views, tasks: [], sessions: [], records, now: NOW, past: false })
  const boardOpen = rows.filter((r) => r.column !== 'done').map((r) => r.id).sort()
  assert.deepEqual(boardOpen, [...OPEN].sort())
  const g = opsLayout(views, records, [{ key: 'harbour-survey', title: 'h', needs: 0 }], null, { now: NOW, future: true, expanded: new Set(['harbour-survey']) })
  const graphOpen = g.nodes.filter((n) => isOpen(bucketOfNode(n, NOW))).map((n) => n.id).sort()
  assert.deepEqual(graphOpen, [...OPEN].sort())
  const threads = groupThreads(views, [group('harbour-survey')], records)
  const listed = threads.threads.flatMap((t) => t.items.map(nodeIdOf)).sort()
  assert.deepEqual(listed, [...OPEN].sort())
  // The suspended record of the cancelled task is paused·queued: open, never working.
  assert.equal(bucketOfNode(g.nodes.find((n) => n.id === 'ses_c1')!, NOW), 'paused')
  assert.equal(rows.find((r) => r.id === 'ses_c1')!.column, 'paused')
})

test('the working counter is the Graph’s working nodes; only ended nodes are faded, a conversation at rest is not', () => {
  const { views, records } = fixture()
  const rows = boardRows({ views, tasks: [], sessions: [], records, now: NOW, past: false })
  const g = opsLayout(views, records, [{ key: 'harbour-survey', title: 'h', needs: 0 }], null, { now: NOW, future: true, expanded: new Set(['harbour-survey']) })
  const working = g.nodes.filter((n) => bucketOfNode(n, NOW) === 'working').map((n) => n.id).sort()
  assert.deepEqual(working, ['ses_r1', 'ses_w1'])
  assert.equal(countersOf(rows, NOW, 0).working, working.length)
  const faded = g.nodes.filter((n) => look(bucketOfNode(n, NOW), false).faded).map((n) => n.id).sort()
  assert.deepEqual(faded, ['ses_d1', 'ses_f1'], 'the done task and the dismissed failure: nothing else')
  assert.ok(!faded.includes('ses_h'), 'the conversation at rest is drawn at full opacity')
  assert.equal(bucketOf(views[4], NOW), 'rest')
})

test('turning the WORKING filter on fades nothing that is working', () => {
  const { views, records } = fixture()
  const rows = boardRows({ views, tasks: [], sessions: [], records, now: NOW, past: false })
  const kept = new Set(shown(rows, { bucket: 'working' }, '', { now: NOW, spentToday: new Set(), records }).rows.map((r) => r.id))
  const g = opsLayout(views, records, [{ key: 'harbour-survey', title: 'h', needs: 0 }], null, { now: NOW, expanded: new Set(['harbour-survey']) })
  for (const n of g.nodes.filter((x) => bucketOfNode(x, NOW) === 'working')) assert.ok(kept.has(n.id), `${n.id} is working and the filter keeps it`)
})

test('the buckets: a paused view reads paused·queued though its attention is idle; unknown records stay open', () => {
  const paused = view('exe_p', 'task', 'waiting', { paused: true }, 'idle')
  assert.equal(bucketOf(paused, NOW), 'paused')
  assert.ok(isOpen('paused') && !isEnded('paused'), 'paused is open and not ended')
  assert.equal(look(bucketOf(paused, NOW), false).faded, false, 'a paused view is not faded')
  assert.equal(bucketOf(view('exe_s', 'task', 'waiting', { doing: doing('sleeping'), next_at_ms: NOW + HOUR }), NOW), 'scheduled')
  assert.equal(bucketOf(view('exe_s2', 'task', 'waiting', { doing: doing('sleeping'), next_at_ms: NOW + 30 * HOUR }), NOW), 'paused')
  assert.equal(bucketOf(view('exe_q', 'task', 'queued'), NOW), 'paused')
  assert.equal(bucketOf(view('exe_f', 'task', 'failed', {}, 'needs_you'), NOW), 'needs', 'a failure not dismissed asks')
  for (const [s, b] of [['suspended', 'paused'], ['accepted', 'paused'], ['proposed', 'paused'], ['in_progress', 'working'], ['waiting_human', 'needs'], ['blocked', 'needs'], ['done', 'ended'], ['failed', 'ended'], ['abandoned', 'ended']] as const) assert.equal(bucketOfRecord(s), b, s)
  assert.equal(bucketOfSession(session('ses_a', { execution_state: 'waiting', attention: { level: 'ready', label: 'ready', since_ms: 1 } })), 'rest')
  assert.equal(bucketGlyph('paused', 'suspended'), '⏸')
  assert.equal(bucketGlyph('paused', 'accepted'), '◷')
})

test('the nodes read the model: opacity is the ended’s alone, the filtered-out wear a dashed outline', () => {
  const node = readFileSync(new URL('../src/ops/GraphNode.tsx', import.meta.url), 'utf8').replace(/\/\/.*$/gm, '')
  assert.ok(!/opacity-30/.test(node), 'a filtered-out node is not drawn faint')
  assert.match(node, /d\.dim && FILTERED_OUT/)
  assert.match(node, /faded && FADED/)
  assert.match(node, /bucketOfNode\(/)
  assert.match(FILTERED_OUT, /outline-dashed/)
  assert.match(FADED, /opacity/)
  assert.ok(!FILTERED_OUT.includes('opacity'))
})

test('a ready conversation rests on the Graph as on the Board, and a running routine is working (join fix 1)', () => {
  const rows = boardRows({ views: [], tasks: [], sessions: [session('ses_q', { execution_state: 'waiting', attention: { level: 'ready', label: 'ready', since_ms: NOW } })], now: NOW, past: true })
  assert.deepEqual(viewsOf(rows).map((v) => bucketOf(v, NOW)), ['rest'], 'the time machine’s resting conversation is not working')
  assert.equal(bucketOf(view('rtn_1', 'routine', 'running', { next_at_ms: NOW + HOUR }), NOW), 'working', 'a routine firing is work in flight')
})

test('waiting is open work but no turn to stop: one predicate for Stop, in the palette, the Inspector and the bulk bar (theseus-jo84p)', () => {
  assert.equal(isRunning('running'), true)
  assert.equal(isRunning('waiting'), false, 'a view waiting on a job, a child or a time has no turn to stop')
  assert.equal(isRunning(undefined, 'running'), true, 'a session whose execution runs, when no view is held')
  assert.equal(isRunning('waiting', 'waiting'), false)
  assert.equal(isRunning(undefined), false)
  const views = [view('exe_c1', 'conversation', 'running', { session_id: 'ses_c1' }), view('exe_c2', 'conversation', 'waiting', { session_id: 'ses_c2', doing: doing('job') })]
  const rows = boardRows({ views, tasks: [], sessions: [], now: NOW, past: false })
  const [run, wait] = [rows.find((r) => r.id === 'ses_c1')!, rows.find((r) => r.id === 'ses_c2')!]
  assert.equal(targetOf(run).running, true)
  assert.equal(targetOf(wait).running, false, 'the palette offers no Stop for a waiting conversation')
  const stop = (r: typeof run) => bulkVerbs([r], []).find((o) => o.verb === 'stop')!.items.length
  assert.equal(stop(run), 1)
  assert.equal(stop(wait), 0, 'bulk Stop skips a waiting conversation')
  const inspector = readFileSync(new URL('../src/ops/Inspector.tsx', import.meta.url), 'utf8')
  assert.match(inspector, /const running = isRunning\(r\.view\?\.state, r\.session\?\.execution_state\)/)
  const infoOnly = boardRows({ views: [], tasks: [info(5, 'waiting')], sessions: [], now: NOW, past: false })[0]
  assert.equal(targetOf(infoOnly).running, false, 'an info-only task waiting is not running either')
})

test('a task known only by its record, waiting, is not called working; running still is (theseus-jo84p)', () => {
  const col = (state: string) => boardRows({ views: [], tasks: [info(6, state)], sessions: [], now: NOW, past: false })[0].column
  assert.equal(col('running'), 'working')
  assert.notEqual(col('waiting'), 'working', 'nothing says it is in flight')
  assert.equal(col('waiting'), 'paused', 'open and recoverable')
})

test('the ended view states are named once (theseus-jo84p)', () => {
  for (const s of ['done', 'failed', 'cancelled', 'expired']) assert.equal(isEndedState(s), true, s)
  for (const s of ['running', 'waiting', 'queued', 'needs_you', 'ready', undefined]) assert.equal(isEndedState(s), false, String(s))
  for (const f of ['graph.ts', 'rehome.ts', 'selection.ts', 'facts.ts', 'GraphNode.tsx']) {
    const t = readFileSync(new URL(`../src/ops/${f}`, import.meta.url), 'utf8')
    assert.ok(!/'done', 'failed', 'cancelled', 'expired'/.test(t) && !/ENDED_VIEW = /.test(t), `${f} repeats the ended list`)
  }
})

// ---- the one bucket of a card (theseus-odj0m) ----

import { bucketOfCard, OWNER_CALLS, RECOMMENDED, type OwnerCalls } from '../src/ops/state.ts'

test('bucketOfCard over every row of the record table, under today’s answers and under the recommended ones', () => {
  const rec = (state: string, over = {}) => record('tsk_x', { state, ...over } as never)
  const at = (calls: OwnerCalls, c: Parameters<typeof bucketOfCard>[0]) => bucketOfCard(c, NOW, calls)
  const table: Array<[string, Parameters<typeof bucketOfCard>[0], string, string]> = [
    // [row, card, today's bucket, the recommended bucket]
    ['waiting_human', { kind: 'step', record: rec('waiting_human') }, 'needs', 'needs'],
    ['blocked', { kind: 'step', record: rec('blocked') }, 'needs', 'needs'],
    ['a proposal waiting', { kind: 'step', record: rec('proposed', { proposal: { card: 'act_p', at_ms: NOW } }) }, 'needs', 'needs'],
    ['in_progress, its own task running', { kind: 'task', view: view('exe_a', 'task', 'running'), record: rec('in_progress', { session: 'ses_a' }) }, 'working', 'working'],
    ['in_progress, no task, carrier in flight', { kind: 'step', record: rec('in_progress'), carrierInFlight: true }, 'working', 'working'],
    ['in_progress, no task, carrier stopped', { kind: 'step', record: rec('in_progress'), carrierInFlight: false }, 'working', 'paused'],
    ['suspended', { kind: 'step', record: rec('suspended') }, 'paused', 'paused'],
    ['accepted', { kind: 'step', record: rec('accepted') }, 'paused', 'todo'],
    ['proposed', { kind: 'step', record: rec('proposed') }, 'paused', 'todo'],
    ['done', { kind: 'step', record: rec('done') }, 'ended', 'ended'],
    ['failed', { kind: 'step', record: rec('failed') }, 'ended', 'ended'],
    ['abandoned', { kind: 'step', record: rec('abandoned') }, 'ended', 'ended'],
    ['any card with an ask', { kind: 'step', record: rec('done'), asks: 1 }, 'needs', 'needs'],
  ]
  for (const [row, card, today, recommended] of table) {
    assert.equal(at(OWNER_CALLS, card), today, `${row}, today`)
    assert.equal(at(RECOMMENDED, card), recommended, `${row}, recommended`)
  }
})

test('bucketOfCard has one path for a task: a failure not dismissed needs you, a suspended record whose execution ended is paused', () => {
  const task = (state: string, over = {}) => ({ kind: 'task', task: { section: state === 'failed' ? 'needs' : 'working', paused: false, view: view('exe_a', 'task', state as never, over, state === 'failed' ? 'needs_you' : undefined) } })
  assert.equal(bucketOfCard(task('failed'), NOW), 'needs')
  const ended = { kind: 'task', task: { section: 'done', paused: false, view: view('exe_b', 'task', 'cancelled') }, record: record('tsk_b', { state: 'suspended' }) }
  assert.equal(bucketOfCard(ended, NOW), 'paused')
  assert.equal(bucketOfCard({ ...ended, record: undefined }, NOW), 'ended')
  const later = { kind: 'task', task: { section: 'working', paused: false, info: { starts_at_ms: NOW + 30 * HOUR, state: 'queued' } as never } }
  assert.equal(bucketOfCard(later, NOW), 'paused')
  assert.equal(bucketOfCard({ kind: 'task', task: { section: 'working', paused: false, info: { starts_at_ms: NOW + HOUR, state: 'queued' } as never } }, NOW), 'scheduled')
})
