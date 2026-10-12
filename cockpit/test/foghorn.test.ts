// The foghorn (`src/ship/foghorn.ts`, its voice `foghornsound.ts`, theseus-7mow), run by `npm test`: its horns come at
// random gaps, rarer than the gull's, never two within 8 minutes, never on a beat and never near a gull; nothing plays,
// or is even scheduled, with sound off; a horn is a long low tone dropping to a lower grunt, louder than the gull and
// under the waves, and nothing like the failure cue; and every node is let go when it stops.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import {
  HORN_CLEAR_S, HORN_GAIN, HORN_GAP_MAX_S, HORN_GAP_MIN_S, hornCall, hornClearMs, hornGap, hornLevel, startFoghorns,
} from '../src/ship/foghorn.ts'
import { GULL_GAIN, GULL_GAP_MAX_S, gullCall, gullRng, type GullClock } from '../src/ship/gull.ts'
import { SURF_GAIN_ROLL } from '../src/ship/surf.ts'
import { ShipAudio } from '../src/ship/audio.ts'
import { FakeContext, FakeSource, installFakeAudio, made, type FakeNode, type FakeParam } from './fakeaudio.ts'

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
  return { clock, fire, pending: () => q.length, now: () => now / 1000 }
}

test('the gaps: random, between 8 and 20 minutes, rarer than the gull, never on a beat', () => {
  const next = gullRng(7)
  const gaps = Array.from({ length: 200 }, () => hornGap(next))
  for (const g of gaps) assert.ok(g >= HORN_GAP_MIN_S && g <= HORN_GAP_MAX_S, `gap ${g}`)
  assert.equal(HORN_GAP_MIN_S, 8 * 60)
  assert.equal(HORN_GAP_MAX_S, 20 * 60)
  assert.ok(HORN_GAP_MIN_S > GULL_GAP_MAX_S, 'its shortest gap is longer than the gull’s longest')
  assert.ok(new Set(gaps.map((g) => Math.round(g))).size > 150, 'a spread of gaps, not a beat')
  assert.deepEqual(Array.from({ length: 5 }, ((n) => () => hornGap(n))(gullRng(7))), gaps.slice(0, 5), 'seeded: the same gaps again')
  assert.notDeepEqual(gaps.slice(0, 5), Array.from({ length: 5 }, ((n) => () => hornGap(n))(gullRng(8))), 'another seed, other gaps')
  assert.equal(hornGap(() => 0), HORN_GAP_MIN_S)
  assert.equal(hornGap(() => 1), HORN_GAP_MAX_S)
})

test('the scheduler: with sound on a horn every gap, the first a gap away; with sound off nothing is scheduled or played', () => {
  const off = fakeClock()
  let played = 0
  const stopOff = startFoghorns(false, gullRng(1), off.clock, () => { played++ }, { first: 5000 })
  assert.equal(off.pending(), 0, 'nothing scheduled with sound off, not even ?horn=now')
  assert.equal(off.fire(), null)
  assert.equal(played, 0)
  stopOff()

  const on = fakeClock()
  const stop = startFoghorns(true, gullRng(1), on.clock, () => { played++ })
  assert.equal(played, 0, 'turning sound on is not a horn')
  assert.equal(on.pending(), 1)
  const first = on.fire()!
  assert.ok(first >= HORN_GAP_MIN_S, `the first horn is a gap away (${first} s)`)
  assert.equal(played, 1)
  on.fire()
  assert.equal(played, 2)
  stop()
  assert.equal(on.pending(), 0, 'stopped: the pending horn is cancelled')
  assert.equal(on.fire(), null)
  assert.equal(played, 2, 'and none plays after the stop')
})

test('the spacing: never two horns within 8 minutes, never on a beat, never within 30 s of a gull, over many seeds', () => {
  for (let seed = 1; seed <= 20; seed++) {
    const c = fakeClock()
    const times: number[] = []
    // Gulls come every 2 to 5 minutes on their own; the horn asks how long until it is clear of the last.
    const gulls = gullRng(seed + 1000)
    const gullTimes: number[] = []
    let gullNext = 120 + 180 * gulls()
    const clear = () => {
      while (gullNext <= c.now()) { gullTimes.push(gullNext); gullNext += 120 + 180 * gulls() }
      return hornClearMs(c.now(), gullTimes.at(-1) ?? -Infinity)
    }
    // `?horn=now` on half of them: the horn five seconds in, then the ordinary gaps.
    const stop = startFoghorns(true, gullRng(seed), c.clock, () => { times.push(c.now()) }, { first: seed % 2 ? 5000 : undefined, clear })
    while (times.length < 40) c.fire()
    stop()
    if (seed % 2) assert.equal(times[0], 5, '?horn=now: the first horn five seconds in')
    const gaps = times.slice(1).map((t, i) => t - times[i])
    for (const g of gaps) assert.ok(g >= HORN_GAP_MIN_S, `two horns ${g} s apart`)
    assert.ok(new Set(gaps.map((g) => Math.round(g))).size > 30, 'no beat')
    // Every horn is at least 30 s after the gull before it (the gull's side, ShipAudio.gull, is held below).
    for (const t of times) {
      const before = gullTimes.filter((x) => x <= t).pop()
      if (before !== undefined) assert.ok(t - before >= HORN_CLEAR_S - 1e-9, `a horn ${t - before} s after a gull`)
    }
  }
  assert.equal(hornClearMs(100, 90), 20_000)
  assert.equal(hornClearMs(100, 70), 0)
  assert.equal(hornClearMs(100, -Infinity), 0)
})

test('a horn: a long low tone, then a lower grunt, 4 to 5 s in all, swelling in and dying away; louder than the gull, under the waves', () => {
  for (let seed = 1; seed <= 50; seed++) {
    const c = hornCall(gullRng(seed))
    const [long, grunt] = c.tones
    assert.ok(long.hz >= 80 && long.hz <= 100, `the long tone at ${long.hz} Hz`)
    assert.ok(grunt.hz >= 60 && grunt.hz <= 70, `the grunt at ${grunt.hz} Hz`)
    assert.ok(grunt.hz < long.hz, 'the second tone is lower')
    assert.equal(long.at, 0)
    assert.ok(long.dur >= 2.3 && long.dur <= 2.8, `the long tone lasts ${long.dur} s`)
    assert.equal(grunt.at, long.at + long.dur, 'the grunt follows the long tone')
    assert.ok(Math.abs(grunt.dur - 0.8) < 0.05, `the grunt holds ${grunt.dur} s`)
    assert.ok(c.secs >= 4 && c.secs <= 5, `${c.secs} s in all`)
    assert.ok(grunt.at + grunt.dur <= c.secs - c.decay + 1e-9, 'the grunt holds before it dies away')
    assert.equal(hornLevel(c, 0), 0)
    assert.equal(hornLevel(c, c.secs), 0)
    assert.ok(c.swell >= 0.6, 'a slow swell in')
    assert.ok(hornLevel(c, c.swell / 2) < hornLevel(c, c.swell), 'it swells in')
    assert.equal(hornLevel(c, c.swell + 0.5), HORN_GAIN, 'held at its peak')
    assert.ok(hornLevel(c, c.secs - c.decay / 2) < HORN_GAIN, 'and dies away')
    assert.ok(c.echo.feedback > 0 && c.echo.feedback < 0.7 && c.echo.tail >= 1.5, 'a long echo, dying')
    assert.ok(c.echo.hz < 1000 && c.lowpass < 500, 'dark: far off')
  }
  // The owner, 2026-10-12: "a little louder than the seagulls", and still under the waves.
  assert.ok(HORN_GAIN > GULL_GAIN, 'louder than the gull')
  assert.ok(HORN_GAIN < SURF_GAIN_ROLL, 'quieter than the waves')
  assert.ok(Math.abs(HORN_GAIN - 0.12) < 1e-9)
})

test('not the failure cue: longer, lower, quieter and slower than the `horn` cue', () => {
  const src = readFileSync(new URL('../src/ship/audio.ts', import.meta.url), 'utf8')
  // The cue's numbers, read where they live: its notes (G2 98 Hz, D3), its peak, and its length (1.9 s).
  const cue = src.slice(src.indexOf('private horn('))
  assert.match(cue, /\[\[98, 0\.5\], \[98\.7, 0\.38\], \[146\.8, 0\.16\]\]/)
  assert.match(cue, /exponentialRampToValueAtTime\(0\.3, t \+ 0\.18\)/)
  assert.match(src, /cue === 'horn' \? 1\.9/)
  for (let seed = 1; seed <= 20; seed++) {
    const c = hornCall(gullRng(seed))
    assert.ok(c.secs > 2 * 1.9, 'more than twice as long')
    assert.ok(c.tones[0].hz < 98 && c.tones[1].hz < 98 * 0.75, 'below the cue’s lowest note')
    assert.ok(HORN_GAIN < 0.3 / 2, 'under half the cue’s peak')
    assert.ok(c.swell > 0.18 * 4, 'swells in, where the cue strikes')
    assert.ok(c.waver < 1, 'a slow waver, not the cue’s 4.6 Hz vibrato')
  }
})

/** The peak an envelope's automation reaches (its events: a gain's resting value is overridden by its first). */
const peak = (p: FakeParam) => Math.max(...p.events.map((e) => e.v))

test('the graph: a horn makes its voices, a panner and an echo on the master, and stopping lets go of every one', () => {
  FakeContext.held = false
  FakeContext.slow = null
  installFakeAudio()
  made.length = 0
  const a = new ShipAudio()
  return a.start().then(() => {
    const ctx = made[0]
    const before = ctx.nodes.length
    const call = hornCall(gullRng(3))
    assert.equal(a.foghorn(call), true)
    assert.equal(a.hornsHeard, 1)
    const mine = ctx.nodes.slice(before)
    const oscs = mine.filter((n): n is FakeSource => n instanceof FakeSource && n.kind === 'oscillator')
    const voices = oscs.filter((o) => o.type === 'sawtooth' || o.type === 'square')
    assert.ok(voices.length >= 3, `several voices (${voices.length})`)
    assert.ok(voices.some((o) => o.type === 'sawtooth') && voices.some((o) => o.type === 'square'), 'saw and square')
    const start = voices.map((o) => o.frequency.valueAt(0.5))
    assert.ok(new Set(start.map((f) => f.toFixed(3))).size === voices.length, 'each a little detuned')
    for (const f of start) assert.ok(Math.abs(f - call.tones[0].hz) < 1, `on the long tone (${f})`)
    const t0 = 0.05
    for (const o of voices) {
      const f = o.frequency.valueAt(t0 + call.tones[1].at + 0.4)
      assert.ok(Math.abs(f - call.tones[1].hz) < 1, `dropped to the grunt (${f})`)
      assert.ok(o.stopped !== null && Math.abs(o.stopped - (t0 + call.secs + 0.05)) < 1e-6, 'the voices end with the horn')
    }
    const out = mine.find((n) => n.kind === 'gain' && peak((n as FakeNode & { gain: FakeParam }).gain) === HORN_GAIN) as FakeNode & { gain: FakeParam }
    assert.ok(out, 'an envelope peaking at HORN_GAIN')
    assert.ok(out.gain.valueAt(t0 + 0.1) < out.gain.valueAt(t0 + call.swell), 'swelling in')
    assert.ok(Math.abs(out.gain.valueAt(t0 + call.swell + 0.5) - HORN_GAIN) < 1e-9, 'held')
    assert.ok(out.gain.valueAt(t0 + call.secs - 0.2) < HORN_GAIN / 2, 'dying away')
    const filters = mine.filter((n) => n.kind === 'biquad') as (FakeNode & { type: string })[]
    assert.ok(filters.some((f) => f.type === 'lowpass') && filters.some((f) => f.type === 'bandpass'), 'a low-pass and a formant')
    const delay = mine.find((n) => n.kind === 'delay')!
    assert.ok(delay, 'an echo')
    // The echo feeds back into itself through a low-pass.
    const dark = delay.outs[0].to as FakeNode & { type: string }
    assert.equal(dark.type, 'lowpass')
    const back = dark.outs.find((o) => (o.to as FakeNode).kind === 'gain' && (o.to as FakeNode).outs.some((x) => x.to === delay))
    assert.ok(back, 'fed back round through the low-pass')
    // The waver runs on until the echo has rung out: its end is the horn's.
    const keeper = oscs.find((o) => o.type === 'sine')!
    assert.ok(keeper.onended, 'the horn ends when the waver does')
    assert.ok(keeper.stopped! >= t0 + call.secs + call.echo.tail - 1e-6, 'the waver outlives the echo')
    const pan = mine.find((n) => n.kind === 'panner')!
    assert.ok(pan.outs.length > 0, 'into the master')
    a.hornStop()
    assert.equal(a.hornsHeard, 0)
    assert.ok(oscs.every((s) => s.stopped !== null && s.stopped <= ctx.currentTime), 'every source stopped')
    assert.ok(out.disconnected && delay.disconnected, 'the horn and its echo let go')
    assert.ok(mine.filter((n) => n.kind === 'gain' && n.disconnected).length >= 2, 'the dry and the wet both let go')
    // The horn ending by itself lets go too, when its echo has rung out.
    assert.equal(a.foghorn(hornCall(gullRng(5))), true)
    const last = ctx.nodes.filter((n): n is FakeSource => n instanceof FakeSource && n.kind === 'oscillator' && !!n.onended).pop()!
    const ends = ctx.nodes.filter((n) => n.kind === 'delay').pop()!
    last.onended!()
    assert.equal(a.hornsHeard, 0)
    assert.ok(ends.disconnected, 'its echo let go on its own end')
    // And a page leaving (dispose) ends a horn in the air.
    assert.equal(a.foghorn(hornCall(gullRng(4))), true)
    a.dispose()
    assert.equal(a.hornsHeard, 0)
  })
})

test('a horn plays only where the sound runs: no context, or a held one, nothing', () => {
  const a = new ShipAudio()
  assert.equal(a.foghorn(hornCall(gullRng(1))), false)
  assert.equal(a.hornsHeard, 0)
  assert.equal(a.hornClear(), 0)
  FakeContext.held = true
  installFakeAudio()
  const b = new ShipAudio()
  return b.start().then((runs) => {
    assert.equal(runs, false)
    assert.equal(b.foghorn(hornCall(gullRng(1))), false, 'the browser holds it: no horn')
    assert.equal(b.hornsHeard, 0)
    FakeContext.held = false
  })
})

test('a horn and a gull keep 30 s apart, on the audio side too', () => {
  FakeContext.held = false
  FakeContext.slow = null
  installFakeAudio()
  made.length = 0
  const a = new ShipAudio()
  return a.start().then(() => {
    const ctx = made[0]
    assert.equal(a.hornClear(), 0, 'no gull yet: clear')
    assert.equal(a.gull(gullCall(gullRng(1))), true)
    assert.equal(a.hornClear(), HORN_CLEAR_S * 1000, 'a gull just now: wait 30 s')
    ctx.currentTime = 20
    assert.equal(a.hornClear(), 10_000)
    ctx.currentTime = 31
    assert.equal(a.hornClear(), 0)
    assert.equal(a.foghorn(hornCall(gullRng(1))), true)
    ctx.currentTime = 50
    assert.equal(a.gull(gullCall(gullRng(2))), false, 'a gull 19 s after a horn is let pass')
    ctx.currentTime = 62
    assert.equal(a.gull(gullCall(gullRng(2))), true, 'and one 31 s after is heard')
    a.dispose()
  })
})

test('the page: the foghorn is heard where the gull is heard, and its timers, ?horn=now’s too, and nodes are let go on cleanup', () => {
  const src = readFileSync(new URL('../src/ship/useShipSound.ts', import.meta.url), 'utf8')
  assert.match(src, /const hearing = on && !calm && swell/)
  const fx = src.slice(src.indexOf('// The foghorn'))
  assert.match(fx, /^[^\n]*\n\s+useEffect\(\(\) => \{\s+if \(!hearing\) return\s+const a = theAudio\(\)/)
  assert.match(fx, /startFoghorns\(true, next, \{ after: \(f, ms\) => setTimeout\(f, ms\), cancel: \(h\) => clearTimeout\(h as number\) \}/)
  assert.match(fx, /\{ first, clear: \(\) => a\.hornClear\(\) \}\)\s+return \(\) => \{ stop\(\); a\.hornStop\(\) \}\s+\}, \[hearing\]\)/)
  // `?horn=now`: the first horn five seconds in, on the scheduler's own timer, so the stop cancels it.
  assert.match(fx, /get\('horn'\) === 'now' \? 5000 : undefined/)
  const c = fakeClock()
  let played = 0
  const stop = startFoghorns(true, gullRng(1), c.clock, () => { played++ }, { first: 5000 })
  assert.equal(c.pending(), 1)
  stop()
  assert.equal(c.pending(), 0, 'cleanup before the five seconds: the ?horn=now timer is cancelled')
  assert.equal(c.fire(), null)
  assert.equal(played, 0)
})
