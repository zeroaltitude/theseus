// The Ship's motion table (`src/ship/motion.ts`, theseus-hnof.2, step 3), run by `npm test`: each event starts one
// motion, nothing moves that the table does not name, and an idle fleet draws nothing.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { MOTIONS, motionsNow, paceOf, STEADY_FPS, type MotionState } from '../src/ship/motion.ts'
import { IDLE_FPS, Loop, type Clock } from '../src/ship/loop.ts'

const idle: MotionState = {
  calm: false, camera: false, settling: false, t: 100, until: {},
  rowing: false, working: false, streaming: false, gears: false, tethers: false, currents: false, sea: false,
}

test('the table: one row a motion, one event a row, and each one-off says how long it plays', () => {
  const ids = MOTIONS.map((m) => m.id)
  assert.equal(new Set(ids).size, ids.length)
  assert.equal(new Set(MOTIONS.map((m) => m.event)).size, MOTIONS.length, 'two motions share an event')
  for (const m of MOTIONS) {
    assert.ok(m.event && m.moves, m.id)
    const secs = (m as { secs?: number }).secs
    if (m.pace === 'steady' || m.pace === 'sea') assert.equal(secs, undefined, `${m.id} lasts while its state does`)
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
    tethers: true, currents: true, sea: true,
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
    if (!/\b(uTime|uSwell)\b/.test(code) || code.startsWith('//') || /^uniform float (uTime|uSwell);$/.test(code)) return
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
