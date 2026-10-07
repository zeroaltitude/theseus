// The ambient sea (`src/ship/surf.ts` and its graph in `src/ship/audio.ts`, theseus-pl0x), run by `npm test`: it waits
// for the gesture, comes in over a second or two and goes when sound is off, follows the sea's height (a quiet lapping
// at the idle roll, a little more with work, never loud), is silent where the sea is still, and ducks under a cue.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { SEA_RISE_S, SEA_ROLL, SEA_SETTLE_S, seaHeight, seaTarget, tpmOf } from '../src/ship/sea.ts'
import {
  DUCK, fadeTo, NOISE_S, pinkNoise, rampAt, SURF, SURF_FADE_IN_S, SURF_FADE_OUT_S, SURF_GAIN_FULL, SURF_GAIN_ROLL, surfHeight,
  surfVoice, surfWaves, WAVES_RATE, WAVES_S,
} from '../src/ship/surf.ts'
import { ShipAudio } from '../src/ship/audio.ts'
import { FakeContext, FakeNode, FakeParam, installFakeAudio, made } from './fakeaudio.ts'

const idle = { on: true, calm: false, swell: true, tpm: 0, turns: 0 }

test('silent unless sound is on and the sea rolls: Calm (reduced motion turns it on) and ?swell=0 still it', () => {
  assert.equal(surfHeight(idle), SEA_ROLL, 'an idle Live page: the roll')
  assert.equal(surfHeight({ ...idle, on: false }), 0, 'sound off')
  assert.equal(surfHeight({ ...idle, calm: true }), 0, 'Calm')
  assert.equal(surfHeight({ ...idle, calm: true, tpm: 50_000, turns: 3 }), 0, 'Calm, however hard the work')
  assert.equal(surfHeight({ ...idle, swell: false, tpm: 5_000 }), 0, '?swell=0')
  assert.deepEqual(surfVoice(0), { gain: 0, wash: 0, rate: 1 }, 'dead calm is silence')
  // The page takes Calm from the one toggle, which reduced motion turns on: the sea is silent wherever it is still.
  assert.match(readFileSync(new URL('../src/ship/useShipSound.ts', import.meta.url), 'utf8'), /const calm = useCalm\(\(s\) => s\.calm\)[\s\S]*surfHeight\(\{ on, calm, swell,/)
  assert.match(readFileSync(new URL('../src/lib/calm.ts', import.meta.url), 'utf8'), /prefers-reduced-motion: reduce/)
})

test('it follows the sea: the height is the Ship’s, a quiet lapping at the roll, a little more swell with work, never loud', () => {
  for (const [tpm, turns] of [[0, 0], [300, 0], [0, 1], [15_000, 1], [60_000, 4]] as const) {
    assert.equal(surfHeight({ ...idle, tpm, turns }), seaHeight(seaTarget(tpm, turns), true), `${tpm} tokens a minute, ${turns} turns`)
  }
  const roll = surfVoice(SEA_ROLL)
  assert.equal(roll.gain, SURF_GAIN_ROLL)
  assert.equal(roll.rate, 1)
  assert.ok(roll.wash < 0.6, `the roll laps, it barely breaks: wash ${roll.wash}`)
  let last = roll
  for (let h = SEA_ROLL; h <= 1.0001; h += 0.05) {
    const v = surfVoice(h)
    assert.ok(v.gain >= last.gain && v.wash >= last.wash && v.rate >= last.rate, `more work never quietens it (${h.toFixed(2)})`)
    assert.ok(v.gain <= SURF_GAIN_FULL && v.wash <= 1 && v.rate <= 1.35)
    last = v
  }
  const busy = surfVoice(surfHeight({ ...idle, tpm: 15_000, turns: 1 }))
  assert.ok(busy.gain > roll.gain * 1.3, `a busy sea is heard rising: ${busy.gain} against ${roll.gain}`)
  // A heavy sea is at most about 6 dB over the roll: it raises the swell a little; it never gets loud.
  assert.ok(20 * Math.log10(surfVoice(1).gain / roll.gain) <= 6, 'a heavy sea within 6 dB of the roll')
})

test('a fade is continuous: turned off halfway in, the sea goes from where it was, in its share of the fade-out', () => {
  const inn = fadeTo(null, 10, 1, SURF_FADE_IN_S)
  assert.deepEqual([rampAt(inn, 10), rampAt(inn, 10 + SURF_FADE_IN_S / 2), rampAt(inn, 10 + SURF_FADE_IN_S)], [0, 0.5, 1])
  const out = fadeTo(inn, 11, 0, SURF_FADE_OUT_S)
  assert.equal(out.v0, 0.5)
  assert.equal(out.t1 - out.t0, SURF_FADE_OUT_S / 2)
  assert.equal(rampAt(out, out.t1), 0)
})

test('the waves: a seamless loop of a swell and its break, every five to nine seconds, deep enough to be waves', () => {
  const [swell, wash] = surfWaves()
  assert.equal(swell.length, WAVES_S * WAVES_RATE)
  for (const sig of [swell, wash]) {
    let lo = 1, hi = 0
    for (const v of sig) { lo = Math.min(lo, v); hi = Math.max(hi, v) }
    assert.ok(lo >= 0 && hi <= 1.0000001 && hi > 0.999, `0 to 1: ${lo} to ${hi}`)
    assert.ok(lo < 0.3, `the trough between waves falls well under the crest (static would not): ${lo}`)
    assert.ok(Math.abs(sig[0] - sig[sig.length - 1]) < 0.03, 'no step where the loop comes round')
  }
  // The crests: local peaks of the swell over its floor, a second apart at least.
  const crests: number[] = []
  const r = WAVES_RATE
  for (let i = r; i < swell.length - r; i += 40) {
    let peak = true
    for (let j = i - r; j <= i + r; j += 40) if (swell[j] > swell[i]) { peak = false; break }
    if (peak && swell[i] > 0.35 && (!crests.length || i - crests[crests.length - 1] > r)) crests.push(i)
  }
  assert.ok(crests.length >= 5 && crests.length <= 9, `${crests.length} waves in ${WAVES_S} s`)
  for (let k = 1; k < crests.length; k++) {
    const gap = (crests[k] - crests[k - 1]) / r
    assert.ok(gap > 3 && gap < 11, `a wave every five to nine seconds or so: ${gap.toFixed(1)} s`)
  }
  // Each break comes at its wave's crest, not between waves.
  let washAtCrest = 0
  for (const c of crests) for (let j = c - r; j < c + 2 * r; j += 40) washAtCrest = Math.max(washAtCrest, wash[j])
  assert.ok(washAtCrest > 0.6, `the wash runs up at a crest: ${washAtCrest}`)
  assert.deepEqual(surfWaves(7), surfWaves(7), 'seeded: the same waves every time')
})

test('the noise is pink and loops without a click: its energy falls with frequency, its seam is smooth', () => {
  const n = Math.round(NOISE_S[0] * 8000)
  const x = pinkNoise(1, n)
  let sq = 0, dsq = 0, worst = 0
  for (let i = 0; i < n; i++) {
    sq += x[i] * x[i]
    const d = x[i] - x[(i + n - 1) % n]
    dsq += d * d
    worst = Math.max(worst, Math.abs(d))
  }
  assert.ok(Math.abs(Math.sqrt(sq / n) - 0.3) < 0.01, 'RMS 0.3')
  // White noise's sample-to-sample change carries twice its power; pink noise's, far less.
  assert.ok(dsq / sq < 0.6, `mostly low: ${(dsq / sq).toFixed(2)}`)
  const seam = Math.abs(x[0] - x[n - 1])
  assert.ok(seam < worst, `the seam is no bigger a step than the noise's own: ${seam} of ${worst}`)
})

test('tokens a minute: the model calls of the last sixty seconds, from the end of the page’s copy of the ledger', () => {
  const now = 10_000_000
  const call = (at: number, tokens: number) => ({ kind: 'provider.call', at_unix_ms: at, data: { usage: { input_tokens: tokens, output_tokens: 1 } } })
  const rows = [
    call(now - 3_600_000, 9e6), call(now - 90_000, 500), { kind: 'turn.ended', at_unix_ms: now - 30_000, data: { usage: { input_tokens: 7e5 } } },
    call(now - 50_000, 200), call(now - 2_000, 99),
  ]
  assert.equal(tpmOf(rows, now), 200 + 1 + 99 + 1, 'model calls only, and only the minute’s')
  assert.equal(tpmOf([], now), 0)
  // The scan stops at the first row well before the minute: what lies before it in the ledger's order is never read,
  // so the whole ledger's copy costs a few minutes' rows.
  assert.equal(tpmOf([call(now - 10_000, 7), call(now - 3_600_000, 9e6), call(now - 59_000, 10)], now), 11)
})

test('each sound says where it comes from: the sea, too, is made in the browser', () => {
  assert.match(SURF.source, /synthesized in the browser/)
  assert.equal(SURF.pages, 'every page')
})

// The graph, on stand-ins for Web Audio.
installFakeAudio()

/** The sea's gains (`level`, `wash`, `duck`, `fade`) in the context, in the order `audio.ts` makes them. */
function seaGains(ctx: FakeContext) {
  const gains = ctx.nodes.filter((n) => n.kind === 'gain') as (FakeNode & { gain: FakeParam })[]
  // The master is the first gain; the sea's fade the one that feeds it.
  const master = gains[0]
  const fade = gains.find((g) => g.outs.some((o) => o.to === master))!
  const duck = gains.find((g) => g.outs.some((o) => o.to === fade))!
  const level = gains.find((g) => g.outs.some((o) => o.to === duck))!
  return { master, fade, duck, level }
}

test('it starts on the gesture: asked for before, nothing is made; the toggle’s click starts it, fading in', () => {
  const a = new ShipAudio()
  const before = made.length
  a.surf(SEA_ROLL)
  assert.equal(made.length, before, 'no audio context before the gesture')
  assert.equal(a.surfing, null)
  assert.equal(a.start(), true, 'the click starts the context')
  const ctx = made[made.length - 1]
  assert.equal(made.length, before + 1)
  assert.deepEqual(a.surfing?.height, SEA_ROLL, 'the sea asked for comes in with it')
  const { fade, level } = seaGains(ctx)
  const t0 = ctx.currentTime
  assert.equal(fade.gain.valueAt(t0), 0, 'from silence')
  assert.ok(Math.abs(fade.gain.valueAt(t0 + SURF_FADE_IN_S / 2) - 0.5) < 1e-9, 'halfway in at half the fade')
  assert.equal(fade.gain.valueAt(t0 + SURF_FADE_IN_S), 1, `all in after ${SURF_FADE_IN_S} s`)
  assert.equal(level.gain.valueAt(t0), SURF_GAIN_ROLL, 'at the roll’s quiet level from the start')
  assert.ok(ctx.playing(t0 + 60).length >= 4, 'its noise, its waves and its drift play, looping')
  a.dispose()
})

test('after a reload the browser holds the context: nothing of the sea is made until the first click resumes it', () => {
  FakeContext.held = true
  try {
    const a = new ShipAudio()
    a.surf(SEA_ROLL)
    assert.equal(a.start(), false, 'made on the page’s load, held')
    const ctx = made[made.length - 1]
    assert.equal(a.surfing, null, 'no sea on a held context')
    // The page's height may come after the context is made (the cues' effect wakes it first): still nothing.
    a.surf(SEA_ROLL + 0.1)
    assert.equal(a.surfing, null, 'a height on a held context is only remembered')
    assert.equal(ctx.nodes.filter((n) => n.kind === 'buffer-source').length, 0, 'no source scheduled on it')
    FakeContext.held = false
    assert.equal(a.start(), true, 'the first click resumes it')
    assert.equal(a.surfing?.height, SEA_ROLL + 0.1, 'and the resume brings the sea in, at the last height asked')
    const { fade } = seaGains(ctx)
    assert.equal(fade.gain.valueAt(0), 0)
    assert.equal(fade.gain.valueAt(SURF_FADE_IN_S), 1)
    a.dispose()
  } finally {
    FakeContext.held = false
  }
})

test('it follows the height on the audio thread: up with the sea’s rise, down with its settling', () => {
  const a = new ShipAudio()
  a.start()
  const ctx = made[made.length - 1]
  a.surf(SEA_ROLL)
  const { level } = seaGains(ctx)
  ctx.currentTime = 10
  const busy = surfHeight({ ...idle, tpm: 15_000, turns: 1 })
  a.surf(busy)
  const want = surfVoice(busy).gain
  const at = (t: number) => level.gain.valueAt(t)
  const part = (t: number) => (at(t) - SURF_GAIN_ROLL) / (want - SURF_GAIN_ROLL)
  assert.ok(Math.abs(part(10 + SEA_RISE_S) - (1 - Math.exp(-1))) < 1e-6, 'one rise time: 63% of the way')
  assert.ok(part(10 + 5 * SEA_RISE_S) > 0.99, 'risen')
  ctx.currentTime = 40
  a.surf(SEA_ROLL)
  const down = (level.gain.valueAt(40 + SEA_SETTLE_S) - want) / (SURF_GAIN_ROLL - want)
  assert.ok(Math.abs(down - (1 - Math.exp(-1))) < 1e-3, `settles over the sea’s settling time: ${down}`)
  a.dispose()
})

test('sound off (or Calm) fades it out and stops it; on again, it comes back in', () => {
  const a = new ShipAudio()
  a.start()
  const ctx = made[made.length - 1]
  a.surf(SEA_ROLL)
  const { fade } = seaGains(ctx)
  ctx.currentTime = 30
  a.surf(0)
  assert.equal(a.surfing, null)
  assert.equal(fade.gain.valueAt(30), 1)
  assert.ok(Math.abs(fade.gain.valueAt(30 + SURF_FADE_OUT_S / 2) - 0.5) < 1e-9)
  assert.equal(fade.gain.valueAt(30 + SURF_FADE_OUT_S), 0)
  assert.equal(ctx.playing(30 + SURF_FADE_OUT_S + 0.1).length, 0, 'every source stopped once it has gone: no work for silence')
  ctx.currentTime = 50
  a.surf(SEA_ROLL)
  assert.equal(a.surfing?.height, SEA_ROLL)
  assert.ok(ctx.playing(60).length >= 4, 'a new sea, fading in')
  a.dispose()
})

test('a cue ducks the sea: the bell and the horn stand clear over it, then it comes back', () => {
  const a = new ShipAudio()
  a.start()
  const ctx = made[made.length - 1]
  a.surf(surfHeight({ ...idle, tpm: 15_000, turns: 1 }))
  const { duck } = seaGains(ctx)
  for (const [cue, at] of [['bell', 20], ['horn', 40]] as const) {
    ctx.currentTime = at
    a.play(cue)
    const t = at + 0.02
    assert.ok(duck.gain.valueAt(t - 0.01) > 0.999, `${cue}: the sea full until it sounds`)
    assert.ok(duck.gain.valueAt(t + 0.2) < DUCK[cue].depth + 0.02, `${cue}: ducked under it at once: ${duck.gain.valueAt(t + 0.2)}`)
    assert.ok(duck.gain.valueAt(t + DUCK[cue].hold) < DUCK[cue].depth + 0.02, `${cue}: held through it`)
    assert.ok(duck.gain.valueAt(t + DUCK[cue].hold + 4) > 0.99, `${cue}: back after`)
    assert.ok(DUCK[cue].depth <= 0.3, `${cue}: at least 10 dB down`)
  }
  // No sea, no duck: a cue alone still plays.
  const b = new ShipAudio()
  b.start()
  b.play('bell')
  assert.equal(b.surfing, null)
  a.dispose()
  b.dispose()
})
