// The Ship's loop (`src/ship/loop.ts`, theseus-wp2d), run by `npm test` on a fake clock: a 60 Hz display whose frames
// the "browser" pauses while the tab is hidden, and timers that fire when due.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { IDLE_FPS, Loop, type Clock, type Tick } from '../src/ship/loop.ts'

const VSYNC = 1000 / 60

class FakeClock implements Clock {
  t = 0
  isHidden = false
  timersSet = 0
  private seq = 0
  private frames = new Map<number, (when: number) => void>()
  private timers = new Map<number, { at: number; cb: () => void }>()
  now() { return this.t }
  frame(cb: (when: number) => void) { const id = ++this.seq; this.frames.set(id, cb); return id }
  cancelFrame(id: number) { this.frames.delete(id) }
  timer(cb: () => void, ms: number) { this.timersSet++; const id = ++this.seq; this.timers.set(id, { at: this.t + ms, cb }); return id }
  cancelTimer(id: number) { this.timers.delete(id) }
  hidden() { return this.isHidden }
  /** What waits: display frames asked for, and timers set. */
  get waiting() { return { frames: this.frames.size, timers: this.timers.size } }
  /** Runs the clock to `until`: each timer when due, and the display frames asked for at each vsync (none while
   *  hidden, as a browser pauses them). */
  run(until: number) {
    for (;;) {
      const vsync = (Math.floor(this.t / VSYNC + 1e-6) + 1) * VSYNC
      let next: { at: number; id: number } | null = null
      for (const [id, x] of this.timers) if (!next || x.at < next.at) next = { at: x.at, id }
      const frameAt = this.frames.size && !this.isHidden ? vsync : Infinity
      const at = Math.min(next?.at ?? Infinity, frameAt)
      if (at > until) { this.t = until; return }
      this.t = at
      if (next && next.at === at) {
        const cb = this.timers.get(next.id)!.cb
        this.timers.delete(next.id)
        cb()
      } else {
        const due = [...this.frames.values()]
        this.frames.clear()
        for (const cb of due) cb(this.t)
      }
    }
  }
}

/** A loop over a fake clock, its frames recorded; `busy` says whether a frame wants the next one. */
function rig(opts: { swell: boolean; busy?: (t: number) => boolean }) {
  const clock = new FakeClock()
  const drawn: { at: number; tick: Tick }[] = []
  const state = { swell: opts.swell }
  const loop = new Loop(clock, (when, tick) => { drawn.push({ at: when, tick }); return opts.busy?.(when) ?? false }, () => state.swell)
  const between = (a: number, b: number) => drawn.filter((f) => f.at >= a && f.at < b)
  return { clock, loop, drawn, state, between }
}

test('in Live mode with nothing moving, the swell keeps drawing at the idle rate, one timer a frame', () => {
  const { clock, loop, drawn, between } = rig({ swell: true })
  loop.request()
  clock.run(3000)
  assert.equal(drawn[0].tick.changed, true, 'the first frame draws the whole scene')
  assert.ok(drawn.slice(1).every((f) => !f.tick.changed && !f.tick.paced), "the swell's frames draw only the sea")
  const second = between(1000, 2000)
  assert.equal(second.length, IDLE_FPS)
  for (let i = 1; i < second.length; i++) assert.ok(Math.abs(second[i].at - second[i - 1].at - 1000 / IDLE_FPS) < 0.01)
  assert.equal(clock.timersSet, drawn.length, 'one timer for each frame of the swell, and none spinning')
  assert.deepEqual(clock.waiting, { frames: 0, timers: 1 })
})

test('in Calm (or with ?swell=0), nothing moving draws one frame and stops', () => {
  const { clock, loop, drawn } = rig({ swell: false })
  loop.request()
  clock.run(10_000)
  assert.equal(drawn.length, 1)
  assert.equal(drawn[0].tick.changed, true)
  assert.deepEqual(clock.waiting, { frames: 0, timers: 0 })
  assert.deepEqual(loop.pending, { frame: false, timer: false })
  assert.equal(clock.timersSet, 0)
})

test('switching to Calm stills the swell after one frame, and back to Live starts it again', () => {
  const { clock, loop, drawn, state, between } = rig({ swell: true })
  loop.request()
  clock.run(1000)
  state.swell = false
  loop.request()
  clock.run(5000)
  assert.equal(between(1000, 5000).length, 1)
  state.swell = true
  loop.request()
  clock.run(6000)
  assert.equal(between(5000, 6000).length, IDLE_FPS)
  assert.ok(drawn.length > 2 * IDLE_FPS)
})

test('activity raises the rate to every display frame, and settling lowers it to the swell again', () => {
  // Something moves from 1 s to 2 s (a sail, a flight): a change starts it, and each frame says it moves on.
  const { clock, loop, between } = rig({ swell: true, busy: (t) => t >= 1000 && t < 2000 })
  loop.request()
  clock.run(1000)
  loop.request()
  clock.run(4000)
  const idle = between(0, 1000)
  const busy = between(1000, 2000)
  const after = between(2100, 3100)
  assert.ok(Math.abs(idle.length - IDLE_FPS) <= 1, `idle ${idle.length}`)
  assert.ok(busy.length >= 59 && busy.length <= 61, `busy ${busy.length}`)
  assert.ok(busy.slice(1).every((f) => f.tick.paced), 'back-to-back frames are paced: their gaps are frame times')
  assert.ok(Math.abs(after.length - IDLE_FPS) <= 1, `after ${after.length}`)
  assert.ok(after.every((f) => !f.tick.paced && !f.tick.changed))
})

test('a change during the swell\'s wait is drawn at the next display frame, not at its next beat', () => {
  const { clock, loop, drawn } = rig({ swell: true })
  loop.request()
  clock.run(1005)
  const before = drawn.length
  loop.request()
  assert.deepEqual(loop.pending, { frame: true, timer: false }, "the swell's timer gives way")
  clock.run(1005 + VSYNC)
  assert.equal(drawn.length, before + 1)
  assert.equal(drawn.at(-1)!.tick.changed, true)
  assert.ok(drawn.at(-1)!.at - 1005 <= VSYNC)
})

test('a hidden tab schedules nothing and draws nothing; shown again, it draws and the swell goes on', () => {
  const { clock, loop, drawn, between } = rig({ swell: true })
  loop.request()
  clock.run(500)
  clock.isHidden = true
  loop.visibility()
  assert.deepEqual(clock.waiting, { frames: 0, timers: 0 })
  assert.deepEqual(loop.pending, { frame: false, timer: false })
  const set = clock.timersSet
  loop.request() // a push while hidden
  clock.run(60_000)
  assert.equal(between(500, 60_000).length, 0)
  assert.equal(clock.timersSet, set, 'no timer while hidden')
  clock.isHidden = false
  loop.visibility()
  clock.run(61_000)
  const back = between(60_000, 61_000)
  assert.equal(back[0].tick.changed, true, 'shown, the whole scene is drawn: it may have changed while hidden')
  assert.ok(Math.abs(back.length - IDLE_FPS) <= 1, `back ${back.length}`)
  assert.ok(drawn.length > 0)
})

test('a hidden tab mid-activity stops at once, and its first frame back is not taken for a slow one', () => {
  const { clock, loop, between } = rig({ swell: true, busy: () => true })
  loop.request()
  clock.run(305)
  clock.isHidden = true
  loop.visibility()
  assert.deepEqual(clock.waiting, { frames: 0, timers: 0 })
  clock.run(10_000)
  assert.equal(between(305, 10_000).length, 0)
  clock.isHidden = false
  loop.visibility()
  clock.run(10_100)
  const back = between(10_000, 10_100)
  assert.equal(back[0].tick.paced, false)
  assert.ok(back.slice(1).every((f) => f.tick.paced))
})

test('a disposed loop draws nothing and leaves nothing waiting', () => {
  const { clock, loop, drawn } = rig({ swell: true })
  loop.request()
  clock.run(200)
  const n = drawn.length
  loop.dispose()
  loop.request()
  loop.visibility()
  clock.run(5000)
  assert.equal(drawn.length, n)
  assert.deepEqual(clock.waiting, { frames: 0, timers: 0 })
})
