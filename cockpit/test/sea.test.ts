// The living sea (`src/ship/sea.ts`, theseus-hnof.2, the owner's C5), run by `npm test`: no work is no height,
// rising with tokens a minute and the turns running, settling as the work ends; in Live mode a slow roll under it all,
// never still (theseus-42ic); stilled, dead calm at last.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { SEA_FULL_TPM, SEA_ROLL, SEA_SETTLE_S, seaHeight, seaPace, seaStep, seaTarget, seaWord } from '../src/ship/sea.ts'

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
  assert.deepEqual([0, SEA_ROLL, 0.1, 0.4, 0.7, 0.95].map(seaWord), ['dead calm', 'a slow roll', 'a light swell', 'a moderate swell', 'a rough sea', 'a heavy sea'])
  assert.ok(seaPace(1) > seaPace(0.2) && seaPace(0) > 0)
})

test('in Live mode the sea never stops: a slow roll when nothing happens, and any work raises it (theseus-42ic)', () => {
  // Nothing happening: the roll, low; stilled (Calm, reduced motion, ?swell=0): dead calm.
  assert.equal(seaHeight(seaTarget(0, 0), true), SEA_ROLL)
  assert.equal(seaHeight(seaTarget(0, 0), false), 0)
  assert.ok(SEA_ROLL > 0 && SEA_ROLL < 0.1, `a slow, low roll: ${SEA_ROLL}`)
  assert.equal(seaWord(seaHeight(0, true)), 'a slow roll')
  // Any work raises it above the roll, and more work higher; a heavy sea is still a heavy sea.
  const light = seaHeight(seaTarget(300, 0), true)
  assert.ok(light > SEA_ROLL + 0.05, `a model call's few hundred tokens over the roll: ${light}`)
  assert.ok(seaHeight(seaTarget(15_000, 2), true) > light)
  assert.equal(seaHeight(1, true), 1)
  assert.equal(seaWord(light), 'a light swell')
  // The roll is already rolling: a page that opens (or Calm let go) starts at it, not easing up from dead calm.
  assert.equal(seaStep(0, SEA_ROLL, 1 / 60), SEA_ROLL)
  assert.equal(seaStep(SEA_ROLL, SEA_ROLL, 1 / 8), SEA_ROLL)
  // The work ends: the sea settles back to the roll, exactly, and stays there (the loop paces it as the roll).
  let h = 0.6
  let n = 0
  for (; h !== SEA_ROLL && n < 10_000; n++) h = seaStep(h, SEA_ROLL, 1 / 15)
  assert.equal(h, SEA_ROLL)
  assert.ok(n / 15 < SEA_SETTLE_S * 6, `back at the roll after ${(n / 15).toFixed(1)} s`)
  for (let i = 0; i < 100; i++) h = seaStep(h, SEA_ROLL, 1 / 8)
  assert.equal(h, SEA_ROLL)
  // Work that starts at the roll rises from it.
  assert.ok(seaStep(SEA_ROLL, light, 1 / 15) > SEA_ROLL)
})

test('the Ship rolls its sea in Live mode only, and the engine paces the roll as the roll', () => {
  // Node's runner cannot mount the view or the engine (three.js): this reads their source, as motion.test.ts reads the
  // shaders.
  const view = readFileSync(new URL('../src/views/Ship.tsx', import.meta.url), 'utf8')
  assert.match(view, /const sea = seaHeight\(seaTarget\(data\.tpm, turnsRunning\), swell && !calm\)/)
  assert.match(view, /engine\?\.setSea\(sea\)/)
  assert.match(view, /<SeaGauge height=\{sea\} word=\{seaWord\(sea\)\}/)
  const engine = readFileSync(new URL('../src/ship/engine.ts', import.meta.url), 'utf8')
  assert.match(engine, /sea: this\.rolling\(\) && !this\.atRoll\(\),\s+roll: this\.rolling\(\) && this\.atRoll\(\),/)
  assert.match(engine, /return this\.seaLevel <= SEA_ROLL && this\.seaWant <= SEA_ROLL/)
  assert.match(engine, /this\.seaLevel = this\.swell && !this\.calm \? seaStep\(/)
})
