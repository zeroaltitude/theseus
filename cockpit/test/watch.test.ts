// The Ship's watch (`src/ship/watch.ts`, theseus-hnof), run by `npm test`: the five plates' numbers, lines, and
// overlays, from a small fleet, its calls, its questions, and its ledger rows, at a fixed moment; and the calls the Ship
// reads, the newest with every one not settled (theseus-hnof.3).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import {
  argvWords, commandKey, DAY_MS, DayScan, dollars, endOf, HOUR_MS, hoursOf, keyOf, median, mergeActions, platesOf, programOf, resultWords, shortPaths, span, watchOf,
  WATCH_KEYS, WRONG_KINDS, type WatchInput,
} from '../src/ship/watch.ts'
import { busyDay, randomLedger, said, seeded } from './busy.ts'

// 14:00 on a day whose local midnight is 07:00 UTC (a UTC-7 clock): the moment's hour is the day's 14th.
const DAY = Date.UTC(2026, 9, 6, 7, 0, 0)
const NOW = DAY + 14 * HOUR_MS
const MIN = 60_000

interface V { id: string; title?: string; state?: string; attention?: { level: string; label: string; since_ms: number }; hold?: Record<string, unknown> }
interface L { id: string; session: string; kind: 'user' | 'model' | 'call' | 'result'; at: number; turn?: string; tool?: string; cid?: string; failed?: boolean; l1?: boolean; running?: boolean; preview?: string }

/** A model with only what the watch reads: vessels, lights, and their lookups. */
function fleet(vs: V[], ls: L[] = []) {
  const vessels = vs.map((v) => ({ id: v.id, title: v.title ?? v.id, state: v.state ?? 'waiting', attention: v.attention, hold: v.hold, kind: 'conversation', rig: 'anchor' }))
  const byId = new Map(vessels.map((v, i) => [v.id, i]))
  const lights = ls.map((l) => ({
    id: l.id, sessionId: l.session, vessel: byId.get(l.session), kind: l.kind, at: l.at, turnId: l.turn, tool: l.tool,
    correlationId: l.cid, failed: l.failed, l1: l.l1, running: l.running, preview: l.preview ?? '',
  }))
  return { vessels, lights, byId, lightById: new Map(lights.map((l, i) => [l.id, i])) } as unknown as WatchInput['model']
}

let pos = 0
const row = (at: number, kind: string, session: string | null, data: Record<string, unknown> = {}, turn: string | null = null) =>
  ({ position: ++pos, at_unix_ms: at, kind, session_id: session, turn_id: turn, data })

const action = (cid: string, o: Record<string, unknown>) => ({
  correlation_id: cid, execution_id: `exe_${cid}`, session_id: 'ses_b', tool: 'proc.run', state: 'dispatched', retry_class: 'non_repeatable',
  planned_at_ms: NOW - 20 * MIN, deadline_at_ms: NOW + HOUR_MS, reserved_usd: 0, confirmed: false, completions_seen: 0, ...o,
}) as any

const at = (o: Partial<WatchInput>): WatchInput => ({ model: fleet([]), actions: [], confirms: [], rows: [], rowsReady: true, now: NOW, dayStart: DAY, ...o })

test('an empty fleet is idle on every plate, and lights nothing', () => {
  const w = watchOf(at({}))
  assert.equal(w.working.value, '0')
  assert.equal(w.working.caption, 'nothing is running now')
  assert.equal(w.waiting.value, '0')
  assert.equal(w.waiting.caption, 'nothing waits for your answer')
  assert.equal(w.slow.value, '—')
  assert.equal(w.slow.longestMs, null)
  assert.equal(w.spent.value, '$0.00')
  assert.equal(w.spent.hour, 14)
  assert.deepEqual(w.spent.hours, new Array(24).fill(0))
  assert.equal(w.spent.top, null)
  assert.equal(w.wrong.value, '0')
  assert.equal(w.wrong.tone, 'ok')
  assert.deepEqual(platesOf(w).map((p) => p.key), ['working', 'waiting', 'slow', 'spent', 'wrong'])
  for (const p of platesOf(w)) {
    assert.deepEqual(p.focus, { vessels: [], lights: [] }, p.key)
    assert.equal(p.more, 0, p.key)
  }
  // The pace is a figure, not a place: its line flies nowhere.
  assert.deepEqual(w.spent.lines.map((l) => [l.tag, l.text, l.to]), [['pace', '$0.00 an hour', undefined]])
})

test('before the fleet, the calls, the questions, and the ledger are read, every number says so', () => {
  const w = watchOf({ model: null, rows: [], rowsReady: false, now: NOW, dayStart: DAY })
  for (const p of platesOf(w)) assert.equal(p.value, '…', p.key)
  assert.deepEqual(w.spent.lines, [])
})

test('a running turn and a running job are working now, newest first; the older is the slowest', () => {
  const rows = [
    row(NOW - 16 * MIN, 'action.planned', 'ses_b', { correlation_id: 'act_dredge', tool: 'proc.run' }),
    row(NOW - 15 * MIN, 'tool.job_started', 'ses_b', { correlation_id: 'act_dredge', argv: ['sh', '-c', 'echo dredging; sleep 21000; echo dredged'] }),
    // An earlier turn of the running session, ended: the turn that runs is the newest.
    row(NOW - 5 * MIN, 'turn.started', 'ses_a', {}, 'turn_old'),
    row(NOW - 5 * MIN + 300, 'turn.ended', 'ses_a', { elapsed_ms: 300 }, 'turn_old'),
    row(NOW - 40_000, 'turn.started', 'ses_a', {}, 'turn_now'),
  ]
  const model = fleet(
    [{ id: 'ses_a', title: 'Tide tables, please.', state: 'running' }, { id: 'ses_b', title: 'Sound the channel', state: 'waiting', attention: { level: 'working', label: 'waiting on 1 call', since_ms: NOW - 15 * MIN } }],
    [
      { id: 'msg_now', session: 'ses_a', kind: 'user', at: NOW - 40_000, turn: 'turn_now' },
      { id: 'tcl_dredge', session: 'ses_b', kind: 'call', at: NOW - 15 * MIN, cid: 'act_dredge', tool: 'proc.run', l1: true, running: true, preview: 'run `sh -c echo dredging; sleep 21000; echo dredged` in /tmp/x/projects' },
      { id: 'trs_dredge', session: 'ses_b', kind: 'result', at: NOW - 15 * MIN + 4000, cid: 'act_dredge', tool: 'proc.run', preview: 'Still running as background job' },
    ],
  )
  const actions = [
    action('act_dredge', { dispatched_at_ms: NOW - 15 * MIN }),
    // Settled, and a call that is not a job: neither is working.
    action('act_done', { dispatched_at_ms: NOW - 9 * MIN, settled_at_ms: NOW - 8 * MIN, state: 'succeeded' }),
    action('act_read', { tool: 'fs.read', dispatched_at_ms: NOW - 1000 }),
  ]
  const w = watchOf(at({ model, actions, rows }))
  assert.equal(w.working.turns, 1)
  assert.equal(w.working.jobs, 1)
  assert.equal(w.working.value, '2')
  assert.equal(w.working.caption, 'running now: 1 turn and 1 job')
  assert.deepEqual(w.working.lines.map((l) => [l.id, l.tag, l.text, l.flag, l.figure]), [
    ['turn:turn_now', 'turn', 'Tide tables, please.', undefined, '40s'],
    ['job:act_dredge', 'proc.run', 'echo dredging; sleep 21000; echo dredged', 'L1', '15m'],
  ])
  assert.deepEqual(w.working.lines[0].to, { session: 'ses_a', node: 'msg_now' })
  assert.deepEqual(w.working.lines[1].to, { session: 'ses_b', node: 'tcl_dredge' })
  assert.deepEqual(w.working.focus, { vessels: ['ses_a', 'ses_b'], lights: ['msg_now', 'tcl_dredge', 'trs_dredge'] })

  assert.equal(w.slow.longestMs, 15 * MIN)
  assert.equal(w.slow.value, '15m')
  assert.equal(w.slow.lines[0].id, 'job:act_dredge')
  // The turn of five minutes ago is the hour's slowest (and only) one.
  assert.equal(w.slow.ended, 1)
  assert.equal(w.slow.slowestMs, 300)
  assert.equal(w.slow.lines.at(-1)!.id, 'slowest:turn_old')
})

test('a job known only from its light still works, and a running session with no open turn row times from its attention', () => {
  const model = fleet(
    [{ id: 'ses_c', state: 'running', attention: { level: 'working', label: 'turn 2', since_ms: NOW - 2 * MIN } }],
    [{ id: 'tcl_build', session: 'ses_c', kind: 'call', at: NOW - 3 * MIN, cid: 'act_build', tool: 'proc.run', running: true, preview: 'run `cargo build --release` in /tmp/x/projects/harbour' }],
  )
  const w = watchOf(at({ model }))
  assert.deepEqual(w.working.lines.map((l) => [l.tag, l.text, l.flag, l.figure]), [
    ['turn', 'ses_c', undefined, '2m'],
    ['proc.run', 'cargo build --release', 'L0', '3m'],
  ])
})

test('the questions waiting say what they would do, how long they waited, and the time left; the worst first', () => {
  const confirms = [
    {
      correlation_id: 'act_write', session_id: 'ses_r', execution_id: 'exe_r', tool: 'fs.write', input: {}, by: 'operator', floor: false,
      reason: 'create /tmp/x/projects/harbour/log.md (38 bytes): fs.write — approve ([policy.tools] "fs.write" = approve)',
      requested_at_ms: NOW - 5 * MIN, expires_at_ms: NOW + 10 * MIN,
    },
    {
      correlation_id: 'act_budget', session_id: 'ses_g', execution_id: 'exe_g', tool: 'budget.reset', input: {}, by: 'operator', floor: false,
      reason: 'Task e4398c is waiting on a call.', requested_at_ms: NOW - 20 * MIN, expires_at_ms: 0,
      budget: { spent_usd: 0, limit_usd: 0.00001, needed_usd: 0.066929, lifetime_usd: 0 }, task: { task_id: 'ses_g', short: 'e4398c', title: 'Count every gull' },
    },
    {
      correlation_id: 'act_old', session_id: 'ses_r', execution_id: 'exe_r', tool: 'fs.write', input: {}, by: 'operator', floor: false,
      reason: 'create /tmp/x/notes.md: fs.write — approve', requested_at_ms: NOW - 16 * MIN, expires_at_ms: NOW - MIN,
    },
  ] as any[]
  const model = fleet([
    { id: 'ses_r', title: 'What changed in the tide tables' },
    { id: 'ses_g', title: 'Count every gull', attention: { level: 'needs_you', label: 'budget: $0 of $0.0000', since_ms: NOW - 20 * MIN } },
    { id: 'ses_x', title: 'A blocked session', attention: { level: 'needs_you', label: 'blocked', since_ms: NOW - 7 * MIN } },
  ], [{ id: 'tcl_write', session: 'ses_r', kind: 'call', at: NOW - 5 * MIN, cid: 'act_write', tool: 'fs.write' }])
  const w = watchOf(at({ model, confirms }))
  assert.equal(w.waiting.approvals, 2)
  assert.equal(w.waiting.budgets, 1)
  // ses_g needs you because of its budget question: counted once, as the question.
  assert.equal(w.waiting.others, 1)
  assert.equal(w.waiting.value, '4')
  assert.equal(w.waiting.caption, 'waiting for your answer: 2 approvals, 1 budget question and 1 session that needs you')
  assert.deepEqual(w.waiting.lines.map((l) => [l.tag, l.text, l.detail, l.flag]), [
    ['fs.write', 'create /tmp/x/notes.md', 'waited 16m · expired · What changed in the tide tables', 'expired'],
    ['fs.write', 'create …/harbour/log.md (38 bytes)', 'waited 5m · 10m left · What changed in the tide tables', undefined],
    ['budget', 'needs $0.067 · limit $0.00001', 'waited 20m · holds until answered · Count every gull', undefined],
  ])
  assert.deepEqual(w.waiting.lines[1].to, { session: 'ses_r', node: 'tcl_write' })
  assert.equal(w.waiting.more, 1)
  assert.deepEqual(w.waiting.focus, { vessels: ['ses_g', 'ses_r', 'ses_x'], lights: ['tcl_write'] })
})

test("today's spend counts from local midnight, by hour, with its pace and the top session", () => {
  const rows = [
    row(DAY - MIN, 'provider.call', 'ses_a', { cost_usd: 5 }), // yesterday
    row(DAY + 30 * MIN, 'provider.call', 'ses_a', { cost_usd: 0.25 }),
    row(NOW - 2 * HOUR_MS, 'provider.call', 'ses_b', { cost_usd: 0.5, node_id: 'msg_b' }),
    row(NOW - 5 * MIN, 'provider.call', 'ses_a', { cost_usd: 0.1, node_id: 'msg_a' }),
  ]
  const model = fleet([{ id: 'ses_a', title: 'Harbour list' }, { id: 'ses_b', title: 'Night build' }], [{ id: 'msg_a', session: 'ses_a', kind: 'model', at: NOW - 5 * MIN }])
  const w = watchOf(at({ model, rows }))
  assert.ok(Math.abs(w.spent.usd - 0.85) < 1e-9)
  assert.equal(w.spent.calls, 3)
  assert.equal(w.spent.value, '$0.850')
  assert.equal(w.spent.caption, 'dollars since local midnight, on 3 model calls')
  assert.equal(w.spent.hours[0], 0.25)
  assert.equal(w.spent.hours[12], 0.5)
  assert.ok(Math.abs(w.spent.hours[13] - 0.1) < 1e-9)
  assert.equal(w.spent.hours.reduce((a, b) => a + b, 0), w.spent.usd)
  assert.ok(Math.abs(w.spent.pace - 0.4) < 1e-9)
  assert.deepEqual(w.spent.top, { session: 'ses_b', usd: 0.5 })
  assert.deepEqual(w.spent.lines.map((l) => [l.tag, l.text, l.figure]), [['pace', '$0.400 an hour', 'last 15 min'], ['top', 'Night build', '$0.500']])
  // The overlay: who spent today, and the model calls the chart draws (msg_b is not read yet).
  assert.deepEqual(w.spent.focus, { vessels: ['ses_a', 'ses_b'], lights: ['msg_a'] })
  // Just after midnight, the day starts over: only the new day's calls.
  const next = watchOf(at({ model, rows: [...rows, row(DAY + DAY_MS + MIN, 'provider.call', 'ses_a', { cost_usd: 0.02 })], now: DAY + DAY_MS + 2 * MIN, dayStart: DAY + DAY_MS }))
  assert.equal(next.spent.usd, 0.02)
  assert.equal(next.spent.hour, 0)
  assert.equal(next.spent.hours[0], 0.02)
})

test("a failed tool call, a failed turn, and a spend limit went wrong; a model call's failure and yesterday's did not", () => {
  const rows = [
    row(NOW - 25 * HOUR_MS, 'action.failed', 'ses_a', { correlation_id: 'act_ancient', producer: 'inproc:fs.read' }),
    row(NOW - 10 * MIN - 5, 'action.planned', 'ses_a', { correlation_id: 'act_read', tool: 'fs.read' }),
    row(NOW - 10 * MIN, 'action.failed', 'ses_a', { correlation_id: 'act_read', producer: 'inproc:fs.read', outcome: 'failed' }),
    row(NOW - 3 * MIN, 'turn.failed', 'ses_b', { reason: 'unpriced: claude-bogus-9' }, 'turn_bad'),
    row(NOW - 2 * MIN, 'action.planned', 'ses_b', { correlation_id: 'act_model', tool: 'provider.messages' }),
    row(NOW - 2 * MIN + 9, 'action.failed', 'ses_b', { correlation_id: 'act_model', producer: 'provider:zai' }),
    row(NOW - MIN, 'budget.asked', 'ses_c', { correlation_id: 'act_gull', needed_usd: 0.066929, limit_usd: 0.00001, exceeds_limit: true }),
  ]
  const model = fleet([{ id: 'ses_a', title: 'Harbour list' }, { id: 'ses_b', title: 'Pilotage note' }, { id: 'ses_c', title: 'Count every gull' }], [
    { id: 'tcl_read', session: 'ses_a', kind: 'call', at: NOW - 10 * MIN, cid: 'act_read', tool: 'fs.read' },
    { id: 'trs_read', session: 'ses_a', kind: 'result', at: NOW - 10 * MIN + 5, cid: 'act_read', tool: 'fs.read', failed: true, preview: 'cannot read /tmp/x/projects/harbour/lighthouse-log.txt: No such file or directory (os error 2)' },
    { id: 'msg_bad', session: 'ses_b', kind: 'user', at: NOW - 3 * MIN - 50, turn: 'turn_bad' },
  ])
  const w = watchOf(at({ model, rows }))
  assert.equal(w.wrong.count, 3)
  assert.deepEqual([w.wrong.calls, w.wrong.turns, w.wrong.budgets], [1, 1, 1])
  assert.equal(w.wrong.value, '3')
  assert.equal(w.wrong.tone, 'fault')
  assert.equal(w.wrong.caption, 'failures in the last 24 hours: 1 call, 1 turn and 1 spend limit')
  assert.deepEqual(w.wrong.lines.map((l) => [l.tag, l.text, l.figure]), [
    ['budget', 'a call needed $0.067, more than its whole $0.00001 limit', '1m ago'],
    ['turn', 'unpriced: claude-bogus-9', '3m ago'],
    ['fs.read', 'cannot read …/harbour/lighthouse-log.txt: No such file or directory (os error 2)', '10m ago'],
  ])
  assert.deepEqual(w.wrong.lines[2].to, { session: 'ses_a', node: 'trs_read' })
  assert.deepEqual(w.wrong.lines[1].to, { session: 'ses_b', node: 'msg_bad' })
  assert.deepEqual(w.wrong.focus, { vessels: ['ses_a', 'ses_b', 'ses_c'], lights: ['msg_bad', 'tcl_read', 'trs_read'] })
  assert.equal(w.wrong.link.to, `/ledger?kind=${WRONG_KINDS.join(',')}&from=${NOW - DAY_MS}&to=${NOW}`)
  assert.ok(w.wrong.link.to.includes('job.wrapper_lost') && w.wrong.link.to.includes('server.crashed'))
})

test("the time machine's moment: nothing after it is read", () => {
  const T = NOW - HOUR_MS
  const rows = [
    row(T - 30 * MIN, 'turn.ended', 'ses_b', { elapsed_ms: 400 }, 'turn_1'),
    row(T - 20 * MIN, 'turn.ended', 'ses_b', { elapsed_ms: 300 }, 'turn_2'),
    row(T - 10 * MIN, 'turn.ended', 'ses_b', { elapsed_ms: 1200 }, 'turn_3'),
    row(T - MIN, 'provider.call', 'ses_a', { cost_usd: 0.2 }),
    row(T - 30_000, 'turn.started', 'ses_a', {}, 'turn_t'),
    // After the moment: the turn's end, a costly call, a failure, a slow turn.
    row(T + 10_000, 'turn.ended', 'ses_a', { elapsed_ms: 40_000 }, 'turn_t'),
    row(T + MIN, 'provider.call', 'ses_a', { cost_usd: 7 }),
    row(T + 5 * MIN, 'action.failed', 'ses_a', { correlation_id: 'act_later', producer: 'inproc:fs.read' }),
    row(T + 6 * MIN, 'turn.ended', 'ses_b', { elapsed_ms: 99_999 }, 'turn_later'),
  ]
  // The fold's fleet at the moment: ses_a runs its turn.
  const model = fleet([{ id: 'ses_a', title: 'Tide tables', state: 'running' }, { id: 'ses_b' }])
  const w = watchOf(at({ model, rows, now: T }))
  assert.equal(w.working.turns, 1)
  assert.equal(w.working.lines[0].figure, '30s')
  assert.equal(w.spent.usd, 0.2)
  assert.equal(w.spent.hour, 13)
  assert.equal(w.wrong.count, 0)
  assert.equal(w.wrong.caption, 'nothing failed in the last 24 hours')
  assert.equal(w.slow.ended, 3)
  assert.equal(w.slow.slowestMs, 1200)
  assert.equal(w.slow.medianMs, 400)
  assert.equal(w.slow.longestMs, 30_000)
  // The same rows, read live an hour later: the turn has ended, and the call and the failure count.
  const live = watchOf(at({ model: fleet([{ id: 'ses_a' }, { id: 'ses_b' }]), rows }))
  assert.equal(live.working.turns, 0)
  assert.ok(Math.abs(live.spent.usd - 7.2) < 1e-9)
  assert.equal(live.wrong.count, 1)
})

test('the words: spans, dollars, commands, paths, and the median', () => {
  assert.deepEqual([0, 999, 42_000, 60_000, 15 * MIN + 59_000, HOUR_MS + 5 * MIN, 26 * HOUR_MS].map(span), ['0s', '0s', '42s', '1m', '15m', '1h 05m', '1d 2h'])
  assert.deepEqual([0, 0.00001, 0.0042, 0.85, 12.5].map(dollars), ['$0.00', '$0.00001', '$0.0042', '$0.850', '$12.50'])
  assert.equal(argvWords(['sh', '-c', 'sleep 1; echo the berths are clear']), 'sleep 1; echo the berths are clear')
  assert.equal(argvWords(['/bin/bash', '-lc', 'cargo test']), 'cargo test')
  assert.equal(argvWords(['cargo', 'build', '--manifest-path', '/srv/x/projects/harbour/Cargo.toml']), 'cargo build --manifest-path …/harbour/Cargo.toml')
  assert.equal(shortPaths('read /tmp/a/b/c.txt and /etc/x'), 'read …/b/c.txt and /etc/x')
  // An address keeps its path.
  assert.equal(shortPaths('copied to s3://harbour/charts/2026/tides.csv'), 'copied to s3://harbour/charts/2026/tides.csv')
  assert.deepEqual(resultWords('[exit code 3]\nerror: the tide gauge is offline\n'), { words: 'error: the tide gauge is offline', exit: '3' })
  assert.deepEqual(resultWords('[ran in L1, the sandbox: no network] [exit code 0] 42 fathoms'), { words: '42 fathoms', exit: '0' })
  assert.deepEqual(resultWords('cannot read it'), { words: 'cannot read it', exit: undefined })
  assert.equal(median([]), null)
  assert.equal(median([5, 1, 3]), 3)
  assert.equal(median([4, 1, 3, 2]), 2.5)
})

test('a lost, refused, stopped or never-started job, an unknown outcome and a crash went wrong, each call once', () => {
  const rows = [
    // A job whose wrapper was killed: the lost wrapper and the unknown outcome are one failure, the wrapper's row its words.
    row(NOW - 50 * MIN, 'action.planned', 'ses_a', { correlation_id: 'act_lost', tool: 'proc.run' }),
    row(NOW - 40 * MIN, 'job.wrapper_lost', 'ses_a', { correlation_id: 'act_lost', pid: 4242, signal: 9, tool: 'proc.run', execution_id: 'exe_a' }),
    row(NOW - 40 * MIN + 5, 'action.outcome_unknown', 'ses_a', { correlation_id: 'act_lost', producer: 'reconciler:overdue_no_evidence', outcome: 'unknown' }),
    // Below the disk's floor: one refused at its launch (its call failed too), one stopped as it ran.
    row(NOW - 30 * MIN, 'job.refused', 'ses_b', { correlation_id: 'act_refused', tool: 'proc.run', free_mb: 812, floor_mb: 1024 }),
    row(NOW - 30 * MIN + 3, 'action.failed', 'ses_b', { correlation_id: 'act_refused', producer: 'job', outcome: 'failed' }),
    row(NOW - 20 * MIN, 'job.stopped_below_floor', 'ses_b', { correlation_id: 'act_stopped', tool: 'proc.run', free_mb: 700, floor_mb: 1024 }),
    // A stop reached it before its launch.
    row(NOW - 15 * MIN, 'job.not_started', 'ses_c', { correlation_id: 'act_never', tool: 'proc.run', cancel: 'requested', resolution: 'its execution was stopped before its launch' }),
    // The daemon crashed 12 minutes ago; the start that found its crash file wrote the row a minute later.
    row(NOW - 11 * MIN, 'server.crashed', null, { at_unix_ms: NOW - 12 * MIN, pid: 77, version: '0.0.1', thread: 'tokio-runtime-worker', location: 'turn.rs:88:5' }),
    // An unknown outcome a later result settled as succeeded is none; nor is a model call's.
    row(NOW - 9 * MIN, 'action.outcome_unknown', 'ses_a', { correlation_id: 'act_late', producer: 'reconciler:overdue_no_evidence' }),
    row(NOW - 8 * MIN, 'action.resolved', 'ses_a', { correlation_id: 'act_late', outcome: 'succeeded', producer: 'job' }),
    row(NOW - 7 * MIN, 'action.planned', 'ses_a', { correlation_id: 'act_model', tool: 'provider.messages' }),
    row(NOW - 6 * MIN, 'action.outcome_unknown', 'ses_a', { correlation_id: 'act_model', producer: 'reconciler:overdue_no_evidence' }),
    // After the moment: never read.
    row(NOW + MIN, 'server.crashed', null, { at_unix_ms: NOW + 30_000, thread: 'main' }),
  ]
  const model = fleet([{ id: 'ses_a', title: 'Night build' }, { id: 'ses_b', title: 'Chart tools' }, { id: 'ses_c', title: 'Dredging run' }], [
    { id: 'tcl_lost', session: 'ses_a', kind: 'call', at: NOW - 50 * MIN, cid: 'act_lost', tool: 'proc.run' },
    { id: 'tcl_refused', session: 'ses_b', kind: 'call', at: NOW - 30 * MIN, cid: 'act_refused', tool: 'proc.run' },
  ])
  const w = watchOf(at({ model, rows }))
  assert.equal(w.wrong.count, 5)
  assert.deepEqual([w.wrong.calls, w.wrong.turns, w.wrong.budgets, w.wrong.crashes], [4, 0, 0, 1])
  assert.equal(w.wrong.caption, 'failures in the last 24 hours: 4 calls and 1 crash')
  assert.deepEqual(w.wrong.lines.map((l) => [l.tag, l.text, l.flag, l.figure]), [
    ['daemon', 'the daemon crashed: a panic on thread tokio-runtime-worker at turn.rs:88:5; it started again', undefined, '12m ago'],
    ['proc.run', 'not started: its execution was stopped before its launch', 'not run', '15m ago'],
    ['proc.run', 'stopped: the disk fell to 700 MB free, under its floor of 1,024 MB', 'disk', '20m ago'],
  ])
  // The daemon's own failure has no session to fly to.
  assert.equal(w.wrong.lines[0].to, undefined)
  assert.equal(w.wrong.more, 2)
  assert.deepEqual(w.wrong.focus, { vessels: ['ses_a', 'ses_b', 'ses_c'], lights: ['tcl_lost', 'tcl_refused'] })
  // The two older, each said by its job's own row, and when the newest of its rows was written.
  const older = watchOf(at({ model, rows: rows.filter((r) => r.at_unix_ms < NOW - 25 * MIN) }))
  assert.deepEqual(older.wrong.lines.map((l) => [l.tag, l.text, l.flag, l.to?.node]), [
    ['proc.run', 'not started: the disk had 812 MB free, under its floor of 1,024 MB', 'disk', 'tcl_refused'],
    ['proc.run', "its job's wrapper was killed by signal 9 before it reported: its outcome is unknown", 'unknown', 'tcl_lost'],
  ])
  assert.equal(older.wrong.count, 2)
  // An unknown outcome alone says why the reconciler could not tell.
  const alone = watchOf(at({ model, rows: rows.filter((r) => r.kind !== 'job.wrapper_lost' && r.at_unix_ms < NOW - 35 * MIN) }))
  assert.deepEqual(alone.wrong.lines.map((l) => [l.text, l.flag]), [['its outcome is unknown: overdue, with no evidence', 'unknown']])
})

/** A settled proc.run: its plan, its job's argv, and its result after `took` ms. */
const job = (cid: string, argv: string[], took: number, end: number) => [
  row(end - took - 5, 'action.planned', 'ses_x', { correlation_id: cid, tool: 'proc.run' }),
  row(end - took, 'tool.job_started', 'ses_x', { correlation_id: cid, argv }),
  row(end, 'action.succeeded', 'ses_x', { correlation_id: cid, duration_ms: took, outcome: 'succeeded' }),
]

test('slow holds each job against its own program, each call against its tool, and each turn against the day', () => {
  const sleep = (s: number) => ['sh', '-c', `sleep ${s}; echo the berths are clear`]
  const rows = [
    // A day of jobs: `sleep` runs take about 2 s, `cargo` builds about 11 minutes.
    ...job('act_s1', sleep(1), 1000, NOW - 9 * HOUR_MS), ...job('act_s2', sleep(2), 2000, NOW - 8 * HOUR_MS), ...job('act_s3', sleep(2), 2000, NOW - 7 * HOUR_MS),
    ...job('act_s4', sleep(3), 3000, NOW - 6 * HOUR_MS), ...job('act_s5', sleep(2), 2500, NOW - 5 * HOUR_MS),
    ...job('act_c1', ['cargo', 'build'], 10 * MIN, NOW - 4 * HOUR_MS), ...job('act_c2', ['cargo', 'build'], 12 * MIN, NOW - 3 * HOUR_MS),
    ...job('act_c3', ['cargo', 'build'], 11 * MIN, NOW - 2 * HOUR_MS),
    // fs.read takes about 125 ms; one, ten minutes ago, took 6 s.
    ...[100, 120, 125, 130].map((t, i) => row(NOW - (5 - i) * HOUR_MS, 'action.succeeded', 'ses_x', { correlation_id: `act_r${i}`, duration_ms: t })),
    ...[0, 1, 2, 3].map((i) => row(NOW - (5 - i) * HOUR_MS - 10, 'action.planned', 'ses_x', { correlation_id: `act_r${i}`, tool: 'fs.read' })),
    row(NOW - 10 * MIN - 6000, 'action.planned', 'ses_x', { correlation_id: 'act_rslow', tool: 'fs.read' }),
    row(NOW - 10 * MIN, 'action.succeeded', 'ses_x', { correlation_id: 'act_rslow', duration_ms: 6000 }),
    // Four turns this hour took 1 to 4 s; a turn has run for 40 s.
    ...[2000, 2000, 4000, 1000].map((e, i) => row(NOW - (50 - i) * MIN, 'turn.ended', 'ses_x', { elapsed_ms: e }, `turn_${i}`)),
    row(NOW - 40_000, 'turn.started', 'ses_t', {}, 'turn_now'),
    // Running: a `sleep` job for 15 minutes, a build for 12.
    row(NOW - 15 * MIN, 'tool.job_started', 'ses_j', { correlation_id: 'act_sleep', argv: sleep(21000) }),
    row(NOW - 12 * MIN, 'tool.job_started', 'ses_j', { correlation_id: 'act_cargo', argv: ['cargo', 'build', '--release'] }),
  ].sort((a, b) => a.at_unix_ms - b.at_unix_ms)
  const model = fleet([{ id: 'ses_t', title: 'Tide tables', state: 'running' }, { id: 'ses_j', title: 'Night build' }, { id: 'ses_x', title: 'Harbour list' }])
  const actions = [
    action('act_sleep', { session_id: 'ses_j', dispatched_at_ms: NOW - 15 * MIN }),
    action('act_cargo', { session_id: 'ses_j', dispatched_at_ms: NOW - 12 * MIN }),
  ]
  const w = watchOf(at({ model, rows, actions }))
  assert.equal(w.slow.slow, 2)
  assert.equal(w.slow.worst, 450)
  assert.equal(w.slow.longestMs, 15 * MIN)
  assert.equal(w.slow.value, '15m')
  assert.equal(w.slow.tone, 'wait')
  assert.equal(w.slow.caption, '2 of the 3 things running are past 3× their usual')
  assert.deepEqual(w.slow.lines.map((l) => [l.id, l.flag, l.tone, l.detail]), [
    ['job:act_sleep', '450×', 'wait', 'usually 2.00 s (5 runs of this command today)'],
    ['turn:turn_now', '20×', 'wait', 'usually 2.00 s (4 turns today)'],
    ['slowcall:act_rslow', '48×', 'wait', 'usually 125 ms (5 fs.read calls today)'],
  ])
  // The build is within its usual: it is the one not shown.
  assert.equal(w.slow.more, 1)
  assert.deepEqual([w.slow.ended, w.slow.slowestMs, w.slow.medianMs], [4, 4000, 2000])
  // With only the build running, the plate says it is within its usual.
  const calm = watchOf(at({ model: fleet([{ id: 'ses_j', title: 'Night build' }, { id: 'ses_x' }]), rows: rows.filter((r) => r.data.correlation_id !== 'act_rslow'), actions: [actions[1]] }))
  assert.equal(calm.slow.slow, 0)
  assert.equal(calm.slow.tone, 'live')
  assert.equal(calm.slow.caption, 'the one job running now, within its usual')
  assert.deepEqual(calm.slow.lines.map((l) => [l.id, l.flag, l.detail]), [
    ['job:act_cargo', 'L0', 'usually 11m 0s (3 `cargo` jobs today)'],
    ['slowest:turn_2', undefined, 'the median of 4 turns this hour: 2.00 s'],
  ])
})

test('a program run too few times is held against its tool, and a call with no usual says so', () => {
  const rows = [
    ...job('act_a', ['ls'], 1000, NOW - 3 * HOUR_MS), ...job('act_b', ['ls', '-l'], 3000, NOW - 2 * HOUR_MS), ...job('act_c', ['date'], 2000, NOW - HOUR_MS - MIN),
    row(NOW - 2 * MIN, 'tool.job_started', 'ses_j', { correlation_id: 'act_npm', argv: ['npm', 'ci'] }),
  ]
  const model = fleet([{ id: 'ses_j', title: 'Night build' }], [
    { id: 'tcl_hands', session: 'ses_j', kind: 'call', at: NOW - MIN, cid: 'act_hands', tool: 'aws.hands.run', running: true, preview: 'run 4 hands' },
  ])
  const actions = [action('act_npm', { session_id: 'ses_j', dispatched_at_ms: NOW - 2 * MIN })]
  const w = watchOf(at({ model, rows, actions }))
  assert.deepEqual(w.slow.lines.map((l) => [l.id, l.flag, l.detail]), [
    ['job:act_npm', '60×', 'usually 2.00 s (3 proc.run calls today)'],
    ['job:act_hands', 'L0', 'no usual yet: under 3 like it today'],
  ])
  assert.equal(w.slow.caption, '1 of the 2 things running is past 3× its usual')
})

test("a day the clocks change has 23 or 25 hours, and each hour's spend lands in its own bar", () => {
  assert.equal(hoursOf(DAY), 24)
  assert.equal(hoursOf(DAY, DAY + 23 * HOUR_MS), 23)
  assert.equal(hoursOf(DAY, DAY + 25 * HOUR_MS), 25)
  // The clocks go back: the day's 25th hour is its last bar, not folded into the 24th.
  const back = watchOf(at({
    rows: [row(DAY + 23 * HOUR_MS + 10 * MIN, 'provider.call', 'ses_a', { cost_usd: 0.25 }), row(DAY + 24 * HOUR_MS + 30 * MIN, 'provider.call', 'ses_a', { cost_usd: 0.5 })],
    now: DAY + 24 * HOUR_MS + 45 * MIN, dayEnd: DAY + 25 * HOUR_MS,
  }))
  assert.equal(back.spent.hours.length, 25)
  assert.deepEqual([back.spent.hours[23], back.spent.hours[24], back.spent.hour], [0.25, 0.5, 24])
  // The clocks go forward: 23 bars, the last the day's 23rd hour.
  const fwd = watchOf(at({ rows: [row(DAY + 22 * HOUR_MS + 12 * MIN, 'provider.call', 'ses_a', { cost_usd: 0.1 })], now: DAY + 22 * HOUR_MS + 30 * MIN, dayEnd: DAY + 23 * HOUR_MS }))
  assert.equal(fwd.spent.hours.length, 23)
  assert.deepEqual([fwd.spent.hours[22], fwd.spent.hour], [0.1, 22])
  assert.equal(fwd.spent.usd, 0.1)
})

test('the calls the Ship reads: the newest, and every one not settled however old, each once', () => {
  const newest = [
    action('act_b', { planned_at_ms: NOW - 2 * MIN, state: 'succeeded', dispatched_at_ms: NOW - 2 * MIN, settled_at_ms: NOW - MIN }),
    action('act_c', { planned_at_ms: NOW - MIN, dispatched_at_ms: NOW - MIN }),
  ]
  const unsettled = [
    // A job dispatched six hours ago, out of the newest page long since.
    action('act_old', { planned_at_ms: NOW - 6 * HOUR_MS, dispatched_at_ms: NOW - 6 * HOUR_MS }),
    // Read a moment before it settled: the newest page says it settled, and a call never unsettles.
    action('act_b', { planned_at_ms: NOW - 2 * MIN, dispatched_at_ms: NOW - 2 * MIN }),
    action('act_c', { planned_at_ms: NOW - MIN, dispatched_at_ms: NOW - MIN }),
    // A daemon from before the option answers its newest page: what it settled is left out.
    action('act_x', { planned_at_ms: NOW - 3 * MIN, state: 'failed', settled_at_ms: NOW - 3 * MIN }),
  ]
  const both = mergeActions(newest, unsettled)!
  assert.deepEqual(both.map((a) => [a.correlation_id, a.state]), [['act_c', 'dispatched'], ['act_b', 'succeeded'], ['act_old', 'dispatched']])
  assert.equal(mergeActions(undefined, undefined), undefined)
  assert.deepEqual(mergeActions(newest, undefined)!.map((a) => a.correlation_id), ['act_c', 'act_b'])
  assert.deepEqual(mergeActions(undefined, unsettled)!.map((a) => a.correlation_id), ['act_c', 'act_b', 'act_old'])
  // The six-hour job works on the watch only from the unsettled read.
  const model = fleet([{ id: 'ses_b', title: 'Dredging run' }])
  assert.deepEqual(watchOf(at({ model, actions: both })).working.lines.map((l) => [l.id, l.figure]), [['job:act_c', '1m'], ['job:act_old', '6h 00m']])
  assert.equal(watchOf(at({ model, actions: newest })).working.jobs, 1)
})

test("the words: a job's program, and each plate's key", () => {
  assert.equal(programOf(['sh', '-c', 'sleep 8; echo busiest hour']), 'sleep')
  assert.equal(programOf(['/bin/bash', '-lc', 'cargo test --workspace']), 'cargo')
  assert.equal(programOf(['/usr/bin/python3', 'tides.py']), 'python3')
  assert.equal(programOf(['sh', '-c', 'RUST_LOG=info cargo run']), 'cargo')
  assert.equal(programOf(['sh', '-c', '(cd harbour && make)']), 'make')
  assert.equal(programOf(['sh', '-c', 'nice -n 19 timeout 60 cargo build']), 'cargo')
  assert.equal(commandKey(['sh', '-c', 'sleep 8; echo 41 departures']), 'sleep N; echo N departures')
  assert.equal(commandKey(['cargo', 'build', '--release']), 'cargo build --release')
  assert.equal(programOf(['sh', '-c', '"$x" go']), undefined)
  // What only says something is passed over: the program is the one doing the work (theseus-cov9).
  assert.equal(programOf(['sh', '-c', 'echo building; cargo build']), 'cargo')
  assert.equal(programOf(['sh', '-c', 'echo night build started; sleep 21000; echo night build done']), 'sleep')
  assert.equal(programOf(['sh', '-c', 'printf "step 1\\n" && make']), 'make')
  assert.equal(programOf(['bash', '-c', 'cd harbour; echo go | tee log.txt']), 'tee')
  assert.equal(programOf(['sh', '-c', 'echo done']), undefined)
  assert.deepEqual(WATCH_KEYS.map(keyOf), ['1', '2', '3', '4', '5', '6'])
  assert.deepEqual([keyOf('working'), keyOf('wrong'), keyOf('since')], ['1', '5', '6'])
})

test('a session that holds web text waits for your trust: said in the caption and on its line, not in the number', () => {
  const hold = { since_ms: NOW - 20 * MIN, tool: 'http.fetch', url: 'tides.example/today', node_id: 'trs_tides' }
  const model = fleet([{ id: 'ses_h', title: 'Tide news', hold }, { id: 'ses_q', title: 'Search the charts', hold: { ...hold, tool: 'web.search', query: 'north channel silt', node_id: '' } }])
  const w = watchOf(at({ model }))
  assert.deepEqual([w.waiting.value, w.waiting.holds, w.waiting.tone], ['0', 2, 'idle'])
  assert.equal(w.waiting.caption, 'nothing waits for your answer; 2 sessions hold web text')
  assert.deepEqual(w.waiting.lines.map((l) => [l.tag, l.text, l.detail, l.to?.node]), [
    ['holds', 'read http.fetch tides.example/today', 'since 20m · Tide news', 'trs_tides'],
    ['holds', 'read web.search "north channel silt"', 'since 20m · Search the charts', undefined],
  ])
  assert.deepEqual(w.waiting.focus, { vessels: ['ses_h', 'ses_q'], lights: [] })
})

test("a walk back from a moment starts at the moment, not at the ledger's end, and misses no row a skew put late", () => {
  const rs = [10, 20, 30, 29, 40, 50, 60].map((m) => row(NOW + m * MIN, 'turn.ended', 'ses_a', { elapsed_ms: m }))
  // Every row at or before the moment is before the index, and so are those within the skew after it (the walk reads
  // past them): a row the skew put after a later one is never missed.
  assert.equal(endOf(rs, NOW + 23 * MIN), 2)
  assert.equal(endOf(rs, NOW + 35 * MIN), 5)
  assert.equal(endOf(rs, NOW + 100 * MIN), 7)
  assert.equal(endOf(rs, NOW), 0)
  assert.equal(endOf([], NOW), 0)
  for (const t of [5, 15, 29, 30, 45, 61].map((m) => NOW + m * MIN)) {
    const i = endOf(rs, t)
    assert.ok(rs.slice(i).every((r) => r.at_unix_ms > t), `a row at or before ${t} after index ${i}`)
  }
})

test("the watch's 'as of' line says the moment alone: what the daemon was then, the Ship says in one place", () => {
  // The time machine's profile, uptime and "the daemon was down then" have one home on the Ship (theseus-hnof), and
  // it is not the watch: the watch's line above the plates keeps the moment, and in a replay its progress and its
  // stop. Watch.tsx is a view, so this reads its source.
  const src = readFileSync(new URL('../src/ship/Watch.tsx', import.meta.url), 'utf8')
  assert.match(src, /\{replay\.playing \? 'replaying' : 'as of'\} \{stamp\(past\.t\)\}/, "the 'as of' line keeps its moment")
  for (const [read, what] of [[/\.profiles?\b/, 'a profile'], [/\buptimeSecs\b/, 'the uptime']] as const) {
    assert.doesNotMatch(src, read, `the watch reads ${what} at the moment: the Ship says it in one place`)
  }
})

// ---------------------------------------------------------------- kept between recomputes (theseus-qilc)

/** The local midnight before `t` on the tests' UTC-7 clock. */
const midnight = (t: number) => DAY + Math.floor((t - DAY) / DAY_MS) * DAY_MS

/** What a scan says at its moment, as the plates read it: its sums, the hour's turns and calls, the sessions' newest
 *  turns, the day's failures, and every kind's usual. */
function scanSays(sc: ReturnType<DayScan['of']>, now: number, rows: readonly { data: Record<string, unknown> }[]) {
  const argvs = [undefined, ['sh', '-c', 'sleep 3'], ['cargo', 'build'], ['sh', '-c', 'echo hi; make']]
  const cids = [...new Set(rows.map((r) => r.data.correlation_id as string).filter(Boolean))]
  return said({
    spent: sc.spent, calls: sc.calls, hours: sc.hours, bySession: sc.bySession, spentNodes: [...sc.spentNodes].sort(), last15: sc.last15,
    newest: sc.newest, failures: sc.failures.list(() => false),
    ended: sc.endedAfter(now - 3 * HOUR_MS).map(({ ms, turn, session, at }) => ({ ms, turn, session, at })),
    settled: sc.settledAfter(now - 3 * HOUR_MS).map(({ cid, ms, at }) => ({ cid, ms, at })),
    usuals: ['proc.run', 'fs.read', 'fs.write'].flatMap((t) => argvs.map((a) => sc.usuals.of(t, a))), turn: sc.usuals.turn,
    rows: cids.map((c) => [sc.tool(c), sc.argv(c), sc.l1(c)]),
  })
}

test('a kept scan says what a fresh read says, as the ledger grows and the moment moves on, back, and over midnight', () => {
  const model = fleet(['ses_a', 'ses_b', 'ses_c', 'ses_d', 'ses_e'].map((id) => ({ id, state: id < 'ses_c' ? 'running' : 'waiting' })))
  for (const seed of [1, 2, 3]) {
    const rows = randomLedger(seed, 2400, DAY - 20 * HOUR_MS)
    const r = seeded(seed + 100)
    const day = new DayScan()
    const scan = new DayScan()
    let n = 150
    let now = rows[n - 1].at_unix_ms
    for (let step = 0; n < rows.length; step++) {
      n = Math.min(rows.length, n + 1 + Math.floor(r() * 50))
      // The moment: on with the rows (and sometimes a few minutes past them), or back (a scrub) now and then.
      now = step % 13 === 12 ? now - Math.floor(r() * 5 * HOUR_MS) : Math.max(now, rows[n - 1].at_unix_ms + Math.floor((r() - 0.3) * 6 * MIN))
      const seen = rows.slice(0, n)
      // The calls read: some of the ledger's, named a tool of their own, changing as they are read again. A call whose
      // plan row the window has let go takes its tool from them.
      const actions = seen.filter((x) => x.kind === 'action.planned' && r() < 0.3).map((x) => action(x.data.correlation_id as string, { tool: 'fs.read', settled_at_ms: x.at_unix_ms }))
      const input = at({ model, actions, rows: seen, now, dayStart: midnight(now), dayEnd: midnight(now) + DAY_MS })
      const where = `seed ${seed}, step ${step}, ${n} rows, the moment ${new Date(now).toISOString()}`
      assert.deepEqual(said(watchOf(input, day)), said(watchOf(input)), where)
      assert.deepEqual(scanSays(scan.of(seen, now, midnight(now), 24), now, seen), scanSays(new DayScan().of(seen, now, midnight(now), 24), now, seen), where)
    }
  }
})

test("slow holds a job that echoes first against its working program's runs, never against every echo-first job", () => {
  // Three echo-first builds and three night sleeps settled today; an echo-first sleep running now is a run of `sleep`.
  const job = (k: number, argv: string[], ms: number) => [
    row(NOW - HOUR_MS + k * MIN, 'action.planned', 'ses_a', { correlation_id: `act_${k}`, tool: 'proc.run' }),
    row(NOW - HOUR_MS + k * MIN, 'tool.job_started', 'ses_a', { correlation_id: `act_${k}`, argv }),
    row(NOW - HOUR_MS + k * MIN + 1, 'action.succeeded', 'ses_a', { correlation_id: `act_${k}`, duration_ms: ms }),
  ]
  const rows = [
    ...[1, 2, 3].flatMap((k) => job(k, ['sh', '-c', `echo building ${k}; make -j${k}`], 3000)),
    ...[4, 5, 6].flatMap((k) => job(k, ['sh', '-c', `sleep ${k}0000`], 9_000_000)),
  ]
  const sc = new DayScan().of(rows, NOW, DAY, 24)
  const usual = sc.usuals.of('proc.run', ['sh', '-c', 'echo night build started; sleep 21000; echo night build done'])
  assert.deepEqual(usual, { ms: 9_000_000, n: 3, of: '`sleep` jobs' })
  assert.deepEqual(sc.usuals.of('proc.run', ['sh', '-c', 'echo go; make -j9']), { ms: 3000, n: 3, of: '`make` jobs' })
})

test('a kept scan lets a failure go when the last 24 hours pass it, with no new row to tell it', () => {
  // A call failed at 22:00 last night; the moment moves on through today, the same local day, and no row comes.
  const rows = [
    row(DAY - 2 * HOUR_MS, 'action.planned', 'ses_a', { correlation_id: 'act_f', tool: 'fs.read' }),
    row(DAY - 2 * HOUR_MS + 1, 'action.failed', 'ses_a', { correlation_id: 'act_f', producer: 'inproc:fs.read', duration_ms: 5 }),
  ]
  const day = new DayScan()
  const wrong = (now: number) => watchOf(at({ rows, now }), day).wrong.count
  // A minute before it is a day old, and a minute after: the window has moved two minutes.
  assert.equal(wrong(DAY + 22 * HOUR_MS - MIN), 1, 'at 21:59, 23 h 59 m on: in the last day')
  assert.equal(wrong(DAY + 22 * HOUR_MS + MIN), 0, 'at 22:01, a day and a minute on: gone, as a fresh read says')
  assert.equal(watchOf(at({ rows, now: DAY + 22 * HOUR_MS + MIN })).wrong.count, 0)
})

test("a kept scan counts a call again when its plan row's time comes after it settled", () => {
  // Rows in the ledger's order, a skew apart in time: act_x's plan and job rows carry a time after its settle row's.
  const T = NOW - 2 * HOUR_MS
  const sleep = ['sh', '-c', 'sleep 9']
  const rows = [
    ...[1, 2, 3].flatMap((k) => [
      row(T - 10 * MIN + k, 'action.planned', 'ses_a', { correlation_id: `act_${k}`, tool: 'proc.run' }),
      row(T - 10 * MIN + k, 'tool.job_started', 'ses_a', { correlation_id: `act_${k}`, argv: sleep }),
      row(T - 9 * MIN + k, 'action.succeeded', 'ses_a', { correlation_id: `act_${k}`, duration_ms: 1000 * k }),
    ]),
    row(T + 2 * MIN, 'action.planned', 'ses_a', { correlation_id: 'act_x', tool: 'proc.run' }),
    row(T + 2 * MIN, 'tool.job_started', 'ses_a', { correlation_id: 'act_x', argv: sleep }),
    row(T, 'action.succeeded', 'ses_a', { correlation_id: 'act_x', duration_ms: 9000 }),
  ]
  const usual = (sc: ReturnType<DayScan['of']>) => sc.usuals.of('proc.run', sleep)
  const day = new DayScan()
  // At T + 1 minute the plan's time has not come: act_x settled with no tool yet, and is not counted.
  assert.deepEqual(usual(day.of(rows, T + MIN, DAY, 24)), { ms: 2000, n: 3, of: 'runs of this command' })
  // Two minutes on, the plan is read: act_x counts as a run of its command, as a fresh read says.
  assert.deepEqual(usual(day.of(rows, T + 3 * MIN, DAY, 24)), { ms: 2500, n: 4, of: 'runs of this command' })
  assert.deepEqual(usual(new DayScan().of(rows, T + 3 * MIN, DAY, 24)), { ms: 2500, n: 4, of: 'runs of this command' })
})

test("a kept scan reads only the rows since its last: a busy day's recompute costs its new rows, not the day", () => {
  // 50,000 rows a day, the busy day the watch is held to; the moment is the newest row's, as live.
  const rows = busyDay(50_400, NOW)
  const model = fleet(Array.from({ length: 40 }, (_, k) => ({ id: `ses_${k}`, state: k % 8 ? 'waiting' : 'running' })))
  const day = new DayScan()
  const input = (n: number) => at({ model, rows: rows.slice(0, n), now: rows[n - 1].at_unix_ms })
  watchOf(input(50_000), day)
  const first = day.read
  assert.ok(first > 40_000, `the first recompute reads the day: ${first} rows`)
  const kept: number[] = []
  const fresh: number[] = []
  for (let k = 1; k <= 30; k++) {
    const i = input(50_000 + k * 10)
    const read = day.read
    let t = performance.now()
    watchOf(i, day)
    kept.push(performance.now() - t)
    assert.equal(day.read - read, 10, 'each recompute reads its 10 new rows, not the day again')
    t = performance.now()
    watchOf(i)
    fresh.push(performance.now() - t)
  }
  // Timed side by side, so a loaded machine slows both: a kept recompute is a small part of a fresh read of the day
  // (about a tenth on a quiet machine, where it is 1 to 3 ms: the report's FAST).
  const [k, f] = [median(kept)!, median(fresh)!]
  assert.ok(k < f / 4, `a kept recompute takes ${k.toFixed(1)} ms at the median, a fresh read ${f.toFixed(1)} ms`)
})
