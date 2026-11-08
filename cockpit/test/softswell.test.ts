// The swell on a software renderer (`webgl.ts`'s `softwareRenderer`, `motion.ts`'s `paceOf`, theseus-yx5i), run by
// `npm test`: a CPU rasteriser (SwiftShader, llvmpipe) draws the swell at a lower rate, and a GPU, or a browser that
// will not say which it has, draws it as it always did.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { IDLE_FPS, Loop, type Clock } from '../src/ship/loop.ts'
import { paceOf, QUIET_FPS, ROLL_FPS, SOFT_SWELL_FPS, STEADY_FPS } from '../src/ship/motion.ts'
import { isSoftwareRenderer, softwareRenderer } from '../src/ship/webgl.ts'

/** What a context's `WEBGL_debug_renderer_info` gives: the extension, and `getParameter` answering its one constant. */
const UNMASKED = 0x9246
const context = (renderer: unknown) => ({
  getExtension: (n: string) => (n === 'WEBGL_debug_renderer_info' ? { UNMASKED_RENDERER_WEBGL: UNMASKED } : null),
  getParameter: (p: number) => (p === UNMASKED ? renderer : null),
})

const SOFTWARE = [
  // Headless Chrome on a machine with no GPU (the case that measured 2.4 to 3.7 cores), and a desktop one.
  'ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero) (0x0000C0DE)), SwiftShader driver)',
  'Google SwiftShader',
  'ANGLE (Google, Vulkan 1.1.0 (SwiftShader Device (LLVM 10.0.0) (0x0000C0DE)), SwiftShader driver-5.0.0)',
  // Mesa's CPU drivers, bare (Firefox, Linux Chrome without ANGLE) and under ANGLE.
  'llvmpipe (LLVM 15.0.7, 256 bits)',
  'ANGLE (Mesa, llvmpipe (LLVM 17.0.6 256 bits), OpenGL 4.5)',
  'softpipe',
  'Mesa Software Rasterizer',
  // Windows without a driver.
  'ANGLE (Microsoft, Microsoft Basic Render Driver (0x0000008C) Direct3D11 vs_5_0 ps_5_0, D3D11)',
]

const GPU = [
  'ANGLE (NVIDIA, NVIDIA GeForce RTX 4070 (0x00002786) Direct3D11 vs_5_0 ps_5_0, D3D11)',
  'ANGLE (Apple, ANGLE Metal Renderer: Apple M1 Max, Unspecified Version)',
  'ANGLE (Intel, Mesa Intel(R) Xe Graphics (TGL GT2), OpenGL 4.6)',
  'ANGLE (AMD, AMD Radeon RX 7800 XT (0x0000747E) Direct3D11 vs_5_0 ps_5_0, D3D11)',
  'AMD Radeon Pro 5500M OpenGL Engine',
  'Mesa Intel(R) UHD Graphics 620 (KBL GT2)',
  'D3D12 (NVIDIA GeForce RTX 3080)',
  'Adreno (TM) 740',
  'Mali-G78',
  'Apple GPU',
  // What a browser that masks its renderer says: nothing to go on.
  'WebKit WebGL',
  'Mozilla',
  'Google Inc.',
  '',
]

test('the renderer names a CPU rasteriser gives are software, and a GPU’s are not', () => {
  for (const n of SOFTWARE) assert.equal(isSoftwareRenderer(n), true, n)
  for (const n of GPU) assert.equal(isSoftwareRenderer(n), false, n)
})

test('a context whose unmasked renderer is SwiftShader or llvmpipe is software; a GPU’s is not', () => {
  assert.equal(softwareRenderer(context('ANGLE (Google, Vulkan 1.3.0 (SwiftShader Device (Subzero) (0x0000C0DE)), SwiftShader driver)')), true)
  assert.equal(softwareRenderer(context('llvmpipe (LLVM 15.0.7, 256 bits)')), true)
  assert.equal(softwareRenderer(context('ANGLE (NVIDIA, NVIDIA GeForce RTX 4070 (0x00002786) Direct3D11 vs_5_0 ps_5_0, D3D11)')), false)
})

test('detection fails toward the GPU path: no context, no extension, no string, or a throw is not software', () => {
  assert.equal(softwareRenderer(null), false)
  assert.equal(softwareRenderer(undefined), false)
  assert.equal(softwareRenderer({}), false, 'a context with no getExtension')
  assert.equal(softwareRenderer({ getExtension: () => null }), false, 'a browser that hides the extension')
  assert.equal(softwareRenderer({ getExtension: () => ({}), getParameter: () => 'llvmpipe' }), false, 'an extension without its constant')
  for (const bad of [null, undefined, 7, {}, ['llvmpipe']]) assert.equal(softwareRenderer(context(bad)), false, String(bad))
  assert.equal(softwareRenderer({ getExtension: () => { throw new Error('denied') } }), false)
  assert.equal(softwareRenderer({
    getExtension: () => ({ UNMASKED_RENDERER_WEBGL: UNMASKED }), getParameter: () => { throw new Error('lost') },
  }), false)
})

test('on a software renderer the swell draws SOFT_SWELL_FPS (5) a second; on a GPU it draws what it always did', () => {
  assert.equal(SOFT_SWELL_FPS, 5)
  assert.equal(IDLE_FPS, 15)
  assert.equal(ROLL_FPS, 8)
  // The sea above the roll (the work going on): 15 on a GPU, 5 on a CPU rasteriser.
  assert.deepEqual(paceOf(['sea'], IDLE_FPS), { fps: 15, full: false, display: false })
  assert.deepEqual(paceOf(['sea'], IDLE_FPS, false, false), { fps: 15, full: false, display: false })
  assert.deepEqual(paceOf(['sea'], IDLE_FPS, false, true), { fps: 5, full: false, display: false })
  // The idle roll: 8 on a GPU, 5 on a CPU rasteriser.
  assert.deepEqual(paceOf(['roll'], IDLE_FPS), { fps: 8, full: false, display: false })
  assert.deepEqual(paceOf(['roll'], IDLE_FPS, false, false), { fps: 8, full: false, display: false })
  assert.deepEqual(paceOf(['roll'], IDLE_FPS, false, true), { fps: 5, full: false, display: false })
  // Nothing moving still asks for nothing, and a rate already under the soft one is not raised.
  assert.deepEqual(paceOf([], IDLE_FPS, false, true), { fps: 0, full: false, display: false })
  assert.equal(paceOf(['sea'], 3, false, true).fps, 3)
})

test('the soft rate is the swell’s alone: one-offs, steady motions and the quiet pace keep theirs on a CPU rasteriser', () => {
  for (const soft of [false, true]) {
    assert.deepEqual(paceOf(['node-born'], IDLE_FPS, false, soft), { fps: 60, full: true, display: true }, `display, soft=${soft}`)
    assert.deepEqual(paceOf(['gear'], IDLE_FPS, false, soft), { fps: 30, full: true, display: false }, `steady, soft=${soft}`)
    assert.deepEqual(paceOf(['gear'], IDLE_FPS, true, soft), { fps: 10, full: true, display: false }, `quiet, soft=${soft}`)
    assert.equal(paceOf(['rowing', 'sea'], IDLE_FPS, false, soft).fps, 30, `steady over the sea, soft=${soft}`)
  }
  assert.equal(STEADY_FPS, 30)
  assert.equal(QUIET_FPS, 10)
})

// A fake 60 Hz display and its timers, as in motion.test.ts.
function framesIn(fps: () => number, ms: number): number {
  const frames: ((w: number) => void)[] = []
  const timers: { at: number; cb: () => void }[] = []
  let t = 0
  const clock: Clock = {
    now: () => t, frame: (cb) => frames.push(cb), cancelFrame: () => {}, hidden: () => false,
    timer: (cb, after) => timers.push({ at: t + after, cb }), cancelTimer: () => {},
  }
  let drawn = 0
  const loop = new Loop(clock, () => { drawn++; return false }, fps)
  loop.request()
  while (t < ms) {
    const vs = (Math.floor(t / (1000 / 60) + 1e-6) + 1) * (1000 / 60)
    timers.sort((a, b) => a.at - b.at)
    const next = Math.min(timers[0]?.at ?? Infinity, frames.length ? vs : Infinity)
    if (next > ms) break
    t = next
    if (timers[0]?.at === t) timers.shift()!.cb()
    else frames.splice(0).forEach((cb) => cb(t))
  }
  return drawn
}

test('on the loop, a second of swell is about 5 frames on a software renderer and 15 (sea) or 8 (roll) on a GPU', () => {
  const within = (n: number, want: number, what: string) => assert.ok(Math.abs(n - want) <= 1, `${what}: ${n} frames, wanted ${want}`)
  // The first frame is the request's; the swell's follow it.
  within(framesIn(() => paceOf(['sea'], IDLE_FPS, false, true).fps, 10_000) / 10, 5, 'sea, software')
  within(framesIn(() => paceOf(['sea'], IDLE_FPS, false, false).fps, 10_000) / 10, 15, 'sea, GPU')
  within(framesIn(() => paceOf(['roll'], IDLE_FPS, false, true).fps, 10_000) / 10, 5, 'roll, software')
  within(framesIn(() => paceOf(['roll'], IDLE_FPS, false, false).fps, 10_000) / 10, 8, 'roll, GPU')
})

test('the engine asks its own context whether it is software, and passes the answer to the pace', () => {
  const engine = readFileSync(new URL('../src/ship/engine.ts', import.meta.url), 'utf8')
  assert.match(engine, /this\.soft\s*=\s*softwareRenderer\(this\.renderer\.getContext\(\)\)/, 'the engine reads its renderer')
  assert.match(engine, /paceOf\(motions, IDLE_FPS, this\.quiet, this\.soft\)/, 'the pace is told')
})
