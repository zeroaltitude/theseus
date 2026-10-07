// The ambient sea (theseus-pl0x): while sound is on, the sea is heard as well as seen, soft waves on a shore, made in the
// browser like the cues (`audio.ts`: shaped noise, no recording and no third-party asset). It follows the sea's motion
// (`sea.ts`): Live mode's idle roll is a quiet lapping, the work raises the swell a little, and it is never loud; where
// the sea is still (Calm, which reduced motion turns on, and `?swell=0`) it is silent. It plays on every page, as the
// bell and the horn do: the toggle is one for the whole cockpit, and the sea is what turning it on is for. A cue ducks
// it, so the bell and the horn are heard clearly over it. Pure: the voice for a height, the fades, the ducks, the
// waves' shape and the noise, so a test holds them; `audio.ts` plays them.
//
//   the body:  pink noise through a band of 70 to 480 Hz, the swell's weight, in the middle; its gain the waves' swell
//   the wash:  the same noise through a broad band-pass, the break running up the shore, its gain each wave's break;
//              drifting slowly from side to side
//   the hiss:  the noise above 3.8 kHz, the backwash over pebbles: the break again, a second later and faint
//
// The waves are one looping control signal (`surfWaves`), played by the audio thread into the layers' gains, so the
// page does no work for the sound while it plays: only a change of the sea's height, sound or Calm sets a new target.
import { SEA_RISE_S, SEA_ROLL, SEA_SETTLE_S, seaHeight, seaTarget } from './sea.ts'
import type { Cue } from './sound.ts'

/** What the sea is and where its sound comes from, beside the cues' table (`CUES`). */
export const SURF = {
  sound: 'soft waves on a shore: a low swell, each wave’s wash running up and back, a faint hiss of pebbles',
  follows: 'the sea’s height (tokens a minute and the turns running): a quiet lapping at the idle roll, a little more swell with work; silent where the sea is still',
  source: 'synthesized in the browser (Web Audio: pink noise through four filters, its gains driven by a looping wave shape), src/ship/audio.ts',
  pages: 'every page',
} as const

/** Seconds the sea takes to come in when sound turns on (or Calm turns off), and to go when it turns off. */
export const SURF_FADE_IN_S = 2
export const SURF_FADE_OUT_S = 1.5

/** The sea's loudness at the idle roll and at a heavy sea, on the master gain the cues share: well under the cues, and
 *  never loud. Rendered offline (R69's samples): the idle roll's waves at -44 dBFS a crest (half-second RMS), a busy
 *  sea's at -40; the bell's strike at -31 and the horn at -34, over a sea ducked 10 dB and more under them (`DUCK`). */
export const SURF_GAIN_ROLL = 0.15
export const SURF_GAIN_FULL = 0.22
/** The wash's share at the roll (the waves lap, they barely break) and at a heavy sea (each one breaks). */
export const SURF_WASH_ROLL = 0.5
/** How much faster the waves come in a heavy sea than at the roll, as the swell's clock runs faster (`seaPace`). */
export const SURF_RATE_FULL = 1.35

/** The layers' filters, the backwash's lag (s) and the hiss's share of the wash. */
export const SURF_RUMBLE_HZ = 70
export const SURF_BODY_HZ = 480
export const SURF_WASH_HZ = 950
export const SURF_HISS_HZ = 3800
export const SURF_BACKWASH_S = 1.1
export const SURF_HISS = 0.3
/** The wash's level against the body's, where the work's share (`wash`) is 1. */
export const SURF_FOAM = 1.7

export interface SurfVoice {
  /** The sea's gain (0: silent). */
  gain: number
  /** The wash's share, 0 to 1. */
  wash: number
  /** How fast the waves come: the wave shape's playback rate. */
  rate: number
}

const clamp01 = (v: number) => Math.max(0, Math.min(1, v))

/** The sea's voice for its height (`seaHeight`): silent at dead calm; from the idle roll up to a heavy sea, a little
 *  louder, breaking more and coming a little faster. */
export function surfVoice(height: number): SurfVoice {
  if (!(height > 0)) return { gain: 0, wash: 0, rate: 1 }
  const w = clamp01((height - SEA_ROLL) / (1 - SEA_ROLL))
  return {
    gain: SURF_GAIN_ROLL + (SURF_GAIN_FULL - SURF_GAIN_ROLL) * w,
    wash: SURF_WASH_ROLL + (1 - SURF_WASH_ROLL) * w,
    rate: 1 + (SURF_RATE_FULL - 1) * w,
  }
}

/** The height the sea is heard at: the sea's own (the roll, raised by the work), only while sound is on and the sea
 *  rolls (Live mode, `?swell=0` aside); 0, silent, otherwise. The present's work, on every page: under the time
 *  machine the Ship shows the moment's sea, and the sound stays with the work now. */
export function surfHeight(s: { on: boolean; calm: boolean; swell: boolean; tpm: number | null; turns: number }): number {
  if (!s.on || s.calm || !s.swell) return 0
  return seaHeight(seaTarget(s.tpm, s.turns), true)
}

/** The time constant a change of height eases with: the sea's own, up over a few seconds and down slower. */
export const surfTau = (from: number, to: number) => (to > from ? SEA_RISE_S : SEA_SETTLE_S)

/** A straight fade of the sea's own gain: from `v0` at `t0` to `v1` at `t1` (the context's seconds). */
export interface Ramp { t0: number; v0: number; t1: number; v1: number }

export const rampAt = (r: Ramp, t: number): number =>
  t <= r.t0 ? r.v0 : t >= r.t1 ? r.v1 : r.v0 + ((r.v1 - r.v0) * (t - r.t0)) / (r.t1 - r.t0)

/** A fade to `to` from wherever the last one is at `t`, over its share of `secs`: turned off halfway in, the sea goes in
 *  half the fade-out, from where it was, with no jump. */
export function fadeTo(prev: Ramp | null, t: number, to: number, secs: number): Ramp {
  const v0 = prev ? rampAt(prev, t) : 0
  return { t0: t, v0, t1: t + secs * Math.abs(to - v0), v1: to }
}

/** How a cue ducks the sea: down to `depth` of itself within a few hundredths of a second, held for `hold` seconds
 *  (the cue's body), then back over a second or so. The oar is water in water, a light dip. */
export const DUCK: Record<Cue, { depth: number; hold: number }> = {
  oar: { depth: 0.75, hold: 0.25 },
  bell: { depth: 0.3, hold: 2.4 },
  horn: { depth: 0.25, hold: 1.4 },
}
export const DUCK_ATTACK_S = 0.04
export const DUCK_RELEASE_S = 0.6

/** A seeded generator, 0 to 1. */
function rng(seed: number): () => number {
  let s = seed >>> 0 || 1
  return () => { s = (s * 1664525 + 1013904223) >>> 0; return s / 4294967296 }
}

/** Smooth noise, -1 to 1: random knots `per` seconds apart, eased between, looping over `n` samples. */
function smooth(next: () => number, n: number, sr: number, per: number): Float32Array {
  const knots = Math.max(2, Math.round(n / sr / per))
  const k = Array.from({ length: knots }, () => next() * 2 - 1)
  const out = new Float32Array(n)
  for (let i = 0; i < n; i++) {
    const x = (i / n) * knots
    const j = Math.floor(x)
    const f = 0.5 - 0.5 * Math.cos(Math.PI * (x - j))
    out[i] = k[j % knots] * (1 - f) + k[(j + 1) % knots] * f
  }
  return out
}

/** The waves' sample rate: a smooth control signal, at the lowest rate every browser makes a buffer at. */
export const WAVES_RATE = 8000
/** The wave shape's length: seven or so waves before it comes round again, at the roll's pace. */
export const WAVES_S = 47

/**
 * The waves, a seamless loop of two control signals, 0 to 1: the swell (each wave's rise and fall, over a floor of
 * water always moving) and its break (a quick wash up the shore at the crest, and a long fall back, fizzing as the
 * foam goes). The waves come every five to nine seconds at the roll's pace, each its own size; the bigger ones break
 * hardest, and some of the small ones hardly break at all.
 */
export function surfWaves(seed = 0x5ea, secs = WAVES_S, sr = WAVES_RATE): [swell: Float32Array, wash: Float32Array] {
  const next = rng(seed)
  const n = Math.round(secs * sr)
  const swell = new Float32Array(n)
  const wash = new Float32Array(n)
  // The waves' times: gaps of 5 to 9 s, scaled to fill the loop exactly, so the last wave's gap to the first is one too.
  const count = Math.max(1, Math.round(secs / 7))
  const gaps = Array.from({ length: count }, () => 5 + 4 * next())
  const scale = secs / gaps.reduce((a, b) => a + b, 0)
  let at = next() * 2
  for (const gap of gaps) {
    const size = next() < 0.18 ? 1 : 0.5 + 0.42 * next()
    const rise = 2 + 1.3 * next()
    const fall = 1.4 + 1.1 * next()
    const breaks = size > 0.66 || next() < 0.5
    const hit = (breaks ? 1 : 0.28) * size ** 1.5
    const attack = 0.6 + 0.5 * next()
    const back = 1.3 + 1.1 * next()
    // The swell: a raised-cosine rise to its crest, then an easing fall.
    const span = Math.round((rise + 6 * fall) * sr)
    for (let i = 0; i < span; i++) {
      const x = i / sr
      const v = x < rise ? 0.5 - 0.5 * Math.cos((Math.PI * x) / rise) : Math.exp(-(x - rise) / fall)
      swell[(Math.round(at * sr) + i) % n] += size * v
    }
    // The break: it starts as the crest nears, washes up quickly and falls back slowly.
    const start = at + 0.75 * rise
    const wspan = Math.round((attack + 6 * back) * sr)
    for (let i = 0; i < wspan; i++) {
      const x = i / sr
      const v = x < attack ? 0.5 - 0.5 * Math.cos((Math.PI * x) / attack) : Math.exp(-(x - attack) / back)
      wash[(Math.round(start * sr) + i) % n] += hit * v
    }
    at += gap * scale
  }
  // The water's life: the swell wavers a little, slowly; the foam fizzes as it goes.
  const waver = smooth(next, n, sr, 1.4)
  const fizz = smooth(next, n, sr, 0.09)
  let sMax = 0
  let wMax = 0
  for (let i = 0; i < n; i++) {
    swell[i] = (0.1 + swell[i]) * (1 + 0.07 * waver[i])
    wash[i] = (0.025 + wash[i]) * (1 + 0.3 * fizz[i])
    sMax = Math.max(sMax, swell[i])
    wMax = Math.max(wMax, wash[i])
  }
  for (let i = 0; i < n; i++) { swell[i] /= sMax; wash[i] /= wMax }
  return [swell, wash]
}

/** Seconds of the two noise loops, one a channel: lengths that share no short period, so the noise never repeats in
 *  step on both sides. */
export const NOISE_S: readonly [number, number] = [7.1, 8.3]

/**
 * Pink noise (a fall of 3 dB an octave, as surf's is), a seamless loop of `n` samples at RMS about 0.3: Paul Kellet's
 * three-pole filter on white noise, its end blended into its start so a loop has no click.
 */
export function pinkNoise(seed: number, n: number): Float32Array {
  const next = rng(seed)
  const blend = Math.min(2048, Math.floor(n / 4))
  const raw = new Float32Array(n + blend)
  let b0 = 0, b1 = 0, b2 = 0
  for (let i = 0; i < raw.length; i++) {
    const w = next() * 2 - 1
    b0 = 0.99765 * b0 + w * 0.099046
    b1 = 0.963 * b1 + w * 0.2965164
    b2 = 0.57 * b2 + w * 1.0526913
    raw[i] = b0 + b1 + b2 + w * 0.1848
  }
  const out = raw.slice(0, n)
  // The loop's seam: the first samples fade from the run past the end into their own, equal in power.
  for (let i = 0; i < blend; i++) {
    const f = (i + 0.5) / blend
    out[i] = raw[i] * Math.sin((Math.PI / 2) * f) + raw[n + i] * Math.cos((Math.PI / 2) * f)
  }
  let sq = 0
  for (let i = 0; i < n; i++) sq += out[i] * out[i]
  const g = 0.3 / Math.sqrt(sq / n || 1)
  for (let i = 0; i < n; i++) out[i] *= g
  return out
}
