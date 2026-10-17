// The cockpit's notice policy (`src/lib/notices.ts`, theseus-go4ig), run by `npm test`: the table of
// `theseus_protocol::notices`, as the page reads it.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { deliver, Notifier, policy, reminderNotice, RULES } from '../src/lib/notices.ts'

const hm = (ms: number) => `t${ms / 1000}`
let position = 0
const view = (o: Record<string, unknown> = {}) => ({
  id: 'exe_beta', kind: 'conversation', position: ++position, at_ms: 1_000, root: 'exe_beta', session_id: 'ses_beta', title: 'Beta summary',
  state: 'running', attention: { level: 'working', label: 'turn 4', since_ms: 0 }, started_at_ms: 0, state_since_ms: 0, ...o,
}) as any
const question = (o: Record<string, unknown> = {}) => ({
  id: 'act_q1', kind: 'approval', asked_by: { id: 'exe_beta', kind: 'conversation', title: 'Beta summary' }, prompt: 'fs.write create notes.txt',
  options: [], floor: false, may_answer: true, tool: 'fs.write', asked_at_ms: 1_000, ...o,
})
const asking = (o: Record<string, unknown> = {}) =>
  view({ state: 'needs_you', attention: { level: 'needs_you', label: 'confirm', since_ms: 1_000 }, question: question(), doing: { what: 'asking', label: '', since_ms: 0, turn: 1, loop_index: 0 }, ...o })

test('a progress change makes no notice', () => {
  const a = view()
  const b = view({ doing: { what: 'turn', label: 'turn 5', since_ms: 0, turn: 5, loop_index: 1 }, attention: { level: 'working', label: 'turn 5', since_ms: 0 }, at_ms: 2_000, cost: { spent_usd: 0.1, limit_usd: 5 } })
  assert.equal(policy(a, b, RULES, hm), undefined)
  assert.equal(policy(undefined, a, RULES, hm), undefined)
  const n = new Notifier()
  n.seed([a])
  assert.equal(n.observe(b, RULES, hm), undefined)
})

test('a question interrupts, and its answer is quiet and retracts the ping', () => {
  const q = policy(view(), asking(), RULES, hm)!
  assert.deepEqual([q.urgency, q.why, q.question], ['interrupt', 'asked', 'act_q1'])
  assert.equal(q.line, '● Beta summary asks: fs.write create notes.txt? · expires t' .replace(/ · expires.*/, ''))
  const done = policy(asking(), view({ at_ms: 3_000 }), RULES, hm)!
  assert.deepEqual([done.urgency, done.why, done.retracts], ['quiet', 'answered', 'act_q1'])
  assert.deepEqual(deliver(done, { seen: 0, away_ms: 0, quiet: false, sound: true }), { as: 'nothing' })
})

test('a question reminds once, at half its life', () => {
  const q = question({ asked_at_ms: 0, expires_at_ms: 600_000 })
  const before = asking({ question: q, at_ms: 100_000 })
  const at = asking({ question: q, at_ms: 300_000 })
  const r = policy(before, at, RULES, hm)!
  assert.deepEqual([r.urgency, r.why], ['interrupt', 'reminder'])
  assert.match(r.line, /still waiting, 5 min left/)
  assert.equal(policy(at, asking({ question: q, at_ms: 400_000 }), RULES, hm), undefined)
  const n = new Notifier()
  n.seed([asking({ question: q, at_ms: 100_000 })])
  assert.equal(n.nextDue(RULES), 300_000)
  assert.equal(n.due(200_000, RULES, hm).length, 0)
  assert.equal(n.due(300_000, RULES, hm).length, 1)
  assert.equal(n.due(400_000, RULES, hm).length, 0)
})

test('a failure and a block interrupt, on the transition only', () => {
  const failed = view({ state: 'failed', attention: { level: 'needs_you', label: 'failed: refused', since_ms: 0 }, at_ms: 2_000 })
  const f = policy(view(), failed, RULES, hm)!
  assert.deepEqual([f.urgency, f.why, f.line], ['interrupt', 'failed', '✗ Beta summary failed: refused'])
  assert.equal(policy(failed, view({ ...failed, position: ++position }), RULES, hm), undefined)
  const job = view({ kind: 'job', state: 'failed' })
  assert.equal(policy(view({ kind: 'job' }), job, RULES, hm)?.urgency ?? 'none', 'none')
  const blocked = view({ state: 'needs_you', attention: { level: 'needs_you', label: 'budget exhausted', since_ms: 0 } })
  assert.equal(policy(view(), blocked, RULES, hm)!.why, 'blocked')
})

test('a finish interrupts when flagged or long, and informs otherwise', () => {
  const working = view({ attention: { level: 'working', label: '', since_ms: 0 } })
  const ready = (o: Record<string, unknown>) => view({ state: 'ready', attention: { level: 'ready', label: 'ready', since_ms: 0 }, ...o })
  assert.equal(policy(working, ready({ at_ms: 400_000 }), RULES, hm)!.urgency, 'interrupt')
  assert.equal(policy(working, ready({ at_ms: 60_000 }), RULES, hm)!.urgency, 'inform')
  assert.equal(policy(working, ready({ at_ms: 60_000, flagged: true }), RULES, hm)!.urgency, 'interrupt')
  assert.equal(policy(working, ready({ at_ms: 60_000, parent: 'exe_other' }), RULES, hm), undefined)
  const quiet = deliver(policy(working, ready({ at_ms: 400_000 }), RULES, hm)!, { seen: 0, away_ms: 0, quiet: false, sound: true })
  assert.deepEqual(quiet, { as: 'ping', sound: false })
})

test('a task\'s report informs, and interrupts for one the owner flagged', () => {
  const task = (o: Record<string, unknown>) => view({ id: 'exe_task', kind: 'task', parent: 'exe_beta', title: 'Sound the channel', ...o })
  const end = { state: 'done', report: { outcome: 'done', first_line: 'depth is 12 m' }, at_ms: 5_000 }
  const n = policy(task({}), task(end), RULES, hm)!
  assert.deepEqual([n.urgency, n.why, n.line], ['inform', 'reported', '📋 Sound the channel reported: depth is 12 m'])
  assert.equal(policy(task({ flagged: true }), task({ ...end, flagged: true }), RULES, hm)!.urgency, 'interrupt')
})

test('a set reminder interrupts; a model\'s check-in does not', () => {
  const wake = { wake_id: 'wak_1', short: '1', session_id: 'ses_beta', execution_id: 'exe_beta', purpose: 'remind', note: 'Water the ferns\nmore', due_at_ms: 10_000, late_ms: 0, answers: [] } as any
  const n = reminderNotice(wake)!
  assert.deepEqual([n.urgency, n.line], ['interrupt', '⏰ Water the ferns'])
  assert.equal(reminderNotice({ ...wake, purpose: 'check' }), undefined)
})

test('what the viewer is looking at is no ping, and a quiet viewer gets no sound', () => {
  const q = policy(view(), asking(), RULES, hm)!
  assert.deepEqual(deliver(q, { focused: 'ses_beta', seen: 0, away_ms: 0, quiet: false, sound: true }), { as: 'nothing' })
  assert.deepEqual(deliver(q, { focused: 'ses_beta', seen: 0, away_ms: 5, quiet: false, sound: true }), { as: 'ping', sound: true })
  assert.deepEqual(deliver(q, { seen: 0, away_ms: 0, quiet: true, sound: true }), { as: 'ping', sound: false })
})

test('a view not newer than the held one is ignored', () => {
  const n = new Notifier()
  const a = view()
  n.seed([a])
  assert.equal(n.observe({ ...asking(), position: a.position }, RULES, hm), undefined)
})
