// The sonar ping (`src/ship/sonar.ts`, its voice `sonarsound.ts`, its ring `Sonar.tsx`, theseus-mbulo), run by `npm test`:
// its gaps are random and seeded; it keeps clear of the gull; nothing is audible with Sound off; the ring shows, and
// removes itself; Calm shows no ring and keeps the sound; a hidden tab has neither; and every node is let go on stop.
import { test } from 'node:test'
import assert from 'node:assert/strict'
import { readFileSync } from 'node:fs'
import {
  hideRing, pingSecs, pingVoice, pingWhat, RING, RING_LIFE_MS, ringDiameter, Room, ROOM_PAD_MS, showRing, SONAR_GAIN,
  SONAR_GAP_MAX_S, SONAR_GAP_MIN_S, sonarGap, startSonar, useRing, type PingVoice,
} from '../src/ship/sonar.ts'
import { GULL_GAP_MIN_S, gullRng, type GullClock } from '../src/ship/gull.ts'
import { SURF_GAIN_ROLL } from '../src/ship/surf.ts'
import { ShipAudio } from '../src/ship/audio.ts'
import { FakeContext, FakeSource, installFakeAudio, made } from './fakeaudio.ts'

function fakeClock() {
  const q: { id: number; at: number; fn: () => void }[] = []
  let id = 0
  let now = 0
  const clock: GullClock = {
    after: (fn, ms) => { const h = ++id; q.push({ id: h, at: now + ms, fn }); return h },
    cancel: (h) => { const i = q.findIndex((e) => e.id === h); if (i >= 0) q.splice(i, 1) },
  }
  const fire = () => {
    q.sort((a, b) => a.at - b.at)
    const e = q.shift()
    if (!e) return null
    now = e.at
    e.fn()
    return now
  }
  return { clock, fire, pending: () => q.length, now: () => now }
}

test('the gaps: a few minutes, random, seeded, never two close together and never on a beat', () => {
  const next = gullRng(7)
  const gaps = Array.from({ length: 200 }, () => sonarGap(next))
  for (const g of gaps) assert.ok(g >= SONAR_GAP_MIN_S && g <= SONAR_GAP_MAX_S, `gap ${g}`)
  assert.ok(SONAR_GAP_MIN_S >= 120 && SONAR_GAP_MAX_S <= 600, 'a few minutes')
  assert.ok(new Set(gaps.map((g) => Math.round(g))).size > 100, 'a spread of gaps, not a beat')
  assert.deepEqual(Array.from({ length: 5 }, ((n) => () => sonarGap(n))(gullRng(7))), gaps.slice(0, 5), 'seeded: the same gaps again')
  assert.notDeepEqual(gaps.slice(0, 5), Array.from({ length: 5 }, ((n) => () => sonarGap(n))(gullRng(8))), 'another seed, other gaps')
  assert.equal(sonarGap(() => 0), SONAR_GAP_MIN_S)
  assert.equal(sonarGap(() => 1), SONAR_GAP_MAX_S)
})

test('the ping: a bright sine of 1.2 to 1.6 kHz, a sharp attack, a long decay, an echo that fades; under the waves', () => {
  for (let seed = 1; seed <= 50; seed++) {
    const v = pingVoice(gullRng(seed))
    assert.ok(v.hz >= 1200 && v.hz <= 1600, `${v.hz} Hz`)
    assert.ok(v.attack <= 0.01, 'a sharp attack')
    assert.ok(v.decay >= 1.5 && v.decay <= 2.5, `decay ${v.decay}`)
    assert.ok(v.echoFeedback < 0.6 && v.echoLevel < 0.5, 'the echo is softer than the ping and dies away')
    assert.ok(v.pong.hz < v.hz && v.pong.level < v.echoLevel && v.pong.at > v.echoDelay, 'the return is later, lower and fainter')
    assert.ok(pingSecs(v) > v.attack + v.decay, 'heard past its tone, to the echoes')
  }
  assert.ok(SONAR_GAIN < SURF_GAIN_ROLL, 'quieter than the waves')
})

function site(opts: { shown?: () => boolean; room?: Room } = {}) {
  const c = fakeClock()
  const played: { at: number; v: PingVoice }[] = []
  const stop = startSonar({
    next: gullRng(1), clock: c.clock, now: c.now, shown: opts.shown ?? (() => true), room: opts.room ?? new Room(),
    play: (v) => played.push({ at: c.now() / 1000, v }),
  })
  return { c, played, stop }
}

test('the scheduler: the first ping a gap away, every one a random gap after, and a stop cancels the pending one', () => {
  const s = site()
  assert.equal(s.played.length, 0, 'opening the Ship is not a ping')
  assert.equal(s.c.pending(), 1)
  let last = 0
  for (let i = 0; i < 5; i++) {
    s.c.fire()
    const at = s.played[i].at
    assert.ok(at - last >= SONAR_GAP_MIN_S - 1e-6 && at - last <= SONAR_GAP_MAX_S + 1e-6, `gap ${at - last}`)
    last = at
  }
  s.stop()
  assert.equal(s.c.pending(), 0)
  assert.equal(s.c.fire(), null)
  assert.equal(s.played.length, 5, 'none plays after the stop')
})

test('a hidden tab: a ping that comes due is dropped, not kept for the return', () => {
  let shown = false
  const s = site({ shown: () => shown })
  s.c.fire()
  s.c.fire()
  assert.equal(s.played.length, 0, 'nothing while hidden, and no queue: one timer pending')
  assert.equal(s.c.pending(), 1)
  shown = true
  s.c.fire()
  assert.equal(s.played.length, 1, 'one ping once shown again, not a burst')
  assert.equal(s.c.pending(), 1)
})

test('never with a gull: a ping that comes due in a gull’s pass waits for it, and a gull is let pass while a ping rings', () => {
  const room = new Room()
  const s = site({ room })
  assert.equal(room.claim(0, 400_000), true, 'a gull is in the air for a long pass')
  assert.equal(room.claim(1000, 5000), false, 'a second voice is not let in')
  s.c.fire()
  assert.equal(s.played.length, 0, 'due in the pass: not played')
  assert.ok(s.c.pending() === 1)
  s.c.fire()
  assert.equal(s.played.length, 1)
  assert.ok(s.played[0].at * 1000 >= 400_000 + ROOM_PAD_MS, `after the pass and its pad (${s.played[0].at} s)`)
  assert.ok(room.wait(s.c.now()) > 0, 'the ping holds the room while it rings')
  assert.equal(room.claim(s.c.now() + 1000, 5000), false, 'so a gull is not heard with it')
  assert.ok(GULL_GAP_MIN_S >= 120)
})

test('what a ping does: the ring in Live, the sound with Sound on, nothing in a hidden tab', () => {
  assert.deepEqual(pingWhat({ soundOn: false, calm: false, hidden: false }), { ring: true, sound: false }, 'the ring shows with Sound off')
  assert.deepEqual(pingWhat({ soundOn: true, calm: false, hidden: false }), { ring: true, sound: true })
  assert.deepEqual(pingWhat({ soundOn: true, calm: true, hidden: false }), { ring: false, sound: true }, 'Calm (reduced motion): the sound alone')
  assert.deepEqual(pingWhat({ soundOn: false, calm: true, hidden: false }), { ring: false, sound: false })
  assert.deepEqual(pingWhat({ soundOn: true, calm: false, hidden: true }), { ring: false, sound: false })
})

test('the ring: it shows, restarts on a new ping, removes itself, and reaches past the corners', () => {
  const c = fakeClock()
  hideRing(c.clock)
  assert.equal(useRing.getState().id, 0, 'idle: nothing mounted, no timer')
  assert.equal(c.pending(), 0)
  const a = showRing(c.clock)
  assert.ok(a > 0 && useRing.getState().id === a)
  assert.equal(c.pending(), 1)
  const b = showRing(c.clock)
  assert.notEqual(b, a, 'a new ping restarts the ring')
  assert.equal(c.pending(), 1, 'one timer, the old one cancelled')
  assert.equal(c.fire(), RING_LIFE_MS)
  assert.equal(useRing.getState().id, 0, 'it removes itself')
  assert.equal(c.pending(), 0)
  assert.ok(RING_LIFE_MS >= (RING.secs + RING.trailAt) * 1000, 'it lives as long as its trailing ring grows')
  assert.ok(RING.secs >= 1.8 && RING.secs <= 2.4, 'about two seconds')
  assert.ok(ringDiameter(1600, 900) >= Math.hypot(1600, 900), 'its edge passes the corners')
})

test('the ring in the page: after the labels and before the panels, no pointer, nothing in Calm, drawn only while it lives', () => {
  const ship = readFileSync(new URL('../src/views/Ship.tsx', import.meta.url), 'utf8')
  const ring = ship.indexOf('<SonarRing />')
  assert.ok(ring > ship.indexOf('className="ship-labels') && ring < ship.indexOf('<Cartouche'), 'above the scene, below the panels')
  const tsx = readFileSync(new URL('../src/ship/Sonar.tsx', import.meta.url), 'utf8')
  assert.match(tsx, /if \(!id\) return null/, 'nothing while idle')
  assert.match(tsx, /pointer-events-none/)
  const css = readFileSync(new URL('../src/ship/ship.css', import.meta.url), 'utf8')
  assert.match(css, /html\.calm \.sonar-box[^}]*display: none/, 'no ring in Calm')
  assert.match(css, /prefers-reduced-motion: reduce\) \{ \.sonar-box \{ display: none/, 'no ring under reduced motion')
  const hook = readFileSync(new URL('../src/ship/useShipSound.ts', import.meta.url), 'utf8')
  assert.match(hook, /if \(what\.ring\) showRing\(\)\s+if \(what\.sound\) a\.ping\(voice\)/, 'the ring and the sound at one instant, each by its own rule')
  assert.match(hook, /return \(\) => \{ stop\(\); hideRing\(\); a\.pingStop\(\) \}/, 'torn down with the page')
  assert.match(hook, /room\.claim\(Date\.now\(\), call\.secs \* 1000 \+ ROOM_PAD_MS\)\) a\.gull\(call\)/, 'the gull claims the room')
})

test('the graph: a ping makes a tone, an echo and a return on the master, and stopping lets go of every one', () => {
  FakeContext.held = false
  FakeContext.slow = null
  installFakeAudio()
  made.length = 0
  const a = new ShipAudio()
  return a.start().then(() => {
    const ctx = made[0]
    const v = pingVoice(gullRng(3))
    assert.equal(a.ping(v), true)
    assert.equal(a.pingsHeard, 1)
    const oscs = ctx.nodes.filter((n): n is FakeSource => n instanceof FakeSource && n.kind === 'oscillator')
    assert.equal(oscs.length, 3, 'a ping, its return, and the timer that outlasts the echoes')
    assert.ok(oscs.every((s) => s.type === 'sine' || s.frequency.value > 0))
    assert.ok(oscs.some((s) => Math.abs(s.frequency.value - v.hz) < 1e-9), 'the tone')
    assert.ok(ctx.nodes.some((n) => n.kind === 'delay'), 'an echo')
    assert.ok(ctx.nodes.some((n) => n.kind === 'biquad' && (n as unknown as { type: string }).type === 'lowpass' && ctx.nodes.some((d) => d.kind === 'delay')), 'lowpassed')
    assert.ok(oscs.every((s) => s.started), 'all sounding')
    a.pingStop()
    assert.equal(a.pingsHeard, 0)
    assert.ok(oscs.every((s) => s.stopped !== null && s.stopped <= ctx.currentTime), 'every source stopped')
    assert.ok(ctx.nodes.filter((n) => n.kind === 'gain' && n.disconnected).length > 0, 'its gains let go of the master')
    // A page leaving (dispose) ends a ping in the air.
    assert.equal(a.ping(pingVoice(gullRng(4))), true)
    a.dispose()
    assert.equal(a.pingsHeard, 0)
  })
})

test('a ping plays only where the sound runs: no context, nothing; and a ping that ends lets go of itself', () => {
  const a = new ShipAudio()
  assert.equal(a.ping(pingVoice(gullRng(1))), false)
  assert.equal(a.pingsHeard, 0)
  installFakeAudio()
  made.length = 0
  const b = new ShipAudio()
  return b.start().then(() => {
    b.ping(pingVoice(gullRng(2)))
    const timer = made[0].nodes.filter((n): n is FakeSource => n instanceof FakeSource).at(-1)!
    timer.onended?.()
    assert.equal(b.pingsHeard, 0, 'ended: forgotten')
  })
})

test('Sound off: the page schedules and plays no sound (the ring alone), and the toggle going off ends a ping', () => {
  const hook = readFileSync(new URL('../src/ship/useShipSound.ts', import.meta.url), 'utf8')
  assert.match(hook, /soundOn: useSound\.getState\(\)\.on/)
  assert.match(hook, /useEffect\(\(\) => \{ if \(!on\) audio\?\.pingStop\(\) \}, \[on\]\)/)
  assert.match(readFileSync(new URL('../src/components/Shell.tsx', import.meta.url), 'utf8'), /useSonar\(onShip\)/)
})
