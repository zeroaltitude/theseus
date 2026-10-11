// The gull's voice on a Web Audio context (theseus-7mow, `gull.ts`): per note a sawtooth falling in pitch, with a slow
// wobble, through a formant band-pass for the gull's reedy throat; all under one envelope (the pass's fade in and out)
// and a stereo panner that crosses slowly. Quiet by `GULL_GAIN`. `stop` ends it at once and lets go of the master.
import { GULL_GAIN, type GullCall } from './gull.ts'

export interface GullVoice {
  sources: AudioScheduledSourceNode[]
  out: GainNode
  stop: (t: number) => void
}

export function gullBuild(c: BaseAudioContext, master: AudioNode, t: number, call: GullCall): GullVoice {
  const out = c.createGain()
  out.gain.setValueAtTime(0.0001, t)
  out.gain.linearRampToValueAtTime(GULL_GAIN, t + call.fade)
  out.gain.setValueAtTime(GULL_GAIN, t + call.secs - call.fade)
  out.gain.linearRampToValueAtTime(0.0001, t + call.secs)
  const pan = c.createStereoPanner()
  pan.pan.setValueAtTime(call.panFrom, t)
  pan.pan.linearRampToValueAtTime(call.panTo, t + call.secs)
  out.connect(pan).connect(master)
  const sources: AudioScheduledSourceNode[] = []
  for (const n of call.notes) {
    const s = t + n.at
    const o = c.createOscillator()
    o.type = 'sawtooth'
    o.frequency.setValueAtTime(n.from, s)
    o.frequency.exponentialRampToValueAtTime(n.to, s + n.dur)
    // The throat: a band-pass that follows the pitch up a little, so the cry is reedy, not a whistle.
    const bp = c.createBiquadFilter()
    bp.type = 'bandpass'
    bp.Q.value = 3
    bp.frequency.setValueAtTime(n.from * 1.1, s)
    bp.frequency.exponentialRampToValueAtTime(n.to * 1.1, s + n.dur)
    const shape = c.createGain()
    shape.gain.setValueAtTime(0.0001, s)
    shape.gain.exponentialRampToValueAtTime(1, s + 0.08)
    shape.gain.exponentialRampToValueAtTime(0.0001, s + n.dur)
    // A fast wobble in the pitch, as a gull's voice breaks.
    const wob = c.createOscillator()
    wob.frequency.value = 14
    const depth = c.createGain()
    depth.gain.value = n.from * 0.012
    wob.connect(depth).connect(o.frequency)
    wob.start(s)
    wob.stop(s + n.dur + 0.05)
    o.connect(bp).connect(shape).connect(out)
    o.start(s)
    o.stop(s + n.dur + 0.05)
    sources.push(o, wob)
  }
  return {
    sources,
    out,
    stop: (now) => {
      for (const s of sources) { try { s.stop(now) } catch { /* not started yet, or already ended */ } }
      out.disconnect()
    },
  }
}
