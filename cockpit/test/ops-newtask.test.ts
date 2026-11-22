// The New task sheet (theseus-jid7t.6): every field of the old form and what `task.start` takes besides.
import test from 'node:test'
import assert from 'node:assert/strict'
import type { ProfileInfo } from '../src/protocol.gen/index.ts'
import { dollarsOf, EMPTY_FORM, profileChoices, quotesOf, startParams } from '../src/ops/newtask.ts'

test('the old form’s fields, trimmed, and only those given', () => {
  assert.equal(startParams(EMPTY_FORM), null, 'no brief')
  assert.deepEqual(startParams({ ...EMPTY_FORM, brief: ' map the quay ' }), { brief: 'map the quay' })
  assert.deepEqual(startParams({ ...EMPTY_FORM, brief: 'b', title: ' T ', budget: '$1.5', home: 'ses_h' }), { brief: 'b', title: 'T', home: 'ses_h', budget_usd: 1.5 })
})

// The open task is retired (theseus-9ymz0): the daemon refuses `open` in
// words, so the sheet no longer offers it and the form no longer carries it.
test('the new fields: profile, start time, quotes', () => {
  const p = startParams({ ...EMPTY_FORM, brief: 'b', profile: 'deep', startWhen: ' fri 9:00 ', quotes: 'first log\n\n  second one  \n\n' })!
  assert.deepEqual(p, { brief: 'b', profile: 'deep', start_when: 'fri 9:00', quotes: ['first log', 'second one'] })
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

test('profile choices: the home’s mark greys no configured profile', () => {
  const ps = [prof('quick', 'us'), prof('kimi', 'cn')]
  assert.deepEqual(profileChoices(ps, 'open').choices.map((c) => c.grey), [null, null])
  const sens = profileChoices(ps, 'sensitive').choices
  assert.equal(sens[0].grey, null)
  assert.equal(sens[1].grey, null)
})

// No rule reads a model's origin since 2026-11-21: a profile of China origin, as `profile.list` sends it, is offered
// like any other, whatever the home holds, and nothing is picked for the owner.
test('a profile of cn origin that the daemon clears is offered like any other', () => {
  const kimi: ProfileInfo = prof('kimi', 'cn')
  const ps = [prof('quick', 'us'), kimi]
  for (const mark of ['open', 'private', 'sensitive'] as const) {
    const { choices } = profileChoices(ps, mark)
    assert.deepEqual(choices.map((c) => [c.name, c.grey]), [['quick', null], ['kimi', null]], mark)
    assert.match(choices[1].label, /kimi · m-kimi · cn/)
  }
  assert.ok(!('pick' in profileChoices(ps, 'open')), 'nothing is picked: the parent’s model, or the owner’s choice')
})
