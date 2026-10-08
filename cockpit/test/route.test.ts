// How routing placed a turn, in words (`src/lib/route.ts`), run by `npm test`: the mode and where the turn ran, and
// route.v3's effort, applied, clamped, or recorded with why it stood aside (theseus-qe3v). Invented ids.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { effortWords, routedOf, routeLine } from '../src/lib/route.ts'

const row = (data: Record<string, unknown>) =>
  ({ position: 1, at_unix_ms: 1, kind: 'route.decided', session_id: 'ses_a', turn_id: 'turn_a', data }) as any

test("a routed turn's line says the mode, the move, and the effort Jev set", () => {
  const r = row({ mode: 'sophisticated', reason: 'verdict', from: 'sonnet', profile: 'opus', wait_ms: 40, late: false,
    effort: 'max', effort_reason: 'applied', effort_applied: true, effort_ran: 'max' })
  assert.equal(routeLine(r), 'sophisticated · sonnet → opus (verdict) · effort max (Jev) · waited 40 ms')
  assert.deepEqual(routedOf(r).effortApplied, 'max')
})

test('a clamped effort names the bound and the answer', () => {
  const t = routedOf(row({ mode: 'chat', reason: 'verdict', from: 'sonnet', profile: 'sonnet', effort: 'max',
    effort_reason: 'clamped', effort_applied: true, effort_ran: 'high' }))
  assert.equal(effortWords(t), 'effort high (Jev; clamped from max)')
})

test("an effort that did not apply says Jev's answer and why, and a row before route.v3 says none", () => {
  for (const [reason, words] of [['unsure', 'Jev: low, unsure'], ['no_effort', 'Jev: high, no effort'], ['fixed', 'Jev: low, fixed'],
    ['carried', 'Jev: max, carried'], ['recorded', 'Jev: max, recorded']] as const) {
    const level = words.split(' ')[1].replace(',', '')
    const t = routedOf(row({ mode: 'chat', reason: 'verdict', effort: level, effort_reason: reason, effort_applied: false, effort_ran: 'high' }))
    assert.equal(t.effortApplied, undefined, reason)
    assert.equal(effortWords(t), words, reason)
  }
  const old = row({ mode: 'trivial', reason: 'detour', from: 'sonnet', profile: 'haiku', detour: true, wait_ms: 12, late: true })
  assert.equal(routeLine(old), 'trivial · sonnet → haiku (detour) · this message alone · waited 12 ms, late')
  assert.equal(routeLine(row({ reason: 'no_verdict', from: 'sonnet', profile: 'sonnet' })), 'no verdict · sonnet (no_verdict)')
})
