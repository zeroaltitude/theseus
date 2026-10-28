// Tasks on the agenda timeline (theseus-1t4q8): a running task is a live block from its start to now, a finished one keeps
// its real span, a deferred one is a future block, a routine's occurrences are recurring blocks (an ended or held routine
// has none), and every block says what a number on the old strip said. Pure: all on invented work.
import test from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import type { RunRow, WorkView } from '../src/protocol.gen/index.ts'
import { weekWindow, type Mark } from '../src/lib/agenda.ts'
import { tableRows, timelineOrder, clashes } from '../src/ops/timeline.ts'
import { DEFAULT_PLANNED_MS, taskBlocks, taskLayout, taskLine, taskVerbs, type TaskBlock } from '../src/ops/taskblocks.ts'
import { local } from './ops-calendar-fixtures.ts'

const read = (p: string) => readFileSync(new URL(`../${p}`, import.meta.url), 'utf8')
const withZone = <T>(zone: string, f: () => T): T => {
  const was = process.env.TZ
  process.env.TZ = zone
  try { return f() } finally { if (was === undefined) delete process.env.TZ; else process.env.TZ = was }
}
const MIN = 60_000
const view = (id: string, over: Partial<WorkView> = {}): WorkView => ({
  id, kind: 'task', position: 1, at_ms: 0, root: 'exe_home', title: id, state: 'running', attention: { level: 'working', label: 'running', since_ms: 0 },
  started_at_ms: 0, state_since_ms: 0, session_id: `ses_${id}`, ...over,
} as WorkView)
const mark = (key: string, kind: Mark['kind'], at_ms: number, over: Partial<Mark> = {}): Mark => ({ key, kind, at_ms, title: `a ${kind}`, id: `wak_${key}`, session_id: 'ses_harbour', ...over })
const run = (n: number, state: string, start: number, dur: number, over: Partial<RunRow> = {}): RunRow => ({ task: `ses_run${n}`, short: `r${n}`, run: n, state, started_ms: start, started_local: '', duration_ms: dur, cost_usd: 0.25, ...over })
const by = (bs: readonly TaskBlock[], key: string) => bs.find((b) => b.key === key)

test('a running task is a live block from its start to now, growing with the clock, carrying its cost and state', () => {
  withZone('America/Phoenix', () => {
    const start = local(2026, 10, 21, 11)
    const v = view('tsk_live', { started_at_ms: start, title: 'fix the build', cost: { spent_usd: 1.4, limit_usd: 5 }, doing: { what: 'tool', label: 'turn 4', since_ms: start, turn: 4, loop_index: 0 } })
    const a = taskBlocks([v], {}, [], local(2026, 10, 21, 12))
    const b = by(a, 'task:tsk_live')!
    assert.equal(b.kind, 'running')
    assert.equal(b.live, true)
    assert.deepEqual([b.from_ms, b.to_ms], [start, local(2026, 10, 21, 12)])
    assert.equal(b.cost_usd, 1.4)
    assert.equal(b.limit_usd, 5)
    assert.equal(b.session_id, 'ses_tsk_live')
    const later = by(taskBlocks([v], {}, [], local(2026, 10, 21, 13)), 'task:tsk_live')!
    assert.equal(later.to_ms, local(2026, 10, 21, 13), 'a minute later it is a minute longer')
    assert.match(taskLine(b, local(2026, 10, 21, 12)), /fix the build/)
    assert.match(taskLine(b, local(2026, 10, 21, 12)), /running/)
    assert.match(taskLine(b, local(2026, 10, 21, 12)), /\$1\.40 of \$5/, 'its cost and its limit, as the board says them')
    assert.match(taskLine(b, local(2026, 10, 21, 12)), /1h/)
  })
})

test('a finished task keeps its real span and how it ended; a task that never started draws nothing', () => {
  withZone('America/Phoenix', () => {
    const now = local(2026, 10, 21, 15)
    const views = [
      view('tsk_ok', { state: 'done', started_at_ms: local(2026, 10, 21, 9), ended_at_ms: local(2026, 10, 21, 9, 40), cost: { spent_usd: 0.5, limit_usd: 5 } }),
      view('tsk_bad', { state: 'failed', started_at_ms: local(2026, 10, 21, 10), ended_at_ms: local(2026, 10, 21, 10, 5) }),
      view('tsk_off', { state: 'cancelled', started_at_ms: local(2026, 10, 21, 11), ended_at_ms: local(2026, 10, 21, 11, 5) }),
      view('tsk_queued', { state: 'queued', started_at_ms: 0 }),
      view('ses_talk', { kind: 'conversation', started_at_ms: local(2026, 10, 21, 8) }),
    ]
    const a = taskBlocks(views, {}, [], now)
    assert.deepEqual([by(a, 'task:tsk_ok')!.kind, by(a, 'task:tsk_ok')!.from_ms, by(a, 'task:tsk_ok')!.to_ms, by(a, 'task:tsk_ok')!.live], ['finished', local(2026, 10, 21, 9), local(2026, 10, 21, 9, 40), false])
    assert.equal(by(a, 'task:tsk_ok')!.tone, 'ok')
    assert.equal(by(a, 'task:tsk_bad')!.tone, 'fault')
    assert.equal(by(a, 'task:tsk_off')!.tone, 'idle')
    assert.equal(a.length, 3, 'an unstarted task and a conversation are not blocks')
    assert.match(taskLine(by(a, 'task:tsk_ok')!, now), /done/)
    assert.match(taskLine(by(a, 'task:tsk_ok')!, now), /\$0\.50/)
  })
})

test('a deferred task is a future block at its planned time, as long as its forecast says; the board’s deferred view is not drawn twice', () => {
  withZone('America/Phoenix', () => {
    const now = local(2026, 10, 21, 12)
    const at = local(2026, 10, 22, 9)
    const v = view('tsk_later', { state: 'waiting', next_at_ms: at, started_at_ms: local(2026, 10, 21, 11), title: 'the nightly sweep', forecast: { usd_p50: 0.2, usd_p90: 0.6, min_p50: 45, min_p90: 90, n: 12 } })
    const bare = view('tsk_bare', { state: 'queued', next_at_ms: local(2026, 10, 22, 14), started_at_ms: local(2026, 10, 21, 11) })
    const marks = [mark('starts:tsk_later:1', 'start', at, { id: 'tsk_later', title: 'the nightly sweep' }), mark('starts:tsk_bare:1', 'start', local(2026, 10, 22, 14), { id: 'tsk_bare', title: 'tsk_bare' })]
    const a = taskBlocks([v, bare], {}, marks, now)
    assert.equal(a.length, 2, 'the deferred tasks are scheduled blocks only, not running ones')
    const s = by(a, 'starts:tsk_later:1')!
    assert.equal(s.kind, 'scheduled')
    assert.equal(s.live, false)
    assert.deepEqual([s.from_ms, s.to_ms], [at, at + 45 * MIN], 'as long as the nearest finished tasks took (the median)')
    assert.equal(s.planned, true, 'its end is a plan, not a fact')
    assert.equal(by(a, 'starts:tsk_bare:1')!.to_ms - local(2026, 10, 22, 14), DEFAULT_PLANNED_MS, 'with no forecast: the default span')
    assert.equal(s.session_id, 'ses_harbour')
  })
})

test('a routine’s occurrences are recurring blocks across the window; its runs are recurring too; a held, ended or over series draws none', () => {
  withZone('America/Phoenix', () => {
    const now = local(2026, 10, 21, 12)
    const routine = (id: string, over: Partial<WorkView> = {}) => view(id, {
      kind: 'routine', state: 'waiting', title: 'morning report', session_id: `ses_${id}`,
      routine: { schedule: 'weekdays 08:00', runs_30d: { ok: 3, failed: 0, skipped: 0 }, month: { spent_usd: 1, limit_usd: 10 }, deliver_to: 'dm', last: { outcome: 'ok', at_ms: local(2026, 10, 21, 8), duration_ms: 20 * MIN, cost_usd: 0.3 } } as WorkView['routine'],
      series: { cron: '0 8 * * 1-5', occurrence: 4, missed_last: 0 }, ...over,
    })
    const views = [
      routine('wak_r1'), routine('wak_ended', { state: 'cancelled' }), routine('wak_held', { paused: true }),
      routine('wak_until', { series: { cron: '0 8 * * 1-5', occurrence: 9, missed_last: 0, until_ms: local(2026, 10, 22, 12) } }),
    ]
    const marks = [
      mark('run:wak_r1:a', 'run', local(2026, 10, 22, 8), { id: 'wak_r1', title: 'morning report', occurrence: 5 }),
      mark('run:wak_r1:b', 'run', local(2026, 10, 23, 8), { id: 'wak_r1', title: 'morning report', occurrence: 6 }),
      mark('run:wak_ended:a', 'run', local(2026, 10, 22, 8), { id: 'wak_ended' }),
      mark('run:wak_held:a', 'run', local(2026, 10, 22, 8), { id: 'wak_held' }),
      mark('run:wak_until:a', 'run', local(2026, 10, 22, 8), { id: 'wak_until' }),
      mark('run:wak_until:b', 'run', local(2026, 10, 23, 8), { id: 'wak_until' }),
      mark('run:wak_unseen:a', 'run', local(2026, 10, 22, 8), { id: 'wak_unseen' }),
    ]
    const a = taskBlocks(views, {}, marks, now)
    const keys = a.map((b) => b.key).sort()
    assert.deepEqual(keys, ['run:wak_r1:a', 'run:wak_r1:b', 'run:wak_unseen:a', 'run:wak_until:a'],
      'the ended routine, the held one and the occurrence after `until` vanish; one the board does not hold stays as the daemon served it')
    const r = by(a, 'run:wak_r1:a')!
    assert.equal(r.kind, 'recurring')
    assert.equal(r.recurring, true)
    assert.equal(r.occurrence, 5)
    assert.equal(r.to_ms - r.from_ms, 20 * MIN, 'as long as its last run took')
    assert.equal(r.planned, true)
    assert.equal(r.session_id, 'ses_harbour')
    assert.match(taskLine(r, now), /recurring/)
    assert.match(taskLine(r, now), /weekdays 08:00/, 'its schedule as written')
  })
})

test('a routine run in flight is a live recurring block, a finished one its real span; the task view of the same run is not drawn twice', () => {
  withZone('America/Phoenix', () => {
    const now = local(2026, 10, 21, 12)
    const runs = { ses_routine: [run(1, 'complete', local(2026, 10, 21, 8), 25 * MIN, { cost_usd: 0.4 }), run(2, 'running', local(2026, 10, 21, 11, 30), 0, { cost_usd: 0.1 })] }
    const views = [view('wak_rt', { kind: 'routine', state: 'running', title: 'morning report', session_id: 'ses_routine' }), view('tsk_run2', { session_id: 'ses_run2', started_at_ms: local(2026, 10, 21, 11, 30), title: 'morning report · run 2' })]
    const a = taskBlocks(views, runs, [], now)
    const one = by(a, 'past:ses_routine:ses_run1')!
    assert.deepEqual([one.kind, one.recurring, one.from_ms, one.to_ms, one.cost_usd], ['finished', true, local(2026, 10, 21, 8), local(2026, 10, 21, 8, 25), 0.4])
    assert.equal(one.title, 'morning report · run 1')
    const two = by(a, 'past:ses_routine:ses_run2')!
    assert.deepEqual([two.kind, two.live, two.to_ms], ['running', true, now])
    assert.equal(a.filter((b) => b.key === 'task:tsk_run2').length, 0, 'the run’s task is the run’s block')
    assert.equal(a.length, 2)
  })
})

test('blocks lay out per day by wall-clock hour, cut at midnight, overlapping ones in side-by-side lanes, a short one still a target', () => {
  withZone('America/Phoenix', () => {
    const w = weekWindow(local(2026, 10, 19, 8))
    const now = local(2026, 10, 21, 12)
    const blocks = taskBlocks([
      view('tsk_a', { started_at_ms: local(2026, 10, 21, 9), state: 'done', ended_at_ms: local(2026, 10, 21, 10, 30) }),
      view('tsk_b', { started_at_ms: local(2026, 10, 21, 10), state: 'done', ended_at_ms: local(2026, 10, 21, 11) }),
      view('tsk_c', { started_at_ms: local(2026, 10, 21, 13), state: 'done', ended_at_ms: local(2026, 10, 21, 13, 1) }),
      view('tsk_night', { started_at_ms: local(2026, 10, 20, 23), state: 'done', ended_at_ms: local(2026, 10, 21, 1) }),
    ], {}, [], now)
    const placed = taskLayout(blocks, w)
    const at = (k: string, day?: number) => placed.filter((p) => p.key === `task:${k}` && (day == null || p.day === day))
    assert.deepEqual([at('tsk_a')[0].day, at('tsk_a')[0].top, at('tsk_a')[0].bottom], [2, 9 / 24, 10.5 / 24])
    assert.deepEqual([at('tsk_a')[0].lane, at('tsk_a')[0].lanes, at('tsk_b')[0].lane, at('tsk_b')[0].lanes], [0, 2, 1, 2])
    assert.deepEqual([at('tsk_c')[0].lane, at('tsk_c')[0].lanes], [0, 1])
    assert.ok(at('tsk_c')[0].bottom - at('tsk_c')[0].top >= 0.25 / 24 - 1e-12, 'a one-minute task is a 15-minute target')
    assert.deepEqual(at('tsk_night').map((p) => [p.day, p.top, p.bottom, p.continues]), [[1, 23 / 24, 1, false], [2, 0, 1 / 24, true]], 'over midnight: both days')
  })
})

test('the block’s verbs are the task’s own: a live one opens, pauses or resumes and stops; a finished one opens; a recurring one carries its routine’s', () => {
  const now = local(2026, 10, 21, 12)
  const live = by(taskBlocks([view('tsk_live', { started_at_ms: now - 10 * MIN })], {}, [], now), 'task:tsk_live')!
  const held = by(taskBlocks([view('tsk_held', { started_at_ms: now - 10 * MIN, paused: true })], {}, [], now), 'task:tsk_held')!
  const done = by(taskBlocks([view('tsk_done', { state: 'done', started_at_ms: now - 10 * MIN, ended_at_ms: now - 5 * MIN })], {}, [], now), 'task:tsk_done')!
  assert.deepEqual(taskVerbs(live).map((v) => [v.key, v.method, v.params]), [['pause', 'task.pause', { task: 'tsk_live' }], ['stop', 'task.cancel', { task: 'tsk_live' }]])
  assert.deepEqual(taskVerbs(held).map((v) => [v.key, v.method]), [['resume', 'task.resume'], ['stop', 'task.cancel']])
  assert.match(taskVerbs(live)[1].ask ?? '', /Stop/)
  assert.deepEqual(taskVerbs(done), [])
  assert.match(taskLine(held, now), /paused/)
  assert.equal(live.title, 'tsk_live')
})

test('the order j and k step, the table and the clashes carry the task blocks too: nothing a mark showed is lost, and a running task clashes with no meeting', () => {
  withZone('America/Phoenix', () => {
    const now = local(2026, 10, 21, 12)
    const w = weekWindow(local(2026, 10, 19, 8))
    const blocks = taskBlocks([view('tsk_live', { started_at_ms: local(2026, 10, 21, 11), title: 'fix the build', cost: { spent_usd: 1.4, limit_usd: 5 } })], {}, [], now)
    const order = timelineOrder([mark('m', 'reminder', local(2026, 10, 21, 13))], [], w, blocks)
    assert.deepEqual(order, ['task:tsk_live', 'm'])
    const rows = tableRows([], [], w, false, clashes([], []), () => '', blocks, now)
    assert.equal(rows.length, 1)
    assert.equal(rows[0].key, 'task:tsk_live')
    assert.equal(rows[0].calendar, 'Theseus')
    assert.match(rows[0].what, /task/)
    assert.match(rows[0].line, /fix the build/)
    assert.match(rows[0].line, /\$1\.40 of \$5/)
    // A block that derives from a mark shares its key: the mark's table row is the block's, so nothing is listed twice.
    const derived = taskBlocks([view('tsk_later', { state: 'waiting', next_at_ms: local(2026, 10, 22, 9) })], {}, [mark('starts:tsk_later:1', 'start', local(2026, 10, 22, 9), { id: 'tsk_later' })], now)
    assert.equal(tableRows([mark('starts:tsk_later:1', 'start', local(2026, 10, 22, 9))], [], w, false, clashes([], []), () => 'x', derived, now).length, 1)
    assert.equal(timelineOrder([mark('starts:tsk_later:1', 'start', local(2026, 10, 22, 9))], [], w, derived).length, 1)
  })
})

test('the lens draws the blocks beside the events, under the existing marks and bars: wired, with no request of its own', () => {
  const lens = read('src/ops/TimelineLens.tsx')
  const strip = read('src/ops/WeekStripV2.tsx')
  assert.ok(lens.includes('taskBlocks('), 'the lens works the blocks out from the board and the agenda it already reads')
  assert.ok(strip.includes('data-task-block'), 'the strip draws them')
  assert.ok(strip.includes('taskLayout('))
  assert.ok(lens.includes('TaskDetail'), 'a picked block shows its facts and verbs under the strip')
  assert.ok(!/useEffect\([^)]*\)\s*=>\s*{[^}]*fetch|setInterval/.test(strip + read('src/ops/taskblocks.ts')), 'no timer or request of its own: the lens’s minute tick moves the live block')
  assert.ok(strip.includes('ranBars('), 'the ran bars stay: a block hides no mark')
  assert.ok(strip.includes('Tick'), 'the ticks stay')
  assert.ok(lens.includes('recurring') || strip.includes('recurring'), 'recurring is said on the legend or the block')
})
