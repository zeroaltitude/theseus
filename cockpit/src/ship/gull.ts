// The seagull (theseus-7mow): while sound is on, now and then a gull cries as it passes, far off over the sea: a short
// falling call of two or three notes, panned slowly across the stereo field, faded in and out, and well under the waves.
// Made in the browser like the sea (`surf.ts`, `audio.ts`): no recording and no asset. It is heard where the sea is
// heard (sound on, and neither Calm, which reduced motion turns on, nor `?swell=0`). Pure: the gaps between visits, the
// shape of a call and the scheduler, so a test holds them; `gullsound.ts` plays a call.

/** What the gull is, beside the sea's own (`SURF`). */
export const GULL = {
  sound: 'a gull crying far off as it passes: two or three falling notes, drifting across from one ear to the other',
  follows: 'sound on and a rolling sea (not Calm, not ?swell=0); a visit every few minutes, never on a beat',
  source: 'synthesized in the browser (Web Audio: sawtooth voices through a formant band-pass), src/ship/gullsound.ts',
} as const

/** The shortest and longest gap between visits (s): never two close together, never on a fixed beat. */
export const GULL_GAP_MIN_S = 120 // the owner, 2026-10-12: "slightly more frequent" (was 150)
export const GULL_GAP_MAX_S = 300 // (was 420)

/** The gull's peak gain on the master the cues and the sea share: under the idle roll's (`SURF_GAIN_ROLL`, 0.15). */
export const GULL_GAIN = 0.1 // the owner, 2026-10-12: "slightly louder" (was 0.07); still under the roll (0.15)

/** The seconds to the next visit, from a generator giving 0 to 1. */
export const gullGap = (next: () => number): number => GULL_GAP_MIN_S + (GULL_GAP_MAX_S - GULL_GAP_MIN_S) * next()

/** A seeded generator, 0 to 1. */
export function gullRng(seed: number): () => number {
  let s = seed >>> 0 || 1
  return () => { s = (s * 1664525 + 1013904223) >>> 0; return s / 4294967296 }
}

export interface GullNote {
  /** Seconds from the call's start, and how long the note lasts. */
  at: number
  dur: number
  /** The pitch (Hz) it starts and ends on: always a fall. */
  from: number
  to: number
}

export interface GullCall {
  notes: GullNote[]
  /** The whole pass, seconds: the fade in, the cries, the fade out. */
  secs: number
  /** The pan at the start and the end of the pass (-1 left, 1 right): across, slowly. */
  panFrom: number
  panTo: number
  /** The fade in and out, seconds. */
  fade: number
}

/** One call, from a generator: two or three notes, each falling, each lower than the one before; its pan crossing from
 *  one side to the other (which side first is the gull's own). */
export function gullCall(next: () => number): GullCall {
  const count = next() < 0.5 ? 2 : 3
  let at = 1.6
  let pitch = 1900 + 300 * next()
  const notes: GullNote[] = []
  for (let i = 0; i < count; i++) {
    const dur = 0.55 + 0.35 * next()
    const to = pitch * (0.55 + 0.12 * next())
    notes.push({ at, dur, from: pitch, to })
    at += dur + 0.25 + 0.3 * next()
    pitch = to * (1.15 + 0.1 * next())
  }
  const fade = 1.6
  const side = next() < 0.5 ? -1 : 1
  return { notes, secs: at + fade, panFrom: -0.85 * side, panTo: 0.85 * side, fade }
}

/** The call's gain at second `x` of its pass: up over the fade, held, and down over the fade, 0 outside. */
export function gullLevel(call: GullCall, x: number): number {
  if (x <= 0 || x >= call.secs) return 0
  return GULL_GAIN * Math.min(1, x / call.fade, (call.secs - x) / call.fade)
}

export interface GullClock {
  /** Run `fn` after `ms`; the handle to cancel it. */
  after: (fn: () => void, ms: number) => unknown
  cancel: (handle: unknown) => void
}

/**
 * The visits: while `on`, one every `gullGap` seconds (the first too is a gap away: turning sound on is not a visit),
 * each played by `play`; nothing is scheduled, and nothing ever plays, while it is off. Returns the stop, which cancels
 * the pending visit.
 */
export function startGulls(on: boolean, next: () => number, clock: GullClock, play: () => void): () => void {
  if (!on) return () => {}
  let handle: unknown = null
  let live = true
  const arm = () => {
    handle = clock.after(() => {
      if (!live) return
      play()
      arm()
    }, gullGap(next) * 1000)
  }
  arm()
  return () => {
    live = false
    clock.cancel(handle)
  }
}
