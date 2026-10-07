// The living sea (`src/ship/sea.ts`, theseus-hnof.2, the owner's C5), run by `npm test`: dead calm when nothing
// happens, rising with tokens a minute and the turns running, settling as the work ends, and still at last.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { SEA_FULL_TPM, SEA_SETTLE_S, seaPace, seaStep, seaTarget, seaWord } from '../src/ship/sea.ts'

test('no work is dead calm, exactly', () => {
  assert.equal(seaTarget(0, 0), 0)
  assert.equal(seaTarget(null, 0), 0)
  assert.equal(seaTarget(undefined, 0), 0)
  // A trickle (a ledger row's worth) is not work enough to move the sea.
  assert.equal(seaTarget(1, 0), 0)
  assert.equal(seaWord(seaTarget(0, 0)), 'dead calm')
})

test('the swell rises with the work: tokens a minute, the turns running, and both', () => {
  const light = seaTarget(300, 0)
  const busy = seaTarget(15_000, 0)
  assert.ok(light > 0 && light < 0.25, `a model call's few hundred tokens: ${light}`)
  assert.ok(busy > 0.55 && busy < 1, `fifteen thousand tokens a minute: ${busy}`)
  assert.equal(seaTarget(SEA_FULL_TPM, 0), 1)
  assert.equal(seaTarget(10 * SEA_FULL_TPM, 4), 1)
  const oneTurn = seaTarget(0, 1)
  assert.ok(oneTurn > 0.2 && oneTurn < 0.4, `a turn running and no tokens yet: ${oneTurn}`)
  assert.ok(seaTarget(300, 1) > Math.max(light, oneTurn), 'both raise it more than either')
  assert.ok(seaTarget(0, 4) > seaTarget(0, 1))
  for (let t = 0; t <= 100_000; t += 500) assert.ok(seaTarget(t + 500, 1) >= seaTarget(t, 1), 'more tokens never lower it')
})

test('it comes up in seconds, settles slower, and reaches dead calm, exactly 0, so the loop stops', () => {
  let h = 0
  for (let s = 0; s < 3; s += 1 / 15) h = seaStep(h, 0.6, 1 / 15)
  assert.ok(h > 0.4 && h < 0.6, `three seconds after the work starts: ${h}`)
  for (let s = 0; s < 15; s += 1 / 15) h = seaStep(h, 0.6, 1 / 15)
  assert.ok(Math.abs(h - 0.6) < 0.01)
  // The work ends: three seconds on it is still well up (it settles), and within about half a minute it is still.
  let after3 = h
  for (let s = 0; s < 3; s += 1 / 15) after3 = seaStep(after3, 0, 1 / 15)
  assert.ok(after3 > 0.3, `three seconds after the work ends: ${after3}`)
  let n = 0
  for (h = 0.6; h > 0 && n < 10_000; n++) h = seaStep(h, 0, 1 / 15)
  assert.equal(h, 0)
  assert.ok(n / 15 < SEA_SETTLE_S * 5, `dead calm after ${(n / 15).toFixed(1)} s`)
  // A long pause between frames (a hidden tab) lands where the easing would have.
  assert.ok(Math.abs(seaStep(0.6, 0, 60)) < 1e-9)
})

test('its state in words, and its pace', () => {
  assert.deepEqual([0, 0.1, 0.4, 0.7, 0.95].map(seaWord), ['dead calm', 'a light swell', 'a moderate swell', 'a rough sea', 'a heavy sea'])
  assert.ok(seaPace(1) > seaPace(0.2) && seaPace(0) > 0)
})
