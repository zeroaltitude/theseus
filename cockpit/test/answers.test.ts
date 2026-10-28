// A question's buttons from its options (`src/lib/answers.ts`, theseus-ds5os), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { answerParams, buttonsOf, mayAnswer } from '../src/lib/answers.ts'

const opt = (id: string, label: string, takes_note = false, style = 'plain') => ({ id, label, style, takes_note, effect: `${id} does it` })
const ask = (o: Record<string, unknown> = {}) => ({ correlation_id: 'act_7f3a9c', session_id: 'ses_a', execution_id: 'exe_a', tool: 'fs.write', input: {}, reason: 'write', by: 'operator', requested_at_ms: 0, expires_at_ms: 0, floor: false, ...o }) as any

test('the buttons are the question\'s options, in order, with their tones', () => {
  const c = ask({ options: [opt('approve', 'Approve', false, 'primary'), opt('decline', 'Decline', true)] })
  assert.deepEqual(buttonsOf(c).map((b) => [b.id, b.tone, b.takesNote]), [['approve', 'ok', false], ['decline', 'fault', true]])
})

test('a floor question\'s approve is drawn as a danger', () => {
  const c = ask({ floor: true, options: [opt('approve', 'Approve', false, 'danger'), opt('decline', 'Decline', true)] })
  assert.equal(buttonsOf(c)[0].tone, 'fault')
})

test('no button is drawn for a connection that may not answer', () => {
  const c = ask({ may_answer: false, options: [opt('approve', 'Approve'), opt('decline', 'Decline')] })
  assert.equal(mayAnswer(c), false)
  assert.deepEqual(buttonsOf(c), [])
  // A question heard on the push says nothing, and is drawn.
  assert.equal(buttonsOf(ask({ options: [opt('approve', 'Approve')] })).length, 1)
  assert.equal(buttonsOf(ask({ may_answer: true, options: [opt('approve', 'Approve')] })).length, 1)
})

test('a daemon that sends no options means the answers its kind always had', () => {
  assert.deepEqual(buttonsOf(ask()).map((b) => b.id), ['approve', 'decline'])
  assert.deepEqual(buttonsOf(ask({ budget: { spent_usd: 1, limit_usd: 1, needed_usd: 1, lifetime_usd: 1 } })).map((b) => b.id), ['reset', 'decline'])
  assert.deepEqual(buttonsOf(ask({ tool: 'extend.ack' })).map((b) => b.id), ['ack', 'decline'])
})

test('work.answer params carry a note only where the option takes one', () => {
  const c = ask({ options: [opt('approve', 'Approve'), opt('decline', 'Decline', true)] })
  assert.deepEqual(answerParams(c, 'decline', ' not now '), { question_id: 'act_7f3a9c', option: 'decline', note: 'not now' })
  assert.deepEqual(answerParams(c, 'approve', 'ignored'), { question_id: 'act_7f3a9c', option: 'approve', note: undefined })
})

test('a question\'s grant is a button of its own, after the plain yes, sent as approve_grant (theseus-drjs)', () => {
  const grant = { ...opt('approve_grant', 'Approve + this command for the conversation', false, 'primary') }
  const c = ask({ grant_offer: { scope: 'session', kind: 'prefix', pattern: 'python3 -m unittest', words: 'yes' }, options: [opt('approve', 'Approve', false, 'primary'), grant, opt('decline', 'Decline', true)] })
  assert.deepEqual(buttonsOf(c).map((b) => [b.id, b.label, b.tone]), [['approve', 'Approve', 'ok'], ['approve_grant', 'Approve + this command for the conversation', 'ok'], ['decline', 'Decline', 'fault']])
  assert.deepEqual(answerParams(c, 'approve_grant', 'a note'), { question_id: 'act_7f3a9c', option: 'approve_grant', note: undefined })
})
