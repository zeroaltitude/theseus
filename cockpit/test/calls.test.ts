// The billed calls the money river reads (`src/lib/calls.ts`), run by `npm test`: a keep-warm read's row is one
// (theseus-ezeg), booked to its session at its own cost and usage.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { providerCalls } from '../src/lib/calls.ts'

const row = (kind: string, data: Record<string, unknown>) =>
  ({ kind, at_unix_ms: 1, position: 7, session_id: 'ses_a', turn_id: null, data }) as any

test("a keep-warm read joins the river's calls, with its cost and its cache read", () => {
  const usage = { input_tokens: 3, output_tokens: 1, cache_read_input_tokens: 626_000, cache_creation_input_tokens: 40 }
  const calls = providerCalls([
    row('keep_warm', { provider: 'anthropic', model: 'claude-fable-5-1', usage, cost_usd: 0.157, profile: 'fable' }),
    row('turn.started', {}),
    row('provider.call', { provider: 'anthropic', model: 'claude-fable-5-1', usage, cost_usd: 0.84, stop_reason: 'end_turn' }),
  ])
  assert.equal(calls.length, 2)
  assert.deepEqual([calls[0].cost, calls[0].stop, calls[0].session_id, calls[0].usage.cache_read_input_tokens], [0.157, 'keep_warm', 'ses_a', 626_000])
  assert.equal(calls[1].stop, 'end_turn')
})
