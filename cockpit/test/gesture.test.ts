// Sound starts only from the Sound button's click (`ShipAudio.turnOn`, `useShipSound`; theseus-pl0x), run by `npm
// test`: no audio context is made or resumed at load or on any other click, so the browser never logs its autoplay
// notice; the click's bell and the sea come when the browser's `resume()` answers, however long it takes; and sound is
// off on every page load.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readdirSync, readFileSync, statSync } from 'node:fs'
import { SEA_ROLL } from '../src/ship/sea.ts'
import { SURF_FADE_IN_S } from '../src/ship/surf.ts'
import { ShipAudio } from '../src/ship/audio.ts'
import { FakeContext, FakeNode, FakeParam, FakeSource, installFakeAudio, made } from './fakeaudio.ts'

installFakeAudio()

/** The bell's strikes in a context: each strikes its prime partial, on E5. */
const strikes = (ctx: FakeContext) => ctx.nodes.filter((n): n is FakeSource =>
  n instanceof FakeSource && n.kind === 'oscillator' && Math.abs(n.frequency.value / 659.25 - 1) < 0.002)

/** The sea's fade: the gain that feeds the master (the context's first gain). */
function seaFade(ctx: FakeContext) {
  const gains = ctx.nodes.filter((n) => n.kind === 'gain') as (FakeNode & { gain: FakeParam })[]
  return gains.find((g) => g.outs.some((o) => o.to === gains[0]))!
}

const later = (ms: number) => new Promise((done) => setTimeout(done, ms))
// A resume that never answers would hang the run: each test that waits on one fails after 5 s instead.

test('nothing but the click makes a context: a height asked for, a cue, before it, make none', () => {
  const before = made.length
  const a = new ShipAudio()
  a.surf(SEA_ROLL)
  a.play('bell')
  a.play('horn')
  a.surf(SEA_ROLL + 0.2)
  assert.equal(made.length, before, 'no audio context')
  assert.equal(a.running, false)
  assert.equal(a.surfing, null)
})

test('the toggle’s bell and the sea come when the browser lets it run, however long its resume takes', { timeout: 5000 }, async () => {
  FakeContext.slow = []
  try {
    const a = new ShipAudio()
    a.surf(SEA_ROLL)
    const on = a.turnOn(() => true)
    const ctx = made[made.length - 1]
    assert.equal(ctx.state, 'suspended', 'the browser has not answered yet')
    // Longer than any fixed wait a page might guess: still nothing scheduled on a context that does not run.
    await later(150)
    assert.equal(strikes(ctx).length, 0, 'no bell before the answer')
    assert.equal(a.surfing, null, 'no sea before the answer')
    ctx.currentTime = 0.4
    FakeContext.slow.shift()!()
    assert.equal(await on, true)
    const bell = strikes(ctx)
    assert.equal(bell.length, 2, 'the bell, struck twice, once the context runs')
    assert.ok(bell.every((o) => o.started!.t >= 0.4), 'rung when it runs, not before')
    assert.equal(a.surfing?.height, SEA_ROLL, 'the sea comes in with it')
    const fade = seaFade(ctx)
    assert.equal(fade.gain.valueAt(0.4), 0, 'its fade-in starts at the answer')
    assert.equal(fade.gain.valueAt(0.4 + SURF_FADE_IN_S), 1)
    a.dispose()
  } finally {
    FakeContext.slow = null
  }
})

test('turned off again before the browser answers: no bell', { timeout: 5000 }, async () => {
  FakeContext.slow = []
  try {
    const a = new ShipAudio()
    let still = true
    const on = a.turnOn(() => still)
    const ctx = made[made.length - 1]
    still = false
    FakeContext.slow.shift()!()
    assert.equal(await on, true)
    assert.equal(strikes(ctx).length, 0)
    a.dispose()
  } finally {
    FakeContext.slow = null
  }
})

test('the browser refuses: the click says so, and nothing plays', { timeout: 5000 }, async () => {
  FakeContext.held = true
  try {
    const a = new ShipAudio()
    a.surf(SEA_ROLL)
    assert.equal(await a.turnOn(() => true), false)
    const ctx = made[made.length - 1]
    assert.equal(strikes(ctx).length, 0)
    assert.equal(a.surfing, null)
    assert.equal(ctx.nodes.filter((n) => n instanceof FakeSource && n.started).length, 0, 'nothing scheduled on a held context')
    a.dispose()
  } finally {
    FakeContext.held = false
  }
})

/** Every source file of the page, but `audio.ts`. */
function pageSources(dir = new URL('../src/', import.meta.url)): [string, string][] {
  const out: [string, string][] = []
  for (const name of readdirSync(dir)) {
    const u = new URL(name, dir)
    if (statSync(u).isDirectory()) out.push(...pageSources(new URL(`${name}/`, dir)))
    else if (/\.tsx?$/.test(name) && !u.pathname.endsWith('/src/ship/audio.ts')) out.push([u.pathname, readFileSync(u, 'utf8')])
  }
  return out
}

test('the page starts sound in one place, the Sound button’s click; nothing at load, nothing on another click', () => {
  for (const [path, src] of pageSources()) {
    assert.ok(!/AudioContext\s*\(|webkitAudioContext|\.resume\(/.test(src), `${path} makes or resumes no audio context`)
    assert.ok(!/(theAudio\(\)|\ba)\.start\(/.test(src), `${path} starts no audio`)
  }
  const hook = readFileSync(new URL('../src/ship/useShipSound.ts', import.meta.url), 'utf8')
  assert.equal(hook.match(/\.turnOn\(/g)?.length, 1, 'one call')
  const toggle = hook.slice(hook.indexOf('const toggle = () => {'), hook.indexOf('return { on, toggle }'))
  assert.match(toggle, /if \(!next\) return[\s\S]*theAudio\(\)\.turnOn\(\(\) => useSound\.getState\(\)\.on\)/, 'in the toggle, turning it on')
  assert.ok(!/setTimeout/.test(toggle), 'the bell waits for the browser’s answer, not a timer')
  assert.match(toggle, /\.then\(\(runs\) => \{ if \(!runs\) useSound\.setState\(\{ on: false \}\) \}\)/, 'refused, the button goes back to off')
  assert.ok(!/addEventListener\('(pointerdown|keydown)'/.test(hook), 'no other click wakes it')
})
