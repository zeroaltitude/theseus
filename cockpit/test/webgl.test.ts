// The Ship without WebGL (`src/ship/webgl.ts`), run by `npm test` (theseus-9k53): the probe on a stubbed canvas, and
// an engine whose renderer throws as three's does when the browser has WebGL off.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { hasWebGL, isWebGLFailure, NO_WEBGL, tryBuild } from '../src/ship/webgl.ts'

const canvas = (gives: Record<string, unknown>) => {
  const asked: string[] = []
  return { asked, getContext: (kind: string) => { asked.push(kind); return gives[kind] ?? null } }
}

test('a canvas whose getContext gives null means no WebGL, after asking for both versions', () => {
  const c = canvas({})
  assert.equal(hasWebGL(() => c), false)
  assert.deepEqual(c.asked, ['webgl2', 'webgl'])
})

test('WebGL 2, or else WebGL 1, means WebGL, and the probe lets its context go', () => {
  let lost = 0
  const ctx = { getExtension: (n: string) => (n === 'WEBGL_lose_context' ? { loseContext: () => { lost++ } } : null) }
  assert.equal(hasWebGL(() => canvas({ webgl2: ctx })), true)
  const one = canvas({ webgl: ctx })
  assert.equal(hasWebGL(() => one), true)
  assert.deepEqual(one.asked, ['webgl2', 'webgl'])
  assert.equal(lost, 2)
  assert.equal(hasWebGL(() => canvas({ webgl2: {} })), true, 'a context without the extension is still WebGL')
})

test('a canvas that throws, or none at all, means no WebGL', () => {
  assert.equal(hasWebGL(() => ({ getContext: () => { throw new Error('no GPU') } })), false)
  assert.equal(hasWebGL(() => { throw new Error('no canvas') }), false)
})

test("a renderer that throws, as three's does without a context, is a WebGL failure and not a crash", () => {
  class Renderer { constructor() { throw new Error('Error creating WebGL context.') } }
  class Engine { renderer = new Renderer() }
  const made = tryBuild(() => new Engine())
  assert.ok('failed' in made)
  assert.equal(made.failed, 'Error creating WebGL context.')
  assert.equal(isWebGLFailure(made.failed), true)
  assert.equal(isWebGLFailure(NO_WEBGL), true)
})

test("an engine that builds is handed back, and an error that isn't WebGL's is not called one", () => {
  assert.deepEqual(tryBuild(() => ({ drawn: 1 })), { engine: { drawn: 1 } })
  assert.deepEqual(tryBuild(() => { throw 'odd' }), { failed: 'odd' })
  assert.equal(isWebGLFailure("Cannot read properties of undefined (reading 'x')"), false)
})
