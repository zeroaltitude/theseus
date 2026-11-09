// A held question's card says how long it waited, never a deadline that has not passed (`src/lib/heldquestion.ts`,
// theseus-lrc3m), run by `npm test`. Pure: no DOM.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { deadlineOf, isHeld, waitedLabel } from '../src/lib/heldquestion.ts'

const MIN = 60_000
const HOUR = 60 * MIN
const ask = (o: Record<string, unknown> = {}) => ({ correlation_id: 'act_7f3a9c', session_id: 'ses_a', execution_id: 'exe_a', tool: 'fs.write', input: {}, reason: 'write', by: 'operator', requested_at_ms: 1_000_000, expires_at_ms: 1_000_000 + 15 * MIN, floor: false, ...o }) as any

test('a call\'s question has no deadline, past its first life or inside it', () => {
  const c = ask()
  assert.equal(isHeld(c), true)
  // Inside its first life the live push still says a time; the card does not call it a deadline.
  assert.equal(deadlineOf(c), 0)
  // Past it nothing has expired either: the question is held until it is answered.
  assert.equal(deadlineOf(ask({ expires_at_ms: 1 })), 0)
})

test('a held question says how long it has waited, not that it expired', () => {
  const c = ask()
  assert.equal(waitedLabel(c, c.requested_at_ms + 3 * HOUR), 'waiting 3 h')
  assert.equal(waitedLabel(c, c.requested_at_ms + 20 * MIN), 'waiting 20 min')
  assert.equal(waitedLabel(c, c.requested_at_ms + 30_000), 'waiting 30 s')
  assert.equal(waitedLabel(c, c.requested_at_ms - 5_000), 'waiting 0 s', 'a clock behind the daemon\'s never goes negative')
  assert.ok(!/expired/.test(waitedLabel(c, c.expires_at_ms + 2 * HOUR) ?? ''))
})

test('a question that expires keeps its deadline and says nothing of a wait', () => {
  for (const tool of ['owner_ask', 'extend.ack', 'pack.promote']) {
    const c = ask({ tool })
    assert.equal(isHeld(c), false, tool)
    assert.equal(deadlineOf(c), c.expires_at_ms, tool)
    assert.equal(waitedLabel(c, c.requested_at_ms + HOUR), null, tool)
  }
})

test('a budget question holds as it says: no deadline, and no wait label', () => {
  const c = ask({ tool: 'budget.reset', expires_at_ms: 0, budget: { spent_usd: 1, limit_usd: 1, needed_usd: 1, lifetime_usd: 1 } })
  assert.equal(isHeld(c), false)
  assert.equal(deadlineOf(c), 0)
  assert.equal(waitedLabel(c, c.requested_at_ms + HOUR), null)
})
