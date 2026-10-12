// The seagull (`src/ship/gull.ts`, its voice `gullsound.ts`, theseus-7mow), run by `npm test`: its visits come at random
// gaps, never close together and never on a beat; nothing plays, or is even scheduled, with sound off; a call falls,
// crosses the stereo field and stays under the waves; and every node is let go when it stops.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import {
  GULL_GAIN, GULL_GAP_MAX_S, GULL_GAP_MIN_S, gullCall, gullGap, gullLevel, gullRng, startGulls, type GullClock,
} from '../src/ship/gull.ts'
import { SURF_GAIN_ROLL } from '../src/ship/surf.ts'
import { ShipAudio } from '../src/ship/audio.ts'
import { FakeContext, FakeSource, installFakeAudio, made } from './fakeaudio.ts'

/** A clock that runs on demand: `after` queues, `fire` runs the earliest. */
function fakeClock() {
  const q: { id: number; at: number; fn: () => void }[] = []
  let id = 0
  let now = 0
  const clock: GullClock = {
    after: (fn, ms) => { const h = ++id; q.push({ id: h, at: now + ms, fn }); return h },
    cancel: (h) => { const i = q.findIndex((e) => e.id === h); if (i >= 0) q.splice(i, 1) },
  }
  const fire = () => {
    q.sort((a, b) => a.at - b.at)
    const e = q.shift()
    if (!e) return null
    now = e.at
    e.fn()
    return now / 1000
  }
  return { clock, fire, pending: () => q.length }
}

test('the gaps: random, between the bounds, never two close together and never on a beat', () => {
  const next = gullRng(7)
  const gaps = Array.from({ length: 200 }, () => gullGap(next))
  for (const g of gaps) assert.ok(g >= GULL_GAP_MIN_S && g <= GULL_GAP_MAX_S, `gap ${g}`)
  assert.ok(GULL_GAP_MIN_S >= 120, 'no two visits within two minutes')
  assert.ok(new Set(gaps.map((g) => Math.round(g))).size > 100, 'a spread of gaps, not a beat')
  assert.deepEqual(Array.from({ length: 5 }, ((n) => () => gullGap(n))(gullRng(7))), gaps.slice(0, 5), 'seeded: the same gaps again')
  assert.notDeepEqual(gaps.slice(0, 5), Array.from({ length: 5 }, ((n) => () => gullGap(n))(gullRng(8))), 'another seed, other gaps')
  assert.equal(gullGap(() => 0), GULL_GAP_MIN_S)
  assert.equal(gullGap(() => 1), GULL_GAP_MAX_S)
})

test('the scheduler: with sound on a visit every gap, the first a gap away; with sound off nothing is scheduled or played', () => {
  const off = fakeClock()
  let played = 0
  const stopOff = startGulls(false, gullRng(1), off.clock, () => { played++ })
  assert.equal(off.pending(), 0, 'nothing scheduled with sound off')
  assert.equal(off.fire(), null)
  assert.equal(played, 0)
  stopOff()

  const on = fakeClock()
  const times: number[] = []
  const stop = startGulls(true, gullRng(1), on.clock, () => { played++; times.push(on.fire() ?? 0) })
  assert.equal(played, 0, 'turning sound on is not a visit')
  assert.equal(on.pending(), 1)
  const first = on.fire()!
  assert.ok(first >= GULL_GAP_MIN_S, `the first visit is a gap away (${first} s)`)
  assert.equal(played, 1)
  on.fire()
  assert.equal(played, 2)
  stop()
  assert.equal(on.pending(), 0, 'stopped: the pending visit is cancelled')
  assert.equal(on.fire(), null)
  assert.equal(played, 2, 'and none plays after the stop')
  void times
})

test('a call: two or three falling notes, each below the last, crossing the stereo field, fading in and out, under the waves', () => {
  for (let seed = 1; seed <= 50; seed++) {
    const c = gullCall(gullRng(seed))
    assert.ok(c.notes.length === 2 || c.notes.length === 3, `${c.notes.length} notes`)
    let prev = Infinity
    for (const n of c.notes) {
      assert.ok(n.to < n.from, 'every note falls')
      assert.ok(n.from < prev, 'each note starts below where the last began')
      assert.ok(n.at + n.dur < c.secs - c.fade + 0.01, 'the cries end before the fade-out')
      prev = n.from
    }
    assert.ok(Math.abs(c.panFrom) > 0.5 && Math.sign(c.panFrom) === -Math.sign(c.panTo), 'it crosses from one side to the other')
    assert.equal(gullLevel(c, 0), 0)
    assert.equal(gullLevel(c, c.secs), 0)
    assert.ok(gullLevel(c, c.fade / 2) < gullLevel(c, c.fade), 'it fades in')
    assert.ok(gullLevel(c, c.secs - c.fade / 2) < GULL_GAIN, 'and out')
  }
  assert.ok(GULL_GAIN < SURF_GAIN_ROLL / 2, 'quieter than the waves')
})

test('the graph: a call makes sources and a panner on the master, and stopping lets go of every one', () => {
  FakeContext.held = false
  FakeContext.slow = null
  installFakeAudio()
  made.length = 0
  const a = new ShipAudio()
  return a.start().then(() => {
    const ctx = made[0]
    const call = gullCall(gullRng(3))
    assert.equal(a.gull(call), true)
    assert.equal(a.gullsHeard, 1)
    const srcs = ctx.nodes.filter((n): n is FakeSource => n instanceof FakeSource && n.kind === 'oscillator')
    assert.equal(srcs.length, call.notes.length * 2, 'a voice and a wobble to a note')
    assert.ok(srcs.every((s) => s.started && (s.stopped === null || s.stopped > ctx.currentTime)), 'all sounding')
    const pan = ctx.nodes.find((n) => n.kind === 'panner')!
    assert.ok(pan, 'a panner')
    assert.ok(pan.outs.length > 0, 'into the master')
    a.gullStop()
    assert.equal(a.gullsHeard, 0)
    assert.ok(srcs.every((s) => s.stopped !== null && s.stopped <= ctx.currentTime), 'every source stopped')
    assert.ok(pan.outs.length > 0 && ctx.nodes.filter((n) => n.kind === 'gain' && n.disconnected).length > 0, 'the gull’s gain let go')
    // And a page leaving (dispose) ends a gull in the air.
    assert.equal(a.gull(gullCall(gullRng(4))), true)
    a.dispose()
    assert.equal(a.gullsHeard, 0)
  })
})

test('a gull plays only where the sound runs: no context, nothing', () => {
  const a = new ShipAudio()
  assert.equal(a.gull(gullCall(gullRng(1))), false)
  assert.equal(a.gullsHeard, 0)
})

test('the page: the gull is heard where the sea is heard, and its timers and nodes are let go on cleanup', () => {
  const src = readFileSync(new URL('../src/ship/useShipSound.ts', import.meta.url), 'utf8')
  assert.match(src, /const hearing = on && !calm && swell/)
  assert.match(src, /if \(!hearing\) return\s+const a = theAudio\(\)[\s\S]*startGulls\(true[\s\S]*return \(\) => \{ stop\(\); a\.gullStop\(\) \}\s+\}, \[hearing\]\)/)
})
