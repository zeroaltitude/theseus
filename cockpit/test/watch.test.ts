// The Ship's watch (`src/ship/watch.ts`, theseus-hnof), run by `npm test`: the five plates' numbers, lines, and
// overlays, from a small fleet, its calls, its questions, and its ledger rows, at a fixed moment.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { argvWords, DAY_MS, dollars, HOUR_MS, median, platesOf, resultWords, shortPaths, span, watchOf, type WatchInput } from '../src/ship/watch.ts'

// 14:00 on a day whose local midnight is 07:00 UTC (a UTC-7 clock): the moment's hour is the day's 14th.
const DAY = Date.UTC(2026, 9, 6, 7, 0, 0)
const NOW = DAY + 14 * HOUR_MS
const MIN = 60_000

interface V { id: string; title?: string; state?: string; attention?: { level: string; label: string; since_ms: number } }
interface L { id: string; session: string; kind: 'user' | 'model' | 'call' | 'result'; at: number; turn?: string; tool?: string; cid?: string; failed?: boolean; l1?: boolean; running?: boolean; preview?: string }

/** A model with only what the watch reads: vessels, lights, and their lookups. */
function fleet(vs: V[], ls: L[] = []) {
  const vessels = vs.map((v) => ({ id: v.id, title: v.title ?? v.id, state: v.state ?? 'waiting', attention: v.attention, kind: 'conversation', rig: 'anchor' }))
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
  assert.equal(w.wrong.link.to, `/ledger?kind=action.failed,turn.failed,budget.asked,execution.budget_exhausted&from=${NOW - DAY_MS}&to=${NOW}`)
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
