// The sonar ping (theseus-mbulo): now and then, while the Ship is shown, a submarine's ping, as in the films: a clean
// bright tone with a long decay and a soft echo (`sonarsound.ts`), and at the same instant a thin ring growing from the
// middle of the Ship to beyond its edges (`Sonar.tsx`). The ring shows with Sound on or off (a visual heartbeat) and not
// in Calm (which reduced motion turns on); the sound plays only with Sound on, Calm or not. A hidden tab has neither,
// and nothing waits for its return. Pure: the gaps, the shape of the ping and its ring, the scheduler, the room it
// leaves the gull, and the ring's own life; a test holds them.
import { create } from 'zustand'
import type { GullClock } from './gull.ts'

/** What the ping is, beside the sea's and the gull's. */
export const SONAR = {
  sound: 'a submarine’s sonar ping: a bright sine tone, a long decay, a soft echo and a faint, lower return',
  follows: 'the Ship shown; a ping every few minutes, never on a beat, never with a gull; the ring in Live, the sound with Sound on',
  source: 'synthesized in the browser (Web Audio: sine voices, a lowpassed feedback delay), src/ship/sonarsound.ts',
} as const

/** The shortest and longest gap between pings (s): a few minutes, never two close together, never on a fixed beat. */
export const SONAR_GAP_MIN_S = 180
export const SONAR_GAP_MAX_S = 480

/** The ping's peak gain on the master the cues and the sea share: under the idle roll's (`SURF_GAIN_ROLL`, 0.15). */
export const SONAR_GAIN = 0.1

/** The seconds to the next ping, from a generator giving 0 to 1. */
export const sonarGap = (next: () => number): number => SONAR_GAP_MIN_S + (SONAR_GAP_MAX_S - SONAR_GAP_MIN_S) * next()

export interface PingVoice {
  /** The tone (Hz), its sharp attack and long decay (s). */
  hz: number
  attack: number
  decay: number
  /** The echo: its delay (s), how much comes back each time round, its low-pass (Hz) and its level against the ping. */
  echoDelay: number
  echoFeedback: number
  echoHz: number
  echoLevel: number
  /** The return, "pong": later, lower, fainter and shorter. */
  pong: { at: number; hz: number; level: number; decay: number }
}

/** One ping, from a generator: the tone 1.2 to 1.6 kHz, decaying over 1.5 to 2.5 s. */
export function pingVoice(next: () => number): PingVoice {
  return {
    hz: 1200 + 400 * next(),
    attack: 0.006,
    decay: 1.5 + next(),
    echoDelay: 0.42 + 0.1 * next(),
    echoFeedback: 0.4,
    echoHz: 2400,
    echoLevel: 0.35,
    pong: { at: 1.1 + 0.4 * next(), hz: 780 + 120 * next(), level: 0.18, decay: 1.2 },
  }
}

/** How long a ping is heard, seconds: the tone and its echoes, or the return, whichever ends last. */
export function pingSecs(v: PingVoice): number {
  return Math.max(v.attack + v.decay + v.echoDelay * 4, v.pong.at + v.pong.decay) + 0.1
}

/** The ring: how long it takes to grow (s), when the second, fainter ring follows (s), its stroke (px; the same at any
 *  size), its first and last opacity, and its colour (the cockpit's live cyan, which reads on the dark sea). */
export const RING = { secs: 2.2, trailAt: 0.45, trailOpacity: 0.4, stroke: 1.5, from: 0.7, color: 'var(--color-live)' } as const

/** The diameter that takes a ring from the middle to beyond the corners of a `w` by `h` box. */
export const ringDiameter = (w: number, h: number): number => Math.ceil(Math.hypot(w, h)) + 2

/** How long the ring stays mounted (ms): its growth, its trailing ring, and a moment over. */
export const RING_LIFE_MS = Math.ceil((RING.secs + RING.trailAt) * 1000) + 100

/** What a ping does, given Sound, Calm and whether the tab is shown: the ring, the sound, both or neither. */
export function pingWhat(o: { soundOn: boolean; calm: boolean; hidden: boolean }): { ring: boolean; sound: boolean } {
  if (o.hidden) return { ring: false, sound: false }
  return { ring: !o.calm, sound: o.soundOn }
}

/** The moments the Ship's voices claim, so a ping and a gull are never heard at once. */
export class Room {
  private until = 0
  /** Milliseconds from `now` until the room is free: 0 when it is. */
  wait(now: number): number {
    return Math.max(0, this.until - now)
  }
  /** Claim the room for `ms` from `now`: false (and nothing claimed) when it is not free. */
  claim(now: number, ms: number): boolean {
    if (this.wait(now) > 0) return false
    this.until = now + ms
    return true
  }
}

/** The one room the ping and the gull share. */
export const room = new Room()

/** A breath between the end of one voice and the start of the next (ms). */
export const ROOM_PAD_MS = 1500

export interface SonarSite {
  next: () => number
  clock: GullClock
  /** The page's clock (ms). */
  now: () => number
  /** Whether the tab is shown. */
  shown: () => boolean
  /** A ping, once the room is claimed for its length. */
  play: (voice: PingVoice) => void
  /** The room (`room`); a test gives its own. */
  room?: Room
}

/**
 * The pings: one every `sonarGap` seconds, the first too a gap away (opening the Ship is not a ping). A ping that comes
 * due with the tab hidden is dropped, not kept for its return; one that comes due while a gull is in the air waits for
 * it to end (and a pad). Returns the stop, which cancels the pending ping.
 */
export function startSonar(s: SonarSite): () => void {
  const r = s.room ?? room
  let handle: unknown = null
  let live = true
  const arm = (ms: number) => {
    handle = s.clock.after(() => {
      if (!live) return
      if (!s.shown()) { arm(sonarGap(s.next) * 1000); return }
      const wait = r.wait(s.now())
      if (wait > 0) { arm(wait + ROOM_PAD_MS); return }
      const voice = pingVoice(s.next)
      r.claim(s.now(), pingSecs(voice) * 1000 + ROOM_PAD_MS)
      s.play(voice)
      arm(sonarGap(s.next) * 1000)
    }, ms)
  }
  arm(sonarGap(s.next) * 1000)
  return () => {
    live = false
    s.clock.cancel(handle)
  }
}

/** The ring on screen: a number a ping sets (a new one restarts the ring), and 0 when it has gone. It removes itself
 *  after `RING_LIFE_MS` on the clock it is given; while 0 nothing is drawn or timed. */
export const useRing = create<{ id: number }>(() => ({ id: 0 }))

let ringTimer: unknown = null
export function showRing(clock: Pick<GullClock, 'after' | 'cancel'> = timers): number {
  if (ringTimer !== null) clock.cancel(ringTimer)
  const id = useRing.getState().id + 1
  useRing.setState({ id })
  ringTimer = clock.after(() => { ringTimer = null; useRing.setState({ id: 0 }) }, RING_LIFE_MS)
  return id
}

export function hideRing(clock: Pick<GullClock, 'after' | 'cancel'> = timers) {
  if (ringTimer !== null) clock.cancel(ringTimer)
  ringTimer = null
  useRing.setState({ id: 0 })
}

const timers: GullClock = { after: (f, ms) => setTimeout(f, ms), cancel: (h) => clearTimeout(h as number) }
