// The New task sheet (theseus-jid7t.6): every field of the old form and what `task.start` takes besides.
import test from 'node:test'
import assert from 'node:assert/strict'
import type { ProfileInfo } from '../src/protocol.gen/index.ts'
import { dollarsOf, EMPTY_FORM, profileChoices, quotesOf, startParams, usClosed } from '../src/ops/newtask.ts'

test('the old form’s fields, trimmed, and only those given', () => {
  assert.equal(startParams(EMPTY_FORM), null, 'no brief')
  assert.deepEqual(startParams({ ...EMPTY_FORM, brief: ' map the quay ' }), { brief: 'map the quay' })
  assert.deepEqual(startParams({ ...EMPTY_FORM, brief: 'b', title: ' T ', budget: '$1.5', home: 'ses_h' }), { brief: 'b', title: 'T', home: 'ses_h', budget_usd: 1.5 })
})

test('the new fields: profile, start time, quotes, open', () => {
  const p = startParams({ ...EMPTY_FORM, brief: 'b', profile: 'deep', startWhen: ' fri 9:00 ', quotes: 'first log\n\n  second one  \n\n', open: true })!
  assert.deepEqual(p, { brief: 'b', profile: 'deep', start_when: 'fri 9:00', quotes: ['first log', 'second one'], open: true })
  assert.deepEqual(quotesOf('  '), [])
})

test('a bad budget blocks the start; blank is a quarter of what is left', () => {
  assert.deepEqual(dollarsOf(''), { bad: false })
  assert.deepEqual(dollarsOf('0'), { bad: true })
  assert.deepEqual(dollarsOf('-3'), { bad: true })
  assert.deepEqual(dollarsOf('abc'), { bad: true })
  assert.equal(startParams({ ...EMPTY_FORM, brief: 'b', budget: 'x' }), null)
})

const prof = (name: string, origin: string): ProfileInfo => ({ name, provider: 'p', model: `m-${name}`, max_output_tokens: 1, has_system: false, live: false, origin })

test('profile choices: the home’s mark greys other origins; a caller’s US-only reason does too', () => {
  const ps = [prof('quick', 'us'), prof('kimi', 'cn')]
  assert.deepEqual(profileChoices(ps, 'open').choices.map((c) => c.grey), [null, null])
  const sens = profileChoices(ps, 'sensitive').choices
  assert.equal(sens[0].grey, null)
  assert.match(sens[1].grey!, /not a model of US origin/)
  const cal = profileChoices(ps, 'open', 'a sensitive calendar event: US models only').choices
  assert.equal(cal[1].grey, 'a sensitive calendar event: US models only')
  assert.equal(cal[0].grey, null)
})

test('US only fails closed (ops-timeline x ops-act): no profile, or a greyed one, starts nothing; one of US origin does', () => {
  const ps = [prof('quick', 'us'), prof('kimi', 'cn')]
  const why = 'this event’s calendar is sensitive: US models only'
  const { choices, pick } = profileChoices(ps, 'open', why, 'kimi')
  assert.equal(pick, 'quick', 'the live one may not: the first of US origin is picked')
  assert.equal(usClosed(choices, '', why), true, 'the parent’s model may be of any origin')
  assert.equal(usClosed(choices, 'kimi', why), true)
  assert.equal(usClosed(choices, 'nowhere', why), true, 'a profile not listed')
  assert.equal(usClosed(choices, 'quick', why), false)
  assert.equal(usClosed(choices, '', undefined), false, 'without US only, the parent’s model starts')
  assert.equal(profileChoices([prof('kimi', 'cn')], 'open', why).pick, '', 'none of US origin: none picked')
})
