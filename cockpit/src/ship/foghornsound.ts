// The foghorn's voice on a Web Audio context (theseus-7mow, `foghorn.ts`): four detuned saw and square voices on the
// horn's pitch, dropping together to the grunt, with a slow waver; through a dark low-pass that opens with the swell and
// closes as it dies, and a band-pass formant for the reed's throat mixed under it; all under one envelope (the swell in,
// the decay out). Then far off: a little to one side, and an echo off the water (a feedback delay through a low-pass,
// each return darker and softer), which rings on past the horn. Quiet by `HORN_GAIN`. `stop` ends it at once and lets
// go of the master.
import { HORN_GAIN, type HornCall } from './foghorn.ts'

/** The voices: a waveform, its detune (a share of the call's cents) and its level. */
const VOICES: [type: OscillatorType, cents: number, amp: number][] = [
  ['sawtooth', -1, 0.34], ['sawtooth', 1, 0.34], ['square', 0.4, 0.2], ['square', -0.6, 0.16],
]

export interface HornVoice {
  /** Every source; the last (the waver) runs to the end of the echo, so its end is the horn's. */
  sources: AudioScheduledSourceNode[]
  /** The nodes that hold on to the master: let go when the horn has gone. */
  ends: AudioNode[]
  stop: (t: number) => void
}

export function hornBuild(c: BaseAudioContext, master: AudioNode, t: number, call: HornCall): HornVoice {
  const end = t + call.secs
  const out = c.createGain()
  out.gain.setValueAtTime(0.0001, t)
  out.gain.linearRampToValueAtTime(HORN_GAIN, t + call.swell)
  out.gain.setValueAtTime(HORN_GAIN, end - call.decay)
  out.gain.linearRampToValueAtTime(0.0001, end)
  // Far off: a little to one side, the horn itself a share under its gain and the echo the rest, so the two together
  // never pass `HORN_GAIN`.
  const pan = c.createStereoPanner()
  pan.pan.value = call.pan
  const dry = c.createGain()
  dry.gain.value = 1 - call.echo.send
  out.connect(dry).connect(pan).connect(master)
  // The echo off the water: each return through the low-pass again, so darker and softer each time round.
  const delay = c.createDelay(2)
  delay.delayTime.value = call.echo.delay
  const dark = c.createBiquadFilter()
  dark.type = 'lowpass'
  dark.frequency.value = call.echo.hz
  dark.Q.value = 0.5
  const back = c.createGain()
  back.gain.value = call.echo.feedback
  const wet = c.createGain()
  wet.gain.value = call.echo.send
  out.connect(delay).connect(dark).connect(back).connect(delay)
  dark.connect(wet).connect(pan)
  // The body: dark, opening a little with the swell and closing as it dies.
  const mix = c.createGain()
  const lp = c.createBiquadFilter()
  lp.type = 'lowpass'
  lp.Q.value = 0.7
  lp.frequency.setValueAtTime(200, t)
  lp.frequency.linearRampToValueAtTime(call.lowpass, t + call.swell)
  lp.frequency.setValueAtTime(call.lowpass, end - call.decay)
  lp.frequency.linearRampToValueAtTime(220, end)
  mix.connect(lp).connect(out)
  // The throat: a gentle band-pass, under the body.
  const throat = c.createBiquadFilter()
  throat.type = 'bandpass'
  throat.frequency.value = call.formant
  throat.Q.value = 1.8
  const throatLevel = c.createGain()
  throatLevel.gain.value = 0.45
  mix.connect(throat).connect(throatLevel).connect(out)
  // The waver: slow, as wind takes a far sound; not the failure cue's quick vibrato.
  const waver = c.createOscillator()
  waver.frequency.value = call.waver
  const depth = c.createGain()
  depth.gain.value = call.waverDepth
  waver.connect(depth)
  const [long, grunt] = call.tones
  const sources: AudioScheduledSourceNode[] = []
  for (const [type, share, amp] of VOICES) {
    const k = 2 ** ((share * call.detune) / 1200)
    const o = c.createOscillator()
    o.type = type
    o.frequency.setValueAtTime(long.hz * k, t)
    o.frequency.setValueAtTime(long.hz * k, t + grunt.at)
    o.frequency.exponentialRampToValueAtTime(grunt.hz * k, t + grunt.at + call.drop)
    depth.connect(o.frequency)
    const g = c.createGain()
    g.gain.value = amp
    o.connect(g).connect(mix)
    o.start(t)
    o.stop(end + 0.05)
    sources.push(o)
  }
  waver.start(t)
  waver.stop(end + call.echo.tail)
  sources.push(waver)
  const ends: AudioNode[] = [out, wet, delay]
  return {
    sources,
    ends,
    stop: (now) => {
      for (const s of sources) { try { s.stop(now) } catch { /* not started yet, or already ended */ } }
      for (const n of ends) n.disconnect()
    },
  }
}
