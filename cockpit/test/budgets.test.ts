// Money as the cockpit reads it (`src/lib/budgets.ts`, M7 42b), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { burnPerHour, dayCeilingView, flatten, handsLines, HOUR_MS, limitWords, questionsWaiting, recentResets, sessionsOf, sums } from '../src/lib/budgets.ts'

const row = (id: string, o: Record<string, unknown> = {}) => ({
  execution_id: `exe_${id}`, session_id: `ses_${id}`, kind: 'conversation', state: 'waiting', limit_usd: 10, limit_from: 'config',
  spent_usd: 0, reserved_usd: 0, held_unknown_usd: 0, available_usd: 10, lifetime_usd: 0, resets: 0, ...o,
}) as any

// A parent that has spent 3 with a task of 1 under it (the task's spend is the parent's too), and a lone session.
const task = row('t', { kind: 'task', limit_usd: 2, limit_from: 'carve', limit_by: 'ses_a', parent: 'exe_a', spent_usd: 1, available_usd: 1, lifetime_usd: 1, carve_held_usd: 1 })
const tree = [row('a', { spent_usd: 3, reserved_usd: 2, available_usd: 5, lifetime_usd: 7, tasks: [task] }), row('b', { spent_usd: 0.5, available_usd: 9.5, lifetime_usd: 0.5 })]

test('the tree flattens with each task under its parent', () => {
  assert.deepEqual(flatten(tree).map((f) => [f.row.session_id, f.depth, f.parent]), [['ses_a', 0, undefined], ['ses_t', 1, 'ses_a'], ['ses_b', 0, undefined]])
  assert.deepEqual([...sessionsOf(tree[0])].sort(), ['ses_a', 'ses_t'])
})

test('the totals are the top rows: a task is never counted twice', () => {
  const s = sums(tree)
  assert.equal(s.spent, 3.5)
  assert.equal(s.limit, 20)
  assert.equal(s.reserved, 2)
  assert.equal(s.available, 14.5)
  // The lifetime adds every session's own.
  assert.equal(s.lifetime, 8.5)
})

test('burn per hour counts the window before the end, and the sessions asked for', () => {
  const end = 10 * HOUR_MS
  const calls = [
    { at: end - 2 * HOUR_MS, cost: 100, session_id: 'ses_a' }, // before the window
    { at: end - HOUR_MS, cost: 50, session_id: 'ses_a' }, // at the window's start: outside it
    { at: end - 1000, cost: 0.4, session_id: 'ses_a' },
    { at: end - 2000, cost: 0.1, session_id: 'ses_t' },
    { at: end - 3000, cost: 9, session_id: 'ses_b' }, // another session
    { at: end + 1, cost: 7, session_id: 'ses_a' }, // after the moment
  ]
  assert.equal(burnPerHour(calls, new Set(['ses_a', 'ses_t']), end), 0.5)
  assert.equal(burnPerHour(calls, new Set(['ses_a']), end, HOUR_MS / 2), 0.8)
  assert.equal(burnPerHour([], new Set(['ses_a']), end), 0)
})

test('recent resets: newest first, to the moment, with who and what was spent', () => {
  const rows = [
    { position: 1, at_unix_ms: 100, kind: 'budget.reset', session_id: 'ses_a', data: { execution_id: 'exe_a', by: 'cli', spent_before_usd: 4 } },
    { position: 2, at_unix_ms: 200, kind: 'turn.started', session_id: 'ses_a', data: {} },
    { position: 3, at_unix_ms: 300, kind: 'budget.reset', session_id: 'ses_b', data: { by: 'web#1', spent_before_usd: 1.5 } },
  ] as any
  assert.deepEqual(recentResets(rows).map((r) => [r.session, r.by, r.before]), [['ses_b', 'web#1', 1.5], ['ses_a', 'cli', 4]])
  assert.equal(recentResets(rows, 8, 250).length, 1)
  assert.equal(recentResets(rows, 1).length, 1)
})

test('a limit says where it comes from', () => {
  assert.equal(limitWords({ limit_from: 'config' }), 'the config’s limit')
  assert.equal(limitWords({ limit_from: 'place', limit_by: '#pier' }), 'the place’s ceiling (#pier)')
  assert.equal(limitWords({ limit_from: 'carve', limit_by: 'ses_parent_long_id' }), 'carved from its parent (ses_parent_l)')
  assert.equal(limitWords({ limit_from: 'pinned' }), 'pinned when it opened')
})

test('the hands say runaway plainly, and only while it holds', () => {
  const h = { groups_open: 0, running_lambda: 1, running_fargate: 2, reserved_micros: 1_500_000, hour_micros: 2_000_000, hour_line_micros: 5_000_000, reaper_failures: 0, read_at_unix_ms: 0, runaway: 'spend ran away', runaway_until_unix_ms: 2_000_000_000_000 } as any
  const lines = handsLines(h, 1_900_000_000_000)
  assert.equal(lines[0].tone, 'fault')
  assert.match(lines[0].text, /^runaway mode: spend ran away; new AWS actions that reserve are refused until /)
  assert.equal(lines[1].text, '3 hands running (1 Lambda, 2 Fargate), $1.50 reserved')
  assert.equal(lines[2].text, 'this hour $2.00 of its $5.00 line')
  assert.equal(handsLines(h, 2_100_000_000_000)[0].text.startsWith('runaway'), false)
})

test("a budget question waiting shows above Money's river, only while one waits; the Budgets panel points up to it", () => {
  // None waits: nothing above the river.
  assert.deepEqual(questionsWaiting(tree), [])
  // A task's question and a session's, in reading order.
  const asked = (id: string) => ({ correlation_id: `act_${id}`, needs_usd: 0.4, asked_at_ms: 1 })
  const waits = [row('a', { question: asked('a'), tasks: [row('t', { kind: 'task', question: asked('t') })] }), row('b')]
  assert.deepEqual(questionsWaiting(waits as any).map((f) => [f.row.session_id, f.row.question?.correlation_id]), [['ses_a', 'act_a'], ['ses_t', 'act_t']])
  // Money: the questions first, then the tiles and the river; the Budgets panel keeps a line that points up to them.
  const money = readFileSync(new URL('../src/views/Money.tsx', import.meta.url), 'utf8')
  const above = money.indexOf('<BudgetQuestions past={world ? world.t : null} />')
  assert.ok(above > 0 && above < money.indexOf('<ChartPanel id="river"') && above < money.indexOf('<StatTile'), 'the questions come before the tiles and the river')
  const budgets = readFileSync(new URL('../src/components/Budgets.tsx', import.meta.url), 'utf8')
  const panel = budgets.slice(budgets.indexOf('export function Budgets('), budgets.indexOf('export function BudgetQuestions('))
  assert.doesNotMatch(panel, /<ConfirmCard/, 'the panel shows no second card')
  assert.match(panel, /'their cards are'\} above the river/, 'its line says where they are')
  const questions = budgets.slice(budgets.indexOf('export function BudgetQuestions('))
  assert.match(questions, /if \(!waiting\.length\) return null/)
  assert.match(questions, /<ConfirmCard key=\{q\.correlation_id\} c=\{card\} \/>/)
})

// The day ceiling (theseus-kp20): a quiet bar under it, a clear stop once reached.
test('the day ceiling is a quiet bar under it and a stop once reached', () => {
  const day = { day: '2026-10-08', ceiling_usd: 200, spent_usd: 50, held_usd: 10, reached: false, turns_at_ms: 1, turns_at: '2026-10-09 00:00' }
  const under = dayCeilingView(day)
  assert.equal(under.stopped, false)
  assert.equal(under.tone, 'ok')
  assert.equal(under.share, 0.3)
  assert.equal(under.text, '$50.00 of $200.00 today')
  assert.match(under.detail, /\$10\.00 held by calls in flight · the day turns at 2026-10-09 00:00 local time/)
  assert.equal(dayCeilingView({ ...day, spent_usd: 165 }).tone, 'wait')
  const stopped = dayCeilingView({ ...day, spent_usd: 199.5, held_usd: 0, reached: true, reached_at_ms: 5 })
  assert.equal(stopped.stopped, true)
  assert.equal(stopped.tone, 'fault')
  assert.equal(stopped.share, 1)
  assert.match(stopped.text, /^stopped: today’s model spend reached the \$200\.00 daily ceiling/)
  assert.match(stopped.detail, /no model call until 2026-10-09 00:00 local time · raise \[kernel\] daily_spend_ceiling_usd/)
})
