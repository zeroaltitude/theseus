// The full-window graph's address and keys (theseus-uu3b2).
import test from 'node:test'
import assert from 'node:assert/strict'
import { REFIT_RETRY_MS, fullKey, parseFull, scheduleRefit, writeFull } from '../src/lib/fullgraph.ts'

test('Esc leaves full screen and an unrelated key does not', () => {
  assert.equal(fullKey({ key: 'Escape' }, true), 'leave')
  assert.equal(fullKey({ key: 'Escape' }, false), null)
  assert.equal(fullKey({ key: 'x' }, true), null)
  assert.equal(fullKey({ key: 'f' }, false), 'enter')
  assert.equal(fullKey({ key: 'f' }, true), 'leave')
  assert.equal(fullKey({ key: 'f', ctrlKey: true }, false), null)
})

test('the address round-trips', () => {
  const base = new URLSearchParams('task=t1&taskgraph=1')
  const on = writeFull(base, true)
  assert.equal(parseFull(on), true)
  assert.equal(on.get('task'), 't1')
  assert.equal(on.get('taskgraph'), '1')
  const off = writeFull(on, false)
  assert.equal(parseFull(off), false)
  assert.equal(off.toString(), base.toString())
  assert.equal(parseFull(new URLSearchParams('graph=huge')), false)
})

test('a key typed in a field does nothing', () => {
  assert.equal(fullKey({ key: 'f', target: { tagName: 'INPUT' } }, false), null)
  assert.equal(fullKey({ key: 'f', target: { tagName: 'textarea' } }, false), null)
  assert.equal(fullKey({ key: 'f', target: { tagName: 'DIV', isContentEditable: true } }, false), null)
  assert.equal(fullKey({ key: 'f', target: { tagName: 'DIV' } }, false), 'enter')
})

/** A clock that runs what is due only when told: `step` runs the next frame, `tick` the next timer. */
function fakeClock() {
  const frames: Array<{ cb: () => void; live: boolean }> = []
  const timers: Array<{ ms: number; cb: () => void; live: boolean }> = []
  return {
    frames, timers,
    clock: {
      frame: (cb: () => void) => { const f = { cb, live: true }; frames.push(f); return () => { f.live = false } },
      after: (ms: number, cb: () => void) => { const t = { ms, cb, live: true }; timers.push(t); return () => { t.live = false } },
    },
    step: () => { const f = frames.find((x) => x.live); if (f) { f.live = false; f.cb() } },
    tick: () => { const t = timers.find((x) => x.live); if (t) { t.live = false; t.cb() } },
  }
}

test('a refit fits after two frames, and once more for the late observer (theseus-4qx8o)', () => {
  const c = fakeClock()
  let fits = 0
  scheduleRefit(() => { fits += 1 }, c.clock)
  assert.equal(fits, 0)
  c.step()
  assert.equal(fits, 0, 'one frame is not enough: the layout has not run')
  c.step()
  assert.equal(fits, 1, 'the fit after two frames')
  assert.equal(c.timers.length, 1)
  assert.equal(c.timers[0].ms, REFIT_RETRY_MS)
  c.tick()
  assert.equal(fits, 2, 'the retry fits the box the observer reported after the frames')
})

test('a refit cancelled at any point runs nothing more', () => {
  for (const stopAfter of [0, 1, 2]) {
    const c = fakeClock()
    let fits = 0
    const cancel = scheduleRefit(() => { fits += 1 }, c.clock)
    for (let i = 0; i < stopAfter; i++) c.step()
    const before = fits
    cancel()
    c.step(); c.step(); c.tick()
    assert.equal(fits, before, `cancelled after ${stopAfter} frames`)
  }
})
