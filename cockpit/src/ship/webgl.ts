// Can this browser draw the Ship? (theseus-9k53) The Ship draws with WebGL, through three.js. A browser with WebGL
// off (hardware acceleration disabled, a blocklisted GPU, a GPU process sandboxed off) has no context to give, and
// three's renderer throws; that error once took the Ship's whole route, the cockpit's landing view. These parts are
// pure, so `npm test` runs them on a stubbed canvas; `NoWebGL.tsx` draws what shows in the Ship's place.

/** The one method of a canvas the probe calls. */
export interface ProbeCanvas { getContext(kind: string): unknown }

type ProbeContext = { getExtension?: (name: string) => { loseContext?: () => void } | null }

/** The reason the probe gives. */
export const NO_WEBGL = 'This browser gives the page no WebGL context.'

/** Whether a canvas gives a WebGL context: WebGL 2 first, then WebGL 1. The probe lets its context go at once, since
 *  a browser keeps only a few alive. A canvas that throws counts as no WebGL. */
export function hasWebGL(canvas: () => ProbeCanvas): boolean {
  try {
    const c = canvas()
    const gl = (c.getContext('webgl2') ?? c.getContext('webgl')) as ProbeContext | null
    if (!gl) return false
    gl.getExtension?.('WEBGL_lose_context')?.loseContext?.()
    return true
  } catch {
    return false
  }
}

/** Builds the engine, or says why it couldn't: a probe can pass and the renderer's context still fail. */
export function tryBuild<T>(build: () => T): { engine: T } | { failed: string } {
  try {
    return { engine: build() }
  } catch (e) {
    return { failed: e instanceof Error ? e.message : String(e) }
  }
}

/** Whether a failure is WebGL's (three's renderer names it), so the fallback says how to turn WebGL back on, not
 *  only what broke. */
export const isWebGLFailure = (reason: string) => /webgl/i.test(reason)
