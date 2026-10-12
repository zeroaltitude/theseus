// The stop nudge in words (`src/lib/nudge.ts`), run by `npm test`: the same cases as theseus-protocol's
// `a_nudged_turn_says_so_after_its_fallback`, so every surface says one thing (theseus-fh0u).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import type { TurnNudge } from '@protocol'
import { nudgeLine } from '../src/lib/nudge.ts'

test("a nudge's line says whether the model went on", () => {
  const n: TurnNudge = { judgment: 'jdg_1', by: 'announced_unfinished', p: 0.97, loop: 0, acted: true }
  assert.equal(nudgeLine(n), 'Nudged once: the reply said more work would follow; the model went on.')
  n.acted = false
  assert.equal(nudgeLine(n), 'Nudged once: the reply said more work would follow; the model answered without acting.')
})
