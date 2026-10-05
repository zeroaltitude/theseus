// A refusal's fallback in words (`src/lib/fallback.ts`), run by `npm test`: the same cases as theseus-protocol's
// `a_fallbacks_line_says_who_declined_and_who_answered`, so every surface says one thing.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { TurnFallback } from '@protocol'
import { fallbackLine, modelName } from '../src/lib/fallback.ts'

test("a fallback's line says who declined and who answered", () => {
  const f: TurnFallback = { from: 'claude-sonnet-5-5', to: 'claude-sonnet-5', category: 'cyber', answered: true }
  assert.equal(fallbackLine(f, 'end_turn'), 'Sonnet 5.5 declined (cyber); Sonnet 5 answered.')
  f.answered = false
  assert.equal(fallbackLine(f, 'refusal'), 'Sonnet 5.5 declined (cyber), and so did Sonnet 5.')
  delete f.category
  assert.equal(fallbackLine(f, 'budget'), 'Sonnet 5.5 declined; the request went to Sonnet 5.')
  for (const [id, name] of [
    ['claude-opus-5-5', 'Opus 5.5'],
    ['claude-haiku-4-5-20251001', 'Haiku 4.5'],
    ['claude-fable-5', 'Fable 5'],
    ['glm-5.3-flash', 'glm-5.3-flash'],
    ['claude-', 'claude-'],
  ]) {
    assert.equal(modelName(id), name, id)
  }
})
