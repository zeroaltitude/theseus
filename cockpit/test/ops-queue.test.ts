// The needs-you queue (theseus-jid7t.3): every kind of item, longest waiting first, one card per question, and a
// cursor a new item never moves.
import test from 'node:test'
import assert from 'node:assert/strict'
import { answerOf, arrived, choiceOption, comingUp, grantKey, itemFor, keepCursor, queueOf, step } from '../src/ops/queue.ts'
import { itemFacts } from '../src/ops/facts.ts'
import { board, confirm, execution, HOUR, MIN, NOW, record, view } from './ops-fixtures.ts'

const titleOf = (s: string) => `«${s}»`

function full() {
  const b = board()
  return queueOf({
    confirms: [...b.confirms, confirm('act_b1', { tool: 'budget.reset', budget: { spent_usd: 2, limit_usd: 2, needed_usd: 0.3, lifetime_usd: 5 } as never, requested_at_ms: NOW - 5 * MIN, expires_at_ms: 0 }),
      confirm('act_ch', { change: { objective: 'map only the north quay' } as never, requested_at_ms: NOW - 3 * MIN })],
    views: b.views,
    records: [record('tsk_x', { session: 'ses_t2', proposal: { abandon: true, by: 'ses_t2', card: 'act_px', base_version: 1, at_ms: NOW - 90 * MIN } }),
      record('tsk_y', { proposal: { objective: 'x', by: 'ses_t1', card: 'act_ch', base_version: 1, at_ms: NOW - 3 * MIN } })],
    groups: b.groups,
    asks: [{ ask: { ask_id: 'ask_1', session_id: 'ses_home', question: 'Is this about the quay or the tides?', options: [], message: 'the north wall', expires_ms: NOW + 5 * MIN }, heard_ms: NOW - MIN }],
    titleOf,
  })
}

test('every kind of item, the longest waiting first', () => {
  const q = full()
  assert.deepEqual(q.map((i) => [i.key, i.kind]), [
    ['f:exe_t5', 'failure'], // two hours
    ['q:act_px', 'change'], // ninety minutes: a proposal not yet in the list
    ['s:tide-tables', 'seal'], // fifty minutes
    ['q:act_q1', 'question'], // forty minutes
    ['q:act_c1', 'approval'], // twenty minutes
    ['q:act_b1', 'budget'],
    ['q:act_ch', 'change'], // the confirm, once: its record's proposal is the same card
    ['a:ask_1', 'ask'],
  ])
  const by = new Map(q.map((i) => [i.key, i]))
  assert.equal(by.get('q:act_c1')!.owner, 'task Map the quay walls')
  assert.equal(by.get('q:act_px')!.title, 'Abandon tsk_x')
  assert.equal(by.get('f:exe_t5')!.title, 'the chart server refused')
  assert.equal(by.get('q:act_q1')!.owner, '«ses_t1»')
})

test('an approval’s question view is its confirm, listed once; a question without choices waits for its confirm', () => {
  const q = queueOf({ confirms: [confirm('act_c9')], views: [view('act_c9', 'question', 'needs_you', { question: { id: 'act_c9', kind: 'approval', prompt: 'write?', options: [], asked_at_ms: 1 } as never })] })
  assert.deepEqual(q.map((i) => i.key), ['q:act_c9'])
  assert.equal(queueOf({ confirms: [], views: [view('act_c8', 'question', 'needs_you', { question: { id: 'act_c8', kind: 'approval', prompt: 'write?', options: [], asked_at_ms: 1 } as never })] }).length, 0)
})

test('an owner_ask in the confirm list is a question with its choices, once', () => {
  const b = board()
  const q = queueOf({ confirms: [confirm('act_q1', { tool: 'owner_ask', session_id: 'ses_t1' })], views: b.views })
  const asks = q.filter((i) => i.key === 'q:act_q1')
  assert.equal(asks.length, 1)
  assert.equal(asks[0].kind, 'question')
  assert.equal(asks[0].title, 'Which pier first?')
  assert.deepEqual(answerOf(asks[0], { verb: 'choice', n: 1 })?.params, { question_id: 'act_q1', option: 'choice:0', note: undefined })
  assert.equal(answerOf(asks[0], { verb: 'approve' }), null, 'it is answered by a choice, never a plain yes')
})

test('a dismissed failure asks for no one; a failure that no longer needs you is not listed', () => {
  const failed = (over: object) => view('exe_f', 'task', 'failed', over, 'needs_you')
  assert.equal(queueOf({ confirms: [], views: [failed({ dismissed: true })] }).length, 0)
  assert.equal(queueOf({ confirms: [], views: [view('exe_f', 'task', 'failed', {}, 'idle')] }).length, 0)
  assert.equal(queueOf({ confirms: [], views: [failed({})] }).length, 1)
})

test('under the time machine the failures are the executions as they stood', () => {
  const q = queueOf({ confirms: [], views: [], executions: [execution('exe_a', 'failed'), execution('exe_b', 'complete'), execution('exe_c', 'budget_exhausted'), execution('exe_d', 'blocked')] })
  assert.deepEqual(q.map((i) => i.key).sort(), ['f:exe_a', 'f:exe_c', 'f:exe_d'])
  assert.equal(q[0].execution?.execution_id, q[0].key.slice(2))
})

test('a new item never moves the cursor: it slides in where its age puts it', () => {
  const b = board()
  const before = queueOf({ confirms: b.confirms, views: b.views })
  const cursor = 'q:act_c1'
  assert.deepEqual(before.map((i) => i.key), ['f:exe_t5', 'q:act_q1', 'q:act_c1'])
  // An older item arrives (three hours old) and a newer one: the order changes, the cursor stays put.
  const after = queueOf({ confirms: [...b.confirms, confirm('act_new', { requested_at_ms: NOW - MIN }), confirm('act_old', { requested_at_ms: NOW - 3 * HOUR })], views: b.views })
  assert.deepEqual(after.map((i) => i.key), ['q:act_old', 'f:exe_t5', 'q:act_q1', 'q:act_c1', 'q:act_new'])
  assert.equal(keepCursor(before.map((i) => i.key), after, cursor), cursor)
  assert.deepEqual(arrived(before.map((i) => i.key), after), ['q:act_old', 'q:act_new'], 'the counter pulses for these')
  // The item under the cursor is answered: the cursor goes to the one that took its place, else the last.
  const answered = after.filter((i) => i.key !== 'q:act_q1')
  assert.equal(keepCursor(after.map((i) => i.key), answered, 'q:act_q1'), 'q:act_c1')
  assert.equal(keepCursor(after.map((i) => i.key), after.slice(0, 2), 'q:act_new'), 'f:exe_t5')
  assert.equal(keepCursor([], [], 'q:x'), null)
  assert.equal(keepCursor(['q:x'], after, null), null, 'no cursor stays none')
})

test('the order is the same on every read', () => {
  const a = full().map((i) => i.key)
  const b = full().reverse()
  assert.deepEqual(queueOf({ confirms: b.flatMap((i) => (i.confirm ? [i.confirm] : [])), views: [] }).map((i) => i.key), a.filter((k) => ['q:act_c1', 'q:act_b1', 'q:act_ch'].includes(k)))
  const tie = queueOf({ confirms: [confirm('act_b', { requested_at_ms: 5 }), confirm('act_a', { requested_at_ms: 5 })], views: [] })
  assert.deepEqual(tie.map((i) => i.key), ['q:act_a', 'q:act_b'])
})

test('tab and shift-tab go round; numbered choices map to the wire’s', () => {
  const q = full()
  assert.equal(step(q, null, 1), q[0].key)
  assert.equal(step(q, null, -1), q[q.length - 1].key)
  assert.equal(step(q, q[q.length - 1].key, 1), q[0].key)
  assert.equal(step(q, q[0].key, -1), q[q.length - 1].key)
  assert.equal(step([], null, 1), null)
  const question = q.find((i) => i.kind === 'question')!
  assert.equal(choiceOption(question, 1), 'choice:0')
  assert.equal(choiceOption(question, 2), 'choice:1')
  assert.equal(choiceOption(question, 3), null, 'two choices')
  assert.equal(choiceOption(q[0], 1), null, 'a failure has no choices')
})

test('a selection or a deep link finds its item', () => {
  const q = full()
  assert.equal(itemFor(q, 'act_c1')?.key, 'q:act_c1')
  assert.equal(itemFor(q, 'q:act_c1')?.key, 'q:act_c1')
  assert.equal(itemFor(q, 'tide-tables')?.key, 's:tide-tables')
  assert.equal(itemFor(q, 'exe_t5')?.key, 'f:exe_t5')
  assert.equal(itemFor(q, 'ses_t1')?.kind, 'question', 'a session’s first item, the longest waiting')
  assert.equal(itemFor(q, null), undefined)
})

test('the compact card: glyph and word, verb, owner, age, and the expiry bar', () => {
  const q = full()
  const c = itemFacts(q.find((i) => i.key === 'q:act_c1')!, NOW)
  assert.equal(c.glyph, '◆')
  assert.equal(c.kind, 'approval')
  assert.equal(c.age, '20m ago')
  assert.ok(Math.abs((c.left ?? 0) - 1 / 3) < 1e-9, 'ten of its thirty minutes left')
  assert.ok(c.confirm, 'the full card is ConfirmCard’s')
  assert.equal(itemFacts(q.find((i) => i.kind === 'budget')!, NOW).left, undefined, 'a budget question does not expire')
})

test('coming up: the next three hours, soonest first, paused ones left out', () => {
  const b = board()
  const views = [...b.views, view('wak_p', 'wake', 'waiting', { next_at_ms: NOW + HOUR, paused: true }), view('wak_s', 'wake', 'waiting', { next_at_ms: NOW + 10 * MIN })]
  assert.deepEqual(comingUp(views, NOW).map((v) => v.id), ['wak_s', 'wak_r1'])
})

test('each answer is one protocol call, the one every surface makes; an item that cannot take it takes none', () => {
  const q = full()
  const by = new Map(q.map((i) => [i.kind === 'change' && i.proposal ? 'proposal' : i.kind, i]))
  const approval = q.find((i) => i.key === 'q:act_c1')!
  assert.deepEqual(answerOf(approval, { verb: 'approve' }), { method: 'work.answer', params: { question_id: 'act_c1', option: 'approve' } })
  assert.deepEqual(answerOf(approval, { verb: 'decline', note: ' not the north wall ' }), { method: 'work.answer', params: { question_id: 'act_c1', option: 'decline', note: 'not the north wall' } })
  assert.equal(answerOf(approval, { verb: 'grant' }), null, 'it offers no grant')
  const offered = queueOf({ confirms: [confirm('act_g', { grant_offer: { scope: 'session', kind: 'edits', pattern: '/srv/harbour', words: 'edits in the project' } })], views: [] })[0]
  assert.deepEqual(answerOf(offered, { verb: 'grant' })?.params, { question_id: 'act_g', option: 'approve_grant' })
  assert.equal(grantKey(offered.confirm!), 'e', 'edits take e, as the TUI reads it')
  assert.equal(grantKey({ grant_offer: { scope: 'session', kind: 'prefix', pattern: 'cargo test', words: '' } }), 'a')
  assert.equal(answerOf(by.get('budget')!, { verb: 'approve' })?.params.option, 'reset')
  assert.equal(answerOf(q.find((i) => i.key === 'q:act_ch')!, { verb: 'approve' })?.params.option, 'accept')
  assert.deepEqual(answerOf(by.get('proposal')!, { verb: 'approve' })?.params, { question_id: 'act_px', option: 'accept' }, 'a proposal by its card')
  const question = by.get('question')!
  assert.deepEqual(answerOf(question, { verb: 'choice', n: 2 }), { method: 'work.answer', params: { question_id: 'act_q1', option: 'choice:1', note: undefined } })
  assert.deepEqual(answerOf(question, { verb: 'choice', n: 0, note: 'the east one' })?.params, { question_id: 'act_q1', option: 'choice:other', note: 'the east one' })
  assert.equal(answerOf(question, { verb: 'approve' }), null, 'a question with choices is answered by a choice')
  const failure = by.get('failure')!
  assert.deepEqual(answerOf(failure, { verb: 'retry' })?.method, 'task.retry')
  assert.deepEqual(answerOf(failure, { verb: 'retry' })?.params, { execution_id: 'exe_t5' })
  assert.deepEqual(answerOf(failure, { verb: 'dismiss' }), { method: 'execution.dismiss', params: { execution_id: 'exe_t5' } })
  const convFailed = queueOf({ confirms: [], views: [view('exe_c', 'conversation', 'failed', {}, 'needs_you')] })[0]
  assert.equal(answerOf(convFailed, { verb: 'retry' }), null, 'only a task is retried')
  assert.deepEqual(answerOf(by.get('seal')!, { verb: 'seal' })?.params, { slug: 'tide-tables', reason: 'it read a private chart' })
  assert.deepEqual(answerOf(by.get('seal')!, { verb: 'seal_dismiss' }), { method: 'thread.seal_dismiss', params: { slug: 'tide-tables' } })
  assert.deepEqual(answerOf(by.get('ask')!, { verb: 'thread_answer', key: '2' }), { method: 'thread.answer', params: { ask_id: 'ask_1', choice: '2' } })
})

test('the present is told, not guessed: past false with no views reads no executions', () => {
  const ex = [execution('exe_x', 'failed')]
  assert.deepEqual(queueOf({ confirms: [], views: [], executions: ex, past: false }), [])
  assert.equal(queueOf({ confirms: [], views: [], executions: ex, past: true }).length, 1)
})
