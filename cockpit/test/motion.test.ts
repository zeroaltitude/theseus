// The Ship's motion table (`src/ship/motion.ts`, theseus-hnof.2, step 3), run by `npm test`: each event starts one
// motion, nothing moves that the table does not name, and an idle fleet draws nothing.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import {
  heard, MOTIONS, motionsNow, paceOf, QUIET_AFTER_S, QUIET_FPS, ROLL_FPS, STEADY_FPS, type MotionState,
} from '../src/ship/motion.ts'
import { IDLE_FPS, Loop, type Clock } from '../src/ship/loop.ts'

const idle: MotionState = {
  calm: false, camera: false, settling: false, t: 100, until: {},
  rowing: false, working: false, streaming: false, gears: false, tethers: false, currents: false, sea: false, roll: false,
}

test('the table: one row a motion, one event a row, and each one-off says how long it plays', () => {
  const ids = MOTIONS.map((m) => m.id)
  assert.equal(new Set(ids).size, ids.length)
  assert.equal(new Set(MOTIONS.map((m) => m.event)).size, MOTIONS.length, 'two motions share an event')
  for (const m of MOTIONS) {
    assert.ok(m.event && m.moves, m.id)
    const secs = (m as { secs?: number }).secs
    if (m.pace === 'steady' || m.pace === 'sea' || m.pace === 'roll') assert.equal(secs, undefined, `${m.id} lasts while its state does`)
    else if (m.id !== 'camera' && m.id !== 'settle') assert.ok(secs! > 0 && secs! < 10, `${m.id} plays once, briefly`)
  }
})

test('an idle fleet on a dead-calm sea moves nothing, and asks for no frame', () => {
  assert.deepEqual(motionsNow(idle), [])
  assert.deepEqual(paceOf([], IDLE_FPS), { fps: 0, full: false, display: false })
  // A one-off that has played out is over.
  assert.deepEqual(motionsNow({ ...idle, until: { 'node-born': 99, 'oar-out': 100 } }), [])
})

test('each state moves at its pace: one-offs every display frame, steady motions steadily, the sea alone at its own', () => {
  assert.deepEqual(paceOf(motionsNow({ ...idle, until: { 'oar-out': 100.4 } }), IDLE_FPS), { fps: 60, full: true, display: true })
  assert.deepEqual(motionsNow({ ...idle, gears: true }), ['gear'])
  assert.deepEqual(paceOf(['gear'], IDLE_FPS), { fps: STEADY_FPS, full: true, display: false })
  assert.deepEqual(paceOf(['sea'], IDLE_FPS), { fps: IDLE_FPS, full: false, display: false })
  assert.deepEqual(motionsNow({ ...idle, rowing: true, working: true, sea: true }), ['rowing', 'working', 'sea'])
  assert.equal(paceOf(['rowing', 'working', 'sea'], IDLE_FPS).fps, STEADY_FPS)
  // The coin flies on the page: the canvas needs no frame for it.
  assert.deepEqual(paceOf(['coin'], IDLE_FPS), { fps: 0, full: false, display: false })
})

test('Calm stills every motion but the operator’s own camera and the layout’s', () => {
  const busy: MotionState = {
    ...idle, calm: true, until: { failed: 102, 'node-born': 101 }, rowing: true, working: true, streaming: true, gears: true,
    tethers: true, currents: true, sea: true, roll: true,
  }
  assert.deepEqual(motionsNow(busy), [])
  assert.deepEqual(motionsNow({ ...busy, camera: true }), ['camera'])
})

test('nothing moves that the table does not name: every time-driven term of the shaders names its row', () => {
  const src = readFileSync(new URL('../src/ship/shaders.ts', import.meta.url), 'utf8').split('\n')
  const canvas = new Set<string>(MOTIONS.filter((m) => m.on === 'canvas').map((m) => m.id))
  const named = new Set<string>()
  src.forEach((line, i) => {
    const code = line.trim()
    if (!/\b(uTime|uSwell|uSea)\b/.test(code) || code.startsWith('//') || /^uniform float (uTime|uSwell|uSea);$/.test(code)) return
    const m = /\/\/ motion: ([a-z -]+)$/.exec(code)
    assert.ok(m, `shaders.ts:${i + 1} moves with time but names no motion: ${code}`)
    for (const id of m![1].trim().split(/\s+/)) {
      assert.ok(canvas.has(id), `shaders.ts:${i + 1} names "${id}", which is no canvas motion`)
      named.add(id)
    }
  })
  // The camera and a vessel's glide are the engine's (its camera and its vessel texture); every other canvas motion is
  // a shader's.
  for (const id of canvas) if (id !== 'camera' && id !== 'settle') assert.ok(named.has(id), `no shader draws "${id}"`)
})

test('the loop draws an idle fleet once, then nothing, and a paced motion at its pace', () => {
  // A fake 60 Hz display and its timers (as in loop.test.ts, kept small).
  const frames: ((w: number) => void)[] = []
  const timers: { at: number; cb: () => void }[] = []
  let t = 0
  const clock: Clock = {
    now: () => t, frame: (cb) => frames.push(cb), cancelFrame: () => {}, hidden: () => false,
    timer: (cb, ms) => timers.push({ at: t + ms, cb }), cancelTimer: () => {},
  }
  const run = (until: number) => {
    while (t < until) {
      const vs = (Math.floor(t / (1000 / 60) + 1e-6) + 1) * (1000 / 60)
      timers.sort((a, b) => a.at - b.at)
      const next = Math.min(timers[0]?.at ?? Infinity, frames.length ? vs : Infinity)
      if (next > until) { t = until; return }
      t = next
      if (timers[0]?.at === t) timers.shift()!.cb()
      else frames.splice(0).forEach((cb) => cb(t))
    }
  }
  let state = idle
  let drawn = 0
  const loop = new Loop(clock, () => { drawn++; return paceOf(motionsNow(state), IDLE_FPS).display }, () => paceOf(motionsNow(state), IDLE_FPS).fps)
  loop.request()
  run(10_000)
  assert.equal(drawn, 1, 'a change draws one frame; an idle fleet on a dead-calm sea then draws nothing')
  state = { ...idle, gears: true }
  loop.request()
  drawn = 0
  run(12_000)
  assert.ok(Math.abs(drawn - 2 * STEADY_FPS) <= 2, `a gear turns at the steady pace: ${drawn} frames in 2 s`)
})

test('Live mode with nothing happening rolls the sea slowly: the composite alone, a few frames a second (theseus-42ic)', () => {
  assert.deepEqual(motionsNow({ ...idle, roll: true }), ['roll'])
  assert.ok(ROLL_FPS > 0 && ROLL_FPS <= 10, `an idle Ship stays light: ${ROLL_FPS} frames a second`)
  assert.deepEqual(paceOf(['roll'], IDLE_FPS), { fps: ROLL_FPS, full: false, display: false })
  // The work raises the sea above the roll: the sea's pace, not the roll's; and anything else that moves sets its own.
  assert.deepEqual(motionsNow({ ...idle, sea: true, roll: true }), ['sea'])
  assert.deepEqual(paceOf(motionsNow({ ...idle, gears: true, roll: true }), IDLE_FPS), { fps: STEADY_FPS, full: true, display: false })
  assert.equal(paceOf(motionsNow({ ...idle, roll: true, until: { failed: 101 } }), IDLE_FPS).fps, 60)
  // Calm and reduced motion still it.
  assert.deepEqual(motionsNow({ ...idle, calm: true, roll: true }), [])
  // On the loop: about ROLL_FPS frames a second, each a composite of the sea alone, and none while the tab is hidden.
  const frames: ((w: number) => void)[] = []
  const timers: { at: number; cb: () => void }[] = []
  let t = 0
  let hidden = false
  const clock: Clock = {
    now: () => t, frame: (cb) => frames.push(cb), cancelFrame: () => {}, hidden: () => hidden,
    timer: (cb, ms) => timers.push({ at: t + ms, cb }), cancelTimer: () => {},
  }
  const run = (until: number) => {
    while (t < until) {
      const vs = (Math.floor(t / (1000 / 60) + 1e-6) + 1) * (1000 / 60)
      timers.sort((a, b) => a.at - b.at)
      const next = Math.min(timers[0]?.at ?? Infinity, frames.length && !hidden ? vs : Infinity)
      if (next > until) { t = until; return }
      t = next
      if (timers[0]?.at === t) timers.shift()!.cb()
      else frames.splice(0).forEach((cb) => cb(t))
    }
  }
  const state = { ...idle, roll: true }
  let drawn = 0
  const loop = new Loop(clock, () => { drawn++; return paceOf(motionsNow(state), IDLE_FPS).display }, () => paceOf(motionsNow(state), IDLE_FPS).fps)
  loop.request()
  run(10_000)
  // A paced frame lands on the display frame after its timer, so the rate runs a little over its pace, never past 10.
  assert.ok(drawn >= 10 * ROLL_FPS - 2 && drawn <= 100, `the roll at its pace: ${drawn} frames in 10 s`)
  hidden = true
  loop.visibility()
  drawn = 0
  run(20_000)
  assert.equal(drawn, 0, 'a hidden tab rolls nothing')
})

test('a lasting state drops to the quiet pace after a minute with no event, and an event brings it back (theseus-n2hd)', () => {
  assert.ok(QUIET_FPS <= STEADY_FPS / 2 && QUIET_FPS >= 8, `a gear still reads as turning: ${QUIET_FPS}`)
  assert.equal(QUIET_AFTER_S, 60)
  assert.deepEqual(paceOf(['gear'], IDLE_FPS, true), { fps: QUIET_FPS, full: true, display: false })
  // Quiet never slows a one-off, the sea, or the roll.
  assert.equal(paceOf(['gear', 'failed'], IDLE_FPS, true).fps, 60)
  assert.equal(paceOf(['sea'], IDLE_FPS, true).fps, IDLE_FPS)
  assert.equal(paceOf(['roll'], IDLE_FPS, true).fps, ROLL_FPS)
  const q = { eventAt: 0, steady: '' }
  // A job starts (its gear): an event. A minute of its gear and nothing else: quiet.
  assert.equal(heard(q, ['gear', 'roll'], 10), false)
  assert.equal(heard(q, ['gear', 'roll'], 69.9), false)
  assert.equal(heard(q, ['gear', 'roll'], 70), true)
  assert.equal(heard(q, ['gear', 'roll'], 3600), true)
  // The operator's camera is not an event.
  assert.equal(heard(q, ['camera', 'gear'], 3601), true)
  // A one-off (a result back, a failure) is: the steady pace again, for a minute.
  assert.equal(heard(q, ['result-back', 'gear'], 3602), false)
  assert.equal(heard(q, ['gear'], 3661), false)
  assert.equal(heard(q, ['gear'], 3662), true)
  // So is a change in the lasting states: a turn starts to row beside the job.
  assert.equal(heard(q, ['rowing', 'gear'], 3663), false)
  assert.equal(heard(q, ['rowing', 'gear'], 3724), true)
  assert.equal(heard(q, ['gear'], 3725), false, 'the turn ended')
  // On the loop: a gear at 30 for its first minute, then at 10.
  const frames: ((w: number) => void)[] = []
  const timers: { at: number; cb: () => void }[] = []
  let t = 0
  const clock: Clock = {
    now: () => t, frame: (cb) => frames.push(cb), cancelFrame: () => {}, hidden: () => false,
    timer: (cb, ms) => timers.push({ at: t + ms, cb }), cancelTimer: () => {},
  }
  const run = (until: number) => {
    while (t < until) {
      const vs = (Math.floor(t / (1000 / 60) + 1e-6) + 1) * (1000 / 60)
      timers.sort((a, b) => a.at - b.at)
      const next = Math.min(timers[0]?.at ?? Infinity, frames.length ? vs : Infinity)
      if (next > until) { t = until; return }
      t = next
      if (timers[0]?.at === t) timers.shift()!.cb()
      else frames.splice(0).forEach((cb) => cb(t))
    }
  }
  const lq = { eventAt: 0, steady: '' }
  let quiet = false
  let drawn = 0
  const loop = new Loop(clock, (when) => {
    drawn++
    quiet = heard(lq, ['gear'], when / 1000)
    return false
  }, () => paceOf(['gear'], IDLE_FPS, quiet).fps)
  loop.request()
  run(50_000)
  assert.ok(Math.abs(drawn / 50 - STEADY_FPS) <= 1.5, `the first minute at the steady pace: ${(drawn / 50).toFixed(1)} a second`)
  run(70_000)
  drawn = 0
  run(130_000)
  assert.ok(Math.abs(drawn / 60 - QUIET_FPS) <= 1.5, `then the quiet pace: ${(drawn / 60).toFixed(1)} a second`)
})

test('the engine paces its lasting states by what it heard', () => {
  const engine = readFileSync(new URL('../src/ship/engine.ts', import.meta.url), 'utf8')
  assert.match(engine, /this\.quiet = heard\(this\.quietQ, motions, t\)\s+const pace = paceOf\(motions, IDLE_FPS, this\.quiet\)/)
})
