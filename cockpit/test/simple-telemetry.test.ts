// The telemetry board's numbers (issue 17): what each pane says, over invented health, calls and ledger rows.
import test from 'node:test'
import assert from 'node:assert/strict'
import type { ExecutionInfo, Health, LedgerEntry } from '../src/protocol.gen/index.ts'
import type { ProviderCall } from '../src/lib/calls.ts'
import { autoRange, failuresOf, headroomOf, headroomWord, latencyOf, machineOf, quantile, rangeStart, runningOf, spendOf } from '../src/simple/telemetry/board.ts'

const NOW = Date.parse('2026-11-14T15:00:00')
const MIN = 60_000
const usage = { input_tokens: 100, output_tokens: 50, cache_read_input_tokens: 400, cache_creation_input_tokens: 0 }
const call = (over: Partial<ProviderCall>): ProviderCall => ({ at: NOW - MIN, position: 1, session_id: 'ses_a', turn_id: null, provider: 'zai', model: 'glm', cost: 0.1, loop: 0, stop: 'end_turn', usage, ...over })
const row = (kind: string, at: number, position = 1): LedgerEntry => ({ position, at_unix_ms: at, kind, session_id: null, turn_id: null, data: {} } as unknown as LedgerEntry)
const health = (over: Record<string, unknown> = {}) => ({
  uptime_secs: 3700, ledger_rows: 12345, provider_errors: 2, wakes: [{ due_local: '16:00' }],
  kernel: { accepting: true, admission_ceiling: 8, turns_held: 1, executions_by_state: { running: 3, waiting: 2, succeeded: 40 }, actions_by_state: { planned: 2, running: 1, succeeded: 90, failed: 4 } },
  disk: { path: '/x', state: 'ok', free_mb: 20_480, total_mb: 102_400, warn_mb: 5_000, floor_mb: 1_000 },
  resident: { rss_bytes: 3 * 1024 ** 3 }, store: { refused_records: 0 }, secrets: { ready: ['a', 'b'], resolving: [], failed: [] }, config: { state: 'confirmed' },
  ...over,
}) as unknown as Health
const fmt = { bytes: (n: number) => `${(n / 1024 ** 3).toFixed(1)} GB`, uptime: (s: number) => `${Math.floor(s / 3600)}h` }

test('what is running reads the kernel the way the Bridge tiles do', () => {
  const f = Object.fromEntries(runningOf(health(), [{}, {}] as ExecutionInfo[], 1).map((x) => [x.key, x]))
  assert.equal(f.running.n, 3)
  assert.equal(f.running.note, '2 waiting · 40 succeeded')
  assert.equal(f.tools.n, 3, 'planned and running are in flight; succeeded and failed are settled')
  assert.equal(f.tools.note, '90 done · 4 failed')
  assert.equal(f.approvals.tone, 'wait')
  assert.equal(f.held.note, 'of 8 Theseus will run at once')
  assert.equal(f.wakes.note, 'next 16:00')
  assert.equal(f.executions.n, 2)
  assert.equal(runningOf(undefined, [], 0).find((x) => x.key === 'running')!.n, 0, 'before health arrives, zero not a crash')
})

test('quantile is the nearest rank and says nothing for nothing', () => {
  assert.equal(quantile([], 0.5), undefined)
  assert.equal(quantile([5, 1, 9, 3], 0.5), 3)
  assert.equal(quantile([1, 2, 3, 4, 5, 6, 7, 8, 9, 10], 0.95), 10)
})

test('latency by model: medians and the slow tail, the slowest median first, calls without timing left out of the numbers', () => {
  const calls = [
    ...[100, 200, 300].map((t, i) => call({ model: 'fast', first_token_ms: t, total_ms: t * 2, position: i })),
    call({ model: 'slow', first_token_ms: 900, total_ms: 4000 }),
    call({ model: 'untimed' }),
  ]
  const rows = latencyOf(calls)
  assert.deepEqual(rows.map((r) => r.model), ['slow', 'fast', 'untimed'])
  assert.equal(rows[1].first50, 200)
  assert.equal(rows[1].total95, 600)
  assert.equal(rows[2].calls, 1)
  assert.equal(rows[2].first50, undefined)
})

test('headroom: the newest call that reported limits, the tightest provider first, with its word', () => {
  const calls = [
    call({ provider: 'a', at: 1, rate: { requests_limit: 100, requests_remaining: 90, tokens_limit: 1000, tokens_remaining: 100 } }),
    call({ provider: 'a', at: 2, rate: { requests_limit: 100, requests_remaining: 60, tokens_limit: 1000, tokens_remaining: 800 } }),
    call({ provider: 'b', at: 3, rate: { requests_limit: 100, requests_remaining: 5 } }),
    call({ provider: 'c', at: 4, rate: {} }),
    call({ provider: 'd', at: 5 }),
  ]
  const rows = headroomOf(calls)
  assert.deepEqual(rows.map((r) => r.provider), ['b', 'a', 'c'], 'd never reported limits; c reported none and sorts last')
  assert.equal(rows[0].word, 'nearly out')
  assert.equal(rows[1].fraction, 0.6)
  assert.equal(rows[1].word, 'plenty left')
  assert.equal(rows[2].word, 'this provider sends no limits')
  assert.equal(headroomWord(0.3), 'running low')
})

test('spend: the range by model, and the hour and the day over every call', () => {
  const midnight = new Date(NOW); midnight.setHours(0, 0, 0, 0)
  const old = call({ at: midnight.getTime() - MIN, cost: 5 })
  const earlier = call({ at: midnight.getTime() + MIN, cost: 1, model: 'opus' })
  const recent = call({ at: NOW - 10 * MIN, cost: 0.5 })
  const s = spendOf([old, earlier, recent], [earlier, recent], NOW)
  assert.equal(s.total, 1.5)
  assert.equal(s.hour, 0.5)
  assert.equal(s.today, 1.5)
  assert.deepEqual(s.byModel.map((r) => [r.model, r.cost, r.tokens]), [['opus', 1, 550], ['glm', 0.5, 550]])
})

test('failures: only fault rows, counted by kind, the newest twelve whole', () => {
  const rows = [row('turn.started', 1), row('tool.failed', 2), row('tool.failed', 3), row('approval.refused', 4), row('provider.call', 5), ...Array.from({ length: 14 }, (_, i) => row('turn.failed', 10 + i))]
  const f = failuresOf(rows)
  assert.equal(f.total, 17)
  assert.deepEqual(f.kinds.map((k) => [k.kind, k.n]), [['turn.failed', 14], ['tool.failed', 2], ['approval.refused', 1]])
  assert.equal(f.recent.length, 12)
  assert.equal(f.recent[0].at_unix_ms, 23, 'newest first')
  assert.deepEqual(failuresOf([]).kinds, [])
})

test('the machine: each fact with a state, and a fact Theseus did not report is not listed', () => {
  const m = Object.fromEntries(machineOf(health(), fmt).map((x) => [x.key, x]))
  assert.equal(m.up.value, '1h')
  assert.equal(m.disk.state, 'ok')
  assert.equal(m.disk.fraction, 0.2)
  assert.equal(m.memory.value, '3.0 GB')
  assert.equal(m.errors.state, 'wait')
  const low = Object.fromEntries(machineOf(health({ disk: { free_mb: 900, total_mb: 1000, warn_mb: 5, floor_mb: 1000 }, store: { refused_records: 3 }, secrets: { ready: [], resolving: [], failed: [{ name: 'k', error: 'x' }] }, resident: undefined }), fmt).map((x) => [x.key, x]))
  assert.equal(low.disk.state, 'fault', 'at or under the floor')
  assert.equal(low.store.value, '3 records refused')
  assert.equal(low.secrets.state, 'fault')
  assert.equal(low.memory, undefined)
  assert.deepEqual(machineOf(undefined, fmt), [])
})

test('the range: the smallest window with 25 rows, and the first row for all of it', () => {
  const rows = Array.from({ length: 30 }, (_, i) => row('turn.started', NOW - 3 * 3600_000 + i, i))
  assert.equal(autoRange(rows, NOW), '6h', 'none in the last half hour, 30 in the last six hours')
  assert.equal(autoRange(rows.slice(0, 5), NOW), 'all')
  assert.equal(rangeStart('6h', NOW, rows), NOW - 6 * 3600_000)
  assert.equal(rangeStart('all', NOW, rows), rows[0].at_unix_ms, 'the first row, when it is older than a minute')
})
