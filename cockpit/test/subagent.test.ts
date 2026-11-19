// A subagent's answer (`src/lib/subagent.ts`), run by `npm test`: the footer names the model that answered and says it is
// a subagent's, and only a result that holds words is one.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { SubagentAnswer } from '@protocol'
import { SUBAGENT_ICON, subagentFooter, subagentOf, subagentOpening } from '../src/lib/subagent.ts'

const answer = (over: Partial<SubagentAnswer> = {}): SubagentAnswer => ({
  profile: 'haiku', provider: 'anthropic', model: 'claude-haiku-5-5', asked_by: 'sonnet', text: 'Hello.',
  cost_usd: 0.0031, elapsed_ms: 1300, output_tokens: 12, cut: false, call: 'act_1', ...over,
})

test("the footer's model part is the profile and model that answered, then who asked, then the call's own cost and time", () => {
  assert.deepEqual(subagentFooter(answer()), ['haiku', 'claude-haiku-5-5', 'subagent answer, asked by sonnet', '$0.0031', '1.3 s'])
  assert.deepEqual(subagentFooter(answer({ asked_by: '', cost_usd: null, cut: true })), ['haiku', 'claude-haiku-5-5', 'subagent answer', 'cut at its cap', '1.3 s'])
})

test('only a result node that holds words is a subagent message', () => {
  assert.equal(subagentOf({ meta: { subagent: answer() } })?.model, 'claude-haiku-5-5')
  assert.equal(subagentOf({ meta: { subagent: answer({ text: '  ' }) } }), null)
  assert.equal(subagentOf({ meta: { exit_code: 0 } }), null)
  assert.equal(subagentOf(null), null)
  assert.equal(subagentOf(undefined), null)
})

test('the message opens with the icon before its first word', () => {
  assert.equal(SUBAGENT_ICON, '🛰️')
  assert.equal(subagentOpening(answer({ text: '  Hello from the other model.\n' })), '🛰️ Hello from the other model.')
})
