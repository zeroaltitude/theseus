// The Operations page's Board lens (theseus-jid7t.3): the five columns over the Tasks page's rows, with the
// conversations, jobs, and what is due today; the filters, the search's "n of m", the counters and the model chip.
import test from 'node:test'
import assert from 'node:assert/strict'
import { boardRows, chipChoices, columnsOf, countersOf, matchesFilters, modelOf, projectOf, selects, shown, spanMs, spendToday } from '../src/ops/board.ts'
import { cardFacts } from '../src/ops/facts.ts'
import { board, HOUR, info, MIN, NOW, record, session, view } from './ops-fixtures.ts'

const cx = { now: NOW, spentToday: new Set<string>(), records: [] }

test('the columns: needs you, working, paused·queued, scheduled, done today and older', () => {
  const b = board()
  const c = columnsOf(boardRows({ ...b, now: NOW }), NOW)
  assert.deepEqual(c.needs.map((r) => r.id), ['ses_t5', 'ses_t1'], 'the longest waiting first: the failure two hours, the question forty minutes')
  assert.deepEqual(c.working.map((r) => r.id).sort(), ['act_j1', 'ses_t2', 'ses_tides'])
  assert.deepEqual(c.paused.map((r) => r.id), ['ses_t3'], 'a paused task')
  assert.deepEqual(c.scheduled.map((r) => r.id), ['wak_r1', 'ses_t6'], 'today’s wake, then the deferred start at 17:00; next week’s wake is not today’s')
  assert.deepEqual(c.done.map((r) => r.id), ['ses_home', 'ses_t4'], 'a conversation at rest, and a task that reported an hour ago')
  assert.deepEqual(c.older.map((r) => r.id), ['ses_t7'], 'ended yesterday: paged under Done')
  const kinds = new Map(boardRows({ ...b, now: NOW }).map((r) => [r.id, r.kind]))
  assert.equal(kinds.get('act_j1'), 'job')
  assert.equal(kinds.get('ses_tides'), 'conversation')
  assert.equal(kinds.get('wak_r1'), 'wake')
})

test('a deferred start beyond today waits in paused·queued; a queued task too', () => {
  const rows = boardRows({ views: [], tasks: [info(8, 'queued', { starts_at_ms: NOW + 30 * HOUR }), info(9, 'queued')], sessions: [], now: NOW })
  assert.deepEqual(rows.map((r) => [r.id, r.column]), [['ses_t8', 'paused'], ['ses_t9', 'paused']])
})

test('the time machine’s board: no views, the tasks and conversations as they stood', () => {
  const rows = boardRows({ views: [], tasks: [info(2, 'running')], sessions: [session('ses_a', { execution_state: 'running' }), session('ses_b', { retired: { reason: 'quiet', at_ms: 1 } as never }), session('ses_t2', { kind: 'task' })], now: NOW })
  assert.deepEqual(rows.map((r) => [r.id, r.kind, r.column]), [['ses_t2', 'task', 'working'], ['ses_a', 'conversation', 'working']], 'a retired session is not on it, a task once')
})

test('the model chip: a checkpoint’s move first, then the session’s profile, then its model', () => {
  const b = board()
  const rows = new Map(boardRows({ ...b, now: NOW }).map((r) => [r.id, r]))
  assert.equal(rows.get('ses_t2')!.model, 'sonnet', 'moved_to wins over the session’s quick')
  assert.equal(rows.get('ses_t1')!.model, 'quick')
  assert.equal(rows.get('ses_tides')!.model, 'deep')
  assert.equal(modelOf(undefined, session('ses_x', { profile: undefined, model: 'haiku' })), 'haiku')
  assert.equal(modelOf(undefined, undefined), undefined)
  assert.equal(cardFacts(rows.get('ses_t2')!, NOW).model, 'sonnet')
})

test('filters: each chip, the counters’ buckets, ended within', () => {
  const b = board()
  const rows = boardRows({ ...b, now: NOW })
  const ids = (f: object, c = cx) => rows.filter((r) => matchesFilters(r, f, c)).map((r) => r.id).sort()
  assert.deepEqual(ids({ thread: 'tide-tables' }), ['act_j1', 'ses_t2', 'ses_tides'].sort())
  assert.deepEqual(ids({ model: 'sonnet' }), ['ses_t2'])
  assert.deepEqual(ids({ data: 'sensitive' }), ['ses_t1'], 'the data chip reads the session’s mark')
  assert.deepEqual(ids({ data: 'private' }), ['act_j1', 'ses_tides'], 'a job carries its session’s mark')
  assert.ok(ids({ data: 'open' }).includes('ses_t2') && !ids({ data: 'open' }).includes('ses_t1'), 'no mark is open')
  assert.deepEqual(ids({ kind: 'job' }), ['act_j1'])
  assert.deepEqual(ids({ state: 'failed' }), ['ses_t5'])
  assert.deepEqual(ids({ place: 'discord dm' }), ['ses_t1'])
  assert.deepEqual(ids({ bucket: 'needs' }), ['ses_t1', 'ses_t5'])
  assert.deepEqual(ids({ bucket: 'still' }), ['ses_t6', 'wak_r1'])
  assert.deepEqual(ids({ bucket: 'done' }), ['ses_home', 'ses_t4'], 'done today only')
  assert.deepEqual(ids({ bucket: 'spent' }, { ...cx, spentToday: new Set(['ses_t2']) }), ['ses_t2'])
  assert.deepEqual(ids({ ended: '2h' }), ['ses_home', 'ses_t4'])
  assert.deepEqual(ids({ ended: 'soon' }), [], 'a span the chip does not know keeps nothing')
  assert.equal(spanMs('7d'), 7 * 24 * HOUR)
  assert.deepEqual(ids({ thread: 'tide-tables', kind: 'task' }), ['ses_t2'], 'chips combine')
  const records = [record('tsk_p', { kind: 'project' }), record('tsk_a', { parent: 'tsk_p', session: 'ses_t2' })]
  assert.equal(projectOf('ses_t2', records), 'tsk_p')
  assert.deepEqual(ids({ project: 'tsk_p' }, { ...cx, records }), ['ses_t2'])
  const choices = chipChoices(rows)
  assert.deepEqual(choices.thread, ['harbour-survey', 'tide-tables'])
  assert.ok(choices.model.includes('sonnet') && choices.kind.includes('job'))
})

test('search reads title, brief, report line and progress line, and says n of m', () => {
  const b = board()
  const rows = boardRows({ ...b, now: NOW })
  const records = [record('tsk_m', { session: 'ses_t3', objective: 'count every buoy in the outer harbour' })]
  const n = (q: string) => shown(rows, {}, q, { ...cx, records }).rows.map((r) => r.id)
  assert.deepEqual(n('moorings'), ['ses_t4'], 'the title')
  assert.deepEqual(n('six free'), ['ses_t4'], 'the report line')
  assert.deepEqual(n('outer harbour'), ['ses_t3'], 'the brief')
  assert.deepEqual(n('paused'), ['ses_t3'], 'the progress line says paused')
  const s = shown(rows, {}, 'tides', { ...cx, records })
  assert.equal(s.of, rows.length)
  assert.ok(s.rows.length >= 1 && s.rows.length < s.of)
})

test('a selection names a row by its session, view, execution or short id', () => {
  const rows = new Map(boardRows({ ...board(), now: NOW }).map((r) => [r.id, r]))
  const t1 = rows.get('ses_t1')!
  for (const sel of ['ses_t1', 'exe_t1', '0000t1']) assert.ok(selects(t1, sel), sel)
  assert.ok(!selects(t1, 'ses_t2'))
  assert.ok(!selects(t1, null))
  assert.ok(selects(rows.get('act_j1')!, 'act_j1'))
})

test('the counters, and today’s spend from the model calls since midnight', () => {
  const b = board()
  const c = countersOf(boardRows({ ...b, now: NOW }), NOW, 1.5)
  assert.deepEqual(c, { working: 3, paused: 1, still: 2, done: 2, spent: 1.5 })
  const rows = [
    { kind: 'provider.call', at_unix_ms: NOW - 20 * HOUR, session_id: 'ses_y', data: { cost_usd: 9 } },
    { kind: 'provider.call', at_unix_ms: NOW - HOUR, session_id: 'ses_a', data: { cost_usd: 0.4 } },
    { kind: 'tool.call', at_unix_ms: NOW - 30 * MIN, session_id: 'ses_b', data: { cost_usd: 5 } },
    { kind: 'provider.call', at_unix_ms: NOW - MIN, session_id: 'ses_b', data: { cost_usd: 0.1 } },
  ]
  const s = spendToday(rows, NOW)
  assert.ok(Math.abs(s.usd - 0.5) < 1e-9, 'yesterday’s call and a tool row are not today’s model spend')
  assert.deepEqual([...s.sessions].sort(), ['ses_a', 'ses_b'])
})

test('a card shows every field of the Tasks page’s card, by state', () => {
  const rows = new Map(boardRows({ ...board(), now: NOW }).map((r) => [r.id, r]))
  const needs = cardFacts(rows.get('ses_t1')!, NOW)
  assert.equal(needs.approvals, true)
  assert.equal(needs.question, 'Which pier first? · north · south')
  assert.equal(needs.waitsOn, 'waits on confirm')
  assert.equal(needs.place, 'discord dm')
  assert.equal(needs.placeTip, 'discord:dm:4242')
  assert.equal(needs.pause, undefined, 'a task that waits on you is answered, not paused')
  assert.equal(needs.stop, 'Stop')
  const paused = cardFacts(rows.get('ses_t3')!, NOW)
  assert.equal(paused.pill, '⏸ paused')
  assert.equal(paused.pause, 'Resume')
  const done = cardFacts(rows.get('ses_t4')!, NOW)
  assert.equal(done.report, 'Forty moorings, six free.')
  assert.equal(done.cost, '$0.250', 'usd() as the Tasks page wrote it')
  assert.equal(done.open, 'open')
  assert.equal(done.stop, undefined)
  const failed = cardFacts(rows.get('ses_t5')!, NOW)
  assert.equal(failed.outcome, 'failed')
  assert.equal(failed.stop, undefined, 'a failure under Needs you is ended: no Stop')
  assert.equal(failed.approvals, false)
  assert.equal(cardFacts(rows.get('ses_t7')!, NOW).report, 'no report line')
  const working = cardFacts(rows.get('ses_tides')!, NOW)
  assert.equal(working.doing, 'thinking')
  assert.equal(working.pause, undefined, 'a conversation has no task pause')
  assert.match(cardFacts(rows.get('wak_r1')!, NOW).due ?? '', /^due \d\d:\d\d, in 2h$/)
})

test('a view the board lacks a session for still shows', () => {
  const rows = boardRows({ views: [view('act_j9', 'job', 'running', { title: '' })], tasks: [], sessions: [], now: NOW })
  assert.equal(rows[0].title, 'running', 'its attention’s label stands in for an empty title')
})

test('WORKING is work in flight: a conversation at rest is no working row, in the present or the past (theseus-g00vv)', () => {
  const rest = { execution_state: 'waiting', attention: { level: 'ready', label: 'ready', since_ms: NOW } } as never
  const sessions = [session('ses_c1', rest), session('ses_c2', rest), session('ses_c3', rest)]
  const present = boardRows({ views: [], tasks: [], sessions, now: NOW, past: false })
  assert.deepEqual(present, [], 'the present holds no view of a conversation at rest, and no row is guessed from the sessions')
  assert.equal(countersOf(present, NOW, 0).working, 0)
  const then = boardRows({ views: [], tasks: [], sessions, now: NOW, past: true })
  assert.equal(countersOf(then, NOW, 0).working, 0, 'the time machine maps ready to rest')
  assert.equal(countersOf(then, NOW, 0).done, 0, 'done counts ended work, not a conversation that went to rest')
  assert.equal(columnsOf(then, NOW).done.length + columnsOf(then, NOW).older.length, 0)
  assert.equal(then.length, 3, 'the rows stay for the Graph’s anchors')
})

test('a running view and a view waiting on a job are working; a sleeping one with a time today is scheduled', () => {
  const job = { what: 'job', label: 'job', since_ms: NOW, turn: 1, loop_index: 0 } as never
  const sleeping = { what: 'sleeping', label: 'sleeping', since_ms: NOW, turn: 1, loop_index: 0 } as never
  const views = [view('exe_a', 'task', 'running', { session_id: 'ses_a' }), view('exe_b', 'task', 'waiting', { session_id: 'ses_b', doing: job }),
    view('exe_s', 'task', 'waiting', { session_id: 'ses_s', doing: sleeping, next_at_ms: NOW + 2 * HOUR })]
  const rows = boardRows({ views, tasks: [], sessions: [], now: NOW, past: false })
  assert.equal(countersOf(rows, NOW, 0).working, 2)
  assert.equal(rows.find((r) => r.id === 'ses_s')!.column, 'scheduled')
  assert.equal(countersOf(rows, NOW, 0).still, 1)
})
