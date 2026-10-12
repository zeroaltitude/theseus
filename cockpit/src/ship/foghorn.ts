// The foghorn (theseus-7mow): while sound is on, rarer than the gull, a ship's foghorn far off across the water, as a
// lighthouse's diaphone sounds: a deep "BEEEEE-ohh", a long low tone that drops to a lower grunt, swelling in, dying
// away, and coming back off the water as a long, dark echo. A little louder than the gull, still under the waves. Made in
// the browser like the gull (`gull.ts`, `audio.ts`): no recording and no asset. It is heard where the gull is heard
// (sound on, and neither Calm, which reduced motion turns on, nor `?swell=0`). Pure: the gaps between horns, the shape
// of a horn and the scheduler, so a test holds them; `foghornsound.ts` plays a horn.
//
// It is not the failure `horn` cue (`audio.ts`), and must never be taken for one: that cue is close and short, a status
// signal (G2 and D3 saws, peak 0.3, up in 0.18 s, the low-pass opening to 680 Hz, a 4.6 Hz vibrato, over in 1.9 s),
// played the moment a run fails. The foghorn is longer (4 to 5 s, and its echo after), lower (about 88 Hz, then about
// 65 Hz, below the cue's lowest note), much quieter (0.12), darker (a low-pass under 500 Hz), slow to swell in (about a
// second), with a slow waver rather than a vibrato, and far off (the echo off the water): and it comes only from its own
// gaps of minutes, never from a push.
import type { GullClock } from './gull.ts'

/** What the foghorn is, beside the sea's (`SURF`) and the gull's (`GULL`). */
export const FOGHORN = {
  sound: 'a ship’s foghorn far off across the water: a long low tone dropping to a lower grunt, and its echo coming back',
  follows: 'sound on and a rolling sea (not Calm, not ?swell=0); rarer than the gull, never on a beat, never near a gull',
  source: 'synthesized in the browser (Web Audio: detuned saw and square voices, a low-pass and a formant, a dark feedback echo), src/ship/foghornsound.ts',
} as const

/** The shortest and longest gap between horns (s): rarer than the gull, never two within 8 minutes, never on a beat. */
export const HORN_GAP_MIN_S = 480 // the owner, 2026-10-12: "a fog horn even less frequently"
export const HORN_GAP_MAX_S = 1200

/** The horn's peak gain on the master the cues and the sea share: "a little louder than the seagulls" (`GULL_GAIN`,
 *  0.1), and under the idle roll's (`SURF_GAIN_ROLL`, 0.15). */
export const HORN_GAIN = 0.12

/** The seconds a horn keeps from a gull: never within this of a gull's visit, either way. */
export const HORN_CLEAR_S = 30

/** The seconds to the next horn, from a generator giving 0 to 1. */
export const hornGap = (next: () => number): number => HORN_GAP_MIN_S + (HORN_GAP_MAX_S - HORN_GAP_MIN_S) * next()

/** The milliseconds until a horn is clear of the gull, at `now` (s) with the last gull begun at `gullAt` (s): 0 when
 *  it is. */
export const hornClearMs = (now: number, gullAt: number): number => Math.max(0, HORN_CLEAR_S - (now - gullAt)) * 1000

export interface HornTone {
  /** Seconds from the horn's start, how long it holds, and its pitch (Hz). */
  at: number
  dur: number
  hz: number
}

export interface HornCall {
  /** The long tone and the lower grunt it drops to. */
  tones: [HornTone, HornTone]
  /** The drop between them, seconds: quick, as a diaphone's pitch falls when its air runs short. */
  drop: number
  /** The voices' detune, in cents either side of the pitch: a thick, beating reed, not one clean tone. */
  detune: number
  /** The swell in and the decay out, seconds. */
  swell: number
  decay: number
  /** The whole horn, seconds: the swell, the tones and the decay; its echo rings `echo.tail` past it. */
  secs: number
  /** The low-pass over the voices (Hz): far off, the horn is dark. */
  lowpass: number
  /** The formant: a band-pass (Hz) on the reed's throat, mixed in under the body. */
  formant: number
  /** The slow waver of the pitch, as wind takes the sound (Hz, and its depth in Hz). */
  waver: number
  waverDepth: number
  /** Where the far ship is (-1 left, 1 right): to one side, a little, and still. */
  pan: number
  /** The echo off the water: its delay (s), how much comes back each time round, its low-pass (Hz), its send (a share
   *  of the horn), and how long it rings past the horn (s). */
  echo: { delay: number; feedback: number; hz: number; send: number; tail: number }
}

/** One horn, from a generator: the long tone 84 to 94 Hz for 2.4 to 2.7 s, then the grunt (about three quarters of
 *  it: 62 to 70 Hz) for 0.8 s, dying away over a second; 4.2 to 4.5 s in all. The far ship off to one side. */
export function hornCall(next: () => number): HornCall {
  const hz = 84 + 10 * next()
  const long = 2.4 + 0.3 * next()
  const grunt = hz * (0.74 + 0.02 * next())
  const swell = 0.9
  const drop = 0.12
  const decay = 1.0
  // The grunt holds 0.8 s, then dies away over the decay, still on its pitch.
  const secs = long + 0.8 + decay
  const side = next() < 0.5 ? -1 : 1
  return {
    tones: [{ at: 0, dur: long, hz }, { at: long, dur: 0.8, hz: grunt }],
    drop,
    detune: 7,
    swell,
    decay,
    secs,
    lowpass: 460,
    formant: 260 + 60 * next(),
    waver: 0.35,
    waverDepth: 0.35,
    pan: side * (0.2 + 0.25 * next()),
    echo: { delay: 0.38 + 0.08 * next(), feedback: 0.45, hz: 520, send: 0.3, tail: 2.4 },
  }
}

/** The horn's gain at second `x`: up over the swell, held, and down over the decay, 0 outside. */
export function hornLevel(call: HornCall, x: number): number {
  if (x <= 0 || x >= call.secs) return 0
  return HORN_GAIN * Math.min(1, x / call.swell, (call.secs - x) / call.decay)
}

export interface HornOpts {
  /** The first horn after this many ms, not a gap (`?horn=now`); the gaps after it are the ordinary ones. */
  first?: number
  /** The ms until a horn may sound now (`hornClearMs`, clear of a gull): one due early waits that long, not a new gap. */
  clear?: () => number
}

/**
 * The horns: while `on`, one every `hornGap` seconds (the first too is a gap away: turning sound on is not a horn,
 * unless `first` asks for one sooner), each played by `play`; a horn that comes due too near a gull waits until it is
 * clear. Waiting only lengthens a gap, so no two horns are ever within `HORN_GAP_MIN_S`. Nothing is scheduled, and
 * nothing ever plays, while it is off. Returns the stop, which cancels the pending horn.
 */
export function startFoghorns(on: boolean, next: () => number, clock: GullClock, play: () => void, opts: HornOpts = {}): () => void {
  if (!on) return () => {}
  let handle: unknown = null
  let live = true
  const arm = (ms: number) => {
    handle = clock.after(() => {
      if (!live) return
      const wait = opts.clear?.() ?? 0
      if (wait > 0) { arm(wait); return }
      play()
      arm(hornGap(next) * 1000)
    }, ms)
  }
  arm(opts.first ?? hornGap(next) * 1000)
  return () => {
    live = false
    clock.cancel(handle)
  }
}
