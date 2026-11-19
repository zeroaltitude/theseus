// A question's buttons from its options (`src/lib/answers.ts`, theseus-ds5os), run by `npm test`.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { answerParams, buttonsOf, hasWholeText, isKeep, isSettingsChange, keepNeedsCode, keepParams, mayAnswer, needsCode, proofLine } from '../src/lib/answers.ts'

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

test('the second proof: the code rides an approving answer to a question that needs it, never a decline or another question (issue 22)', () => {
  const c = ask({ needs_proof: true, options: [opt('approve', 'Approve'), opt('decline', 'Decline', true)] })
  assert.deepEqual(answerParams(c, 'approve', '', ' 123456 '), { question_id: 'act_7f3a9c', option: 'approve', note: undefined, code: '123456' })
  assert.deepEqual(answerParams(c, 'approve', '', ''), { question_id: 'act_7f3a9c', option: 'approve', note: undefined })
  assert.equal('code' in answerParams(c, 'decline', 'no', '123456'), false)
  const plain = ask({ options: [opt('approve', 'Approve')] })
  assert.equal('code' in answerParams(plain, 'approve', '', '123456'), false)
})

test('the card says which footing the second proof is on, and shows the code field only when a code is needed (issue 22)', () => {
  assert.equal(proofLine(ask()), null)
  assert.equal(needsCode(ask()), false)
  const needed = ask({ needs_proof: true })
  assert.equal(proofLine(needed), 'second proof: needed, enter the code from your authenticator app')
  assert.equal(needsCode(needed), true)
  const given = ask({ needs_proof: true, proof: { code_needed: false, line: 'second proof: given here 3 hours ago, good for 21 hours more' } })
  assert.equal(proofLine(given), 'second proof: given here 3 hours ago, good for 21 hours more')
  assert.equal(needsCode(given), false)
  const here = ask({ needs_proof: true, proof: { code_needed: true, line: 'second proof: needed, enter the code from your authenticator app' } })
  assert.equal(needsCode(here), true)
})

test('a settings change keeps its lines, and has a whole text to read only when the daemon says so', () => {
  const plain = ask({ tool: 'config.ack', input: { question: 'Change 1 setting?' } })
  assert.equal(isSettingsChange(plain), true)
  assert.equal(hasWholeText(plain), false)
  assert.equal(hasWholeText(ask({ tool: 'config.ack', input: { whole: true } })), true)
  // Only a settings change: the field means nothing on another tool's question.
  assert.equal(isSettingsChange(ask({ input: { whole: true } })), false)
  assert.equal(hasWholeText(ask({ input: { whole: true } })), false)
  assert.equal(hasWholeText(ask({ tool: 'config.ack', input: null })), false)
})

test('a keep card takes a value in a field of its own, never a yes, and the value rides one request', () => {
  const card = (extra: object = {}) =>
    ask({ correlation_id: 'act_keep1', tool: 'config.ack', input: { keep: { vault: 'Harbor', title: 'Lantern new' } }, needs_proof: true, ...extra })
  assert.equal(isKeep(card()), true)
  assert.equal(isKeep(card({ input: { paths: [] } })), false)
  assert.equal(isKeep(card({ tool: 'proc.run' })), false)
  // A new item: a code only when no window of this place covers it.
  assert.equal(keepNeedsCode(card({ proof: { code_needed: false, line: '' } })), false)
  assert.equal(keepNeedsCode(card({ proof: { code_needed: true, line: '' } })), true)
  // A replace: a fresh code every time, window or not.
  assert.equal(keepNeedsCode(card({ needs_fresh_proof: true, proof: { code_needed: false, line: '' } })), true)
  assert.deepEqual(keepParams(card({ proof: { code_needed: true, line: '' } }), 'INVENTED-VALUE', ' 123456 '), { question_id: 'act_keep1', value: 'INVENTED-VALUE', code: '123456' })
  assert.deepEqual(keepParams(card({ proof: { code_needed: false, line: '' } }), 'INVENTED-VALUE', '123456'), { question_id: 'act_keep1', value: 'INVENTED-VALUE' })
})

test('a keep card offers a decline alone, and its field sends what the daemon takes', async () => {
  // The daemon offers no yes to a secret to keep: only its decline is a button, and the field
  // and "Keep it" carry the value.
  const card = (extra: object = {}) =>
    ask({ correlation_id: 'act_keep1', tool: 'config.ack', input: { keep: { vault: 'Harbor', title: 'Lantern new' } }, needs_proof: true, ...extra })
  const c = card({ options: [opt('decline', 'Decline', true)] })
  assert.equal(isKeep(c), true)
  assert.deepEqual(buttonsOf(c).map((b) => [b.id, b.tone]), [['decline', 'fault']])
  // What the field sends is the file the daemon's own test sends through the web port.
  const { readFileSync } = await import('node:fs')
  const fixture = JSON.parse(readFileSync(new URL('./fixtures/keep_answer.json', import.meta.url), 'utf8'))
  const sent = keepParams(card({ proof: { code_needed: true, line: '' } }), 'INVENTED-COCKPIT-VALUE-5531', '123456')
  assert.deepEqual(sent, fixture)
})
