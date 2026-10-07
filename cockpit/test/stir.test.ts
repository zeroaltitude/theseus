// The page's endless decorations go still while nothing changes (`src/lib/stir.ts`, theseus-jgme), run by `npm test`
// on a fake clock; and the waiting lantern stays still after its one swell (the owner's F12, item 6).
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import { Stir, STIR_CLASS, STIR_MS, type StirClock } from '../src/lib/stir.ts'
import { BEACON_VERT } from '../src/ship/shaders.ts'
import { motion, motionsNow, paceOf, type MotionState } from '../src/ship/motion.ts'

class FakeClock implements StirClock {
  t = 0
  private seq = 0
  private timers = new Map<number, { at: number; cb: () => void }>()
  timer(cb: () => void, ms: number) { const id = ++this.seq; this.timers.set(id, { at: this.t + ms, cb }); return id }
  cancelTimer(id: number) { this.timers.delete(id) }
  run(until: number) {
    for (;;) {
      let next: { at: number; id: number } | null = null
      for (const [id, x] of this.timers) if (!next || x.at < next.at) next = { at: x.at, id }
      if (!next || next.at > until) { this.t = until; return }
      this.t = next.at
      const cb = this.timers.get(next.id)!.cb
      this.timers.delete(next.id)
      cb()
    }
  }
  get pending() { return this.timers.size }
}

test('nothing changing, the decorations stand still: no class, no timer', () => {
  const clock = new FakeClock()
  const sets: boolean[] = []
  const s = new Stir(clock, (on) => sets.push(on))
  clock.run(60_000)
  assert.equal(s.stirred, false)
  assert.deepEqual(sets, [])
  assert.equal(clock.pending, 0)
})

test('a change stirs them for one sweep and a little, and a stream of changes keeps them running', () => {
  assert.ok(STIR_MS >= 3200 && STIR_MS <= 6000, `one sweep (3.2 s) and a little: ${STIR_MS}`)
  const clock = new FakeClock()
  const sets: boolean[] = []
  const s = new Stir(clock, (on) => sets.push(on))
  s.poke()
  assert.equal(s.stirred, true)
  clock.run(STIR_MS - 1)
  assert.equal(s.stirred, true)
  clock.run(STIR_MS)
  assert.equal(s.stirred, false)
  assert.deepEqual(sets, [true, false])
  // A turn streaming: a push every half second for 20 s, then nothing. One on, one off, STIR_MS after the last.
  sets.length = 0
  for (let t = 10_000; t <= 30_000; t += 500) { clock.run(t); s.poke() }
  clock.run(30_000 + STIR_MS - 1)
  assert.equal(s.stirred, true)
  clock.run(30_000 + STIR_MS)
  assert.deepEqual(sets, [true, false])
  assert.equal(clock.pending, 0, 'still, nothing waits')
  s.poke()
  s.dispose()
  assert.equal(clock.pending, 0)
})

test('the page wears the class while they run, and the style stills them without it', () => {
  assert.equal(STIR_CLASS, 'stirred')
  const css = readFileSync(new URL('../src/index.css', import.meta.url), 'utf8')
  assert.match(css, /html:not\(\.stirred\) \.live-sweep, html:not\(\.stirred\) \.animate-ping, html:not\(\.stirred\) \.animate-pulse-soft \{ animation: none !important; \}/)
  assert.match(css, /html:not\(\.stirred\) \.animate-ping \{ display: none; \}/)
  // Calm still stills them, stirred or not.
  assert.match(css, /html\.calm \.live-sweep, html\.calm \.animate-ping, html\.calm \.animate-pulse-soft/)
  const shell = readFileSync(new URL('../src/components/Shell.tsx', import.meta.url), 'utf8')
  assert.match(shell, /export function Shell\(\) \{\s+useStir\(\)/)
  assert.match(shell, /client\.onNotify\(\(\) => s\.poke\(\)\)/)
  assert.match(shell, /onNewRows\(\(\) => s\.poke\(\)\)/)
  assert.match(shell, /useEffect\(\(\) => \{ stir\.current\?\.poke\(\) \}, \[status\]\)/)
  assert.match(shell, /document\.documentElement\.classList\.toggle\(STIR_CLASS, on\)/)
})

test('the waiting lantern swells once as it lights, then stands lit and still', () => {
  // The table: a one-off of 1.5 s, at the display's pace; nothing lasting.
  const w = motion('waiting')
  assert.equal(w.pace, 'display')
  assert.equal(w.secs, 1.5)
  assert.match(w.moves, /swelling once; then it stands lit/)
  const idle: MotionState = {
    calm: false, camera: false, settling: false, t: 0, until: {},
    rowing: false, working: false, streaming: false, gears: false, tethers: false, currents: false, sea: false, roll: false,
  }
  // A lantern lit at 100 s: it plays its swell, and from 101.5 s on it asks for no frame.
  assert.deepEqual(motionsNow({ ...idle, t: 101, until: { waiting: 101.5 } }), ['waiting'])
  assert.deepEqual(motionsNow({ ...idle, t: 101.5, until: { waiting: 101.5 } }), [])
  assert.equal(paceOf(motionsNow({ ...idle, t: 3600, until: { waiting: 101.5 } }), 15).fps, 0)
  // The shader: the lantern's branch moves with time only through its one swell, gated to its first 1.5 s and Live.
  const src = BEACON_VERT
  const branch = src.slice(src.indexOf('// The lantern'), src.indexOf('} else if (b.y > 2.5)'))
  assert.ok(branch.length > 0, 'the lantern has its branch')
  const timed = branch.split('\n').filter((l) => /\b(uTime|lit)\b/.test(l) && !l.trim().startsWith('//'))
  assert.deepEqual(timed.map((l) => l.trim()), [
    'float lit = d.x > 0.0 ? uTime - d.x : 1e6; // motion: waiting',
    'if (lit < 1.5 && uCalm < 0.5) pulse = smoothstep(0.0, 0.25, lit) * (1.0 + 0.9 * exp(-lit * 2.6));',
  ])
  assert.ok(!/\bsin\(|\bcos\(|fract\(/.test(branch), 'no breathing: nothing periodic in the lantern')
  // The engine lights it once, as it turns to a lantern while the page watches; one lit before stands lit.
  const engine = readFileSync(new URL('../src/ship/engine.ts', import.meta.url), 'utf8')
  assert.match(engine, /else if \(old && rigWas !== undefined && rigWas !== 'lantern' && !this\.lanternAt\.has\(v\.id\)\) this\.lanternAt\.set\(v\.id, now\)/)
  assert.match(engine, /if \(lit\) this\.play\('waiting', lit \+ 1\.5\)/)
})
