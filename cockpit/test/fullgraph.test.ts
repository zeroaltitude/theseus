// The full-window graph's address and keys (theseus-uu3b2).
import test from 'node:test'
import assert from 'node:assert/strict'
import { FULL_PARAM, G_CHORD_MS, REFIT_RETRY_MS, fullKey, gChord, parseFull, scheduleRefit, writeFull } from '../src/lib/fullgraph.ts'

test('Esc leaves full screen and an unrelated key does not', () => {
  assert.equal(fullKey({ key: 'Escape' }, true), 'leave')
  assert.equal(fullKey({ key: 'Escape' }, false), null)
  assert.equal(fullKey({ key: 'x' }, true), null)
  assert.equal(fullKey({ key: 'f' }, false), 'enter')
  assert.equal(fullKey({ key: 'f' }, true), 'leave')
  assert.equal(fullKey({ key: 'f', ctrlKey: true }, false), null)
})

test('an f right after g is the shell\u2019s jump to the Fleet and never fills the window (theseus-ka0lv)', () => {
  assert.equal(fullKey({ key: 'f' }, false, true), null, 'g f from the Ops page does not enter full screen')
  assert.equal(fullKey({ key: 'f' }, true, true), null, 'nor leave it')
  assert.equal(fullKey({ key: 'f' }, false, false), 'enter', 'a lone f still fills the window')
  assert.equal(fullKey({ key: 'Escape' }, true, true), 'leave', 'Esc leaves even right after a g')
})

test('the g window: an f within G_CHORD_MS of a g is a chord, a later or a second one is not (theseus-ka0lv)', () => {
  let s = gChord(0, { key: 'g' }, 1000)
  assert.equal(s.afterG, false)
  assert.equal(gChord(s.at, { key: 'f' }, 1000 + G_CHORD_MS - 1).afterG, true, 'inside the window')
  assert.equal(gChord(s.at, { key: 'f' }, 1000 + G_CHORD_MS).afterG, false, 'the window has closed')
  const after = gChord(s.at, { key: 'f' }, 1100)
  assert.equal(after.at, 0, 'any key closes the window')
  assert.equal(gChord(after.at, { key: 'f' }, 1200).afterG, false, 'the chord is spent')
  s = gChord(0, { key: 'g', ctrlKey: true }, 1000)
  assert.equal(s.at, 0, 'a modified g starts nothing')
  s = gChord(0, { key: 'g', target: { tagName: 'INPUT' } }, 1000)
  assert.equal(s.at, 0, 'a g typed in a field starts nothing')
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

test('on the Ops page f is a key of the table and the address keeps the graph parameter (theseus-g00vv)', async () => {
  const { KEYS } = await import('../src/ops/keys.ts')
  const { writeAddress, parseAddress, OWN_KEYS } = await import('../src/ops/address.ts')
  assert.ok(KEYS.some((k) => k.verb === 'full_window' && k.keys.includes('f')))
  assert.ok(!(OWN_KEYS as readonly string[]).includes(FULL_PARAM))
  const kept = writeAddress({ ...parseAddress(''), lens: 'timeline' }, new URLSearchParams('graph=full&lens=graph'))
  assert.equal(kept.get('graph'), 'full')
})

test('the hook feeds the g window to fullKey, so g f never pushes ?graph=full (theseus-ka0lv)', async () => {
  const { readFileSync } = await import('node:fs')
  const src = readFileSync(new URL('../src/components/FullGraph.tsx', import.meta.url), 'utf8')
  assert.match(src, /gChord\(g, e, Date\.now\(\)\)/)
  assert.match(src, /fullKey\(e, full, step\.afterG\)/)
})
