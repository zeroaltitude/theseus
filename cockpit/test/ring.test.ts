// A failure's ring on the sea at fleet depth (`RING_VERT` in `src/ship/shaders.ts`, theseus-exda), run by `npm test`.
// Node's runner cannot draw WebGL: this reads the shader and the engine, as motion.test.ts reads the shaders.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { RING_FRAG, RING_S, RING_VERT } from '../src/ship/shaders.ts'
import { MOTIONS } from '../src/ship/motion.ts'
import { depthOf } from '../src/ship/words.ts'

const code = (glsl: string) => glsl.split('\n').map((l) => l.trim()).filter((l) => l && !l.startsWith('//'))

test('the ring runs out from the flare and fades within a second', () => {
  assert.ok(RING_S > 0 && RING_S <= 1, `it fades within a second: ${RING_S} s`)
  const v = code(RING_VERT)
  // It starts with the flare: the vessel's burst time, the same texel the flare reads (row 2, x).
  assert.ok(v.includes('vec4 c = vRow(aIdx, 2.0);'))
  assert.ok(v.includes('float age = c.x > 0.0 ? uTime - c.x : 1e6; // motion: failed'))
  // Past its second, under Calm, or with the ship at its own depth, nothing is drawn.
  const gate = v.find((l) => l.startsWith('if (age < 0.0'))
  assert.ok(gate, 'the ring has its gate')
  assert.match(gate!, new RegExp(`age >= ${RING_S.toFixed(2).replace('.', '\\.')}`))
  assert.match(gate!, /uCalm > 0\.5/)
  assert.match(gate!, /fleet <= 0\.0/)
  assert.match(gate!, /gl_Position = vec4\(2\.0, 2\.0, 2\.0, 1\.0\); return;/)
  // It fades as it runs out, and with the zoom towards the ship's depth.
  assert.ok(v.includes(`float k = age / ${RING_S.toFixed(2)};`))
  assert.ok(v.includes('vA = (1.0 - k) * (1.0 - k) * fleet;'))
  assert.ok(RING_FRAG.includes('* vA;'), 'the fragment takes its fade')
})

test('it shows at fleet depth only: gone by the ship’s own depth, where the flare itself is plain', () => {
  assert.equal(depthOf(379, false), 'fleet')
  assert.equal(depthOf(380, false), 'ship')
  assert.ok(code(RING_VERT).includes('float fleet = 1.0 - smoothstep(200.0 * uPixel, 380.0 * uPixel, hullPx);'))
  // Sized in pixels on screen, so it reads the same at any fleet zoom, and it lies on the sea.
  assert.ok(code(RING_VERT).includes('float r = px * depth / uScale;'))
  assert.match(RING_VERT, /vec4\(a\.x \+ position\.x \* 2\.0 \* r, 0\.0, a\.y \+ position\.z \* 2\.0 \* r, 1\.0\)/)
})

test('the engine draws a ring for every vessel, beside the flare, as part of the failed motion', () => {
  const engine = readFileSync(new URL('../src/ship/engine.ts', import.meta.url), 'utf8')
  assert.match(engine, /this\.rings = new THREE\.Mesh\(instanced\(quadXZ\), mat\(RING_VERT, RING_FRAG\)\)/)
  assert.match(engine, /for \(const o of \[this\.rings, this\.hulls,/, 'the rings are in the scene')
  assert.match(engine, /for \(const mesh of \[this\.hulls, this\.sails, this\.rings\]\)/, 'one ring a vessel')
  // The 'failed' motion plays its 3 s from the flare's time: the ring's second is inside it, so the loop draws it.
  assert.match(engine, /if \(fl\) this\.play\('failed', fl \+ 3\)/)
  const failed = MOTIONS.find((m) => m.id === 'failed')!
  assert.match(failed.moves, /at fleet depth a ring runs out on the sea/)
  assert.ok(RING_S <= failed.secs!)
})
