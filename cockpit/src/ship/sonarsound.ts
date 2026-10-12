// The sonar ping's voice on a Web Audio context (theseus-mbulo, `sonar.ts`): a sine with a sharp attack and a long
// exponential decay, a soft echo (a feedback delay through a low-pass, so each return is quieter and duller), and a
// faint, lower, shorter "pong" a moment later. All on one gain, into the master the sea and the cues share, so the
// volume and the mute apply. `stop` ends it at once and lets go of the master.
import { pingSecs, SONAR_GAIN, type PingVoice } from './sonar.ts'

export interface PingNodes {
  sources: AudioScheduledSourceNode[]
  out: GainNode
  /** Every node it made that holds a connection, to let go of. */
  stop: (t: number) => void
}

export function pingBuild(c: BaseAudioContext, master: AudioNode, t: number, v: PingVoice, done: () => void = () => {}): PingNodes {
  const out = c.createGain()
  out.gain.value = SONAR_GAIN
  out.connect(master)
  const sources: AudioScheduledSourceNode[] = []
  const tone = (at: number, hz: number, level: number, attack: number, decay: number, into: AudioNode) => {
    const o = c.createOscillator()
    o.type = 'sine'
    o.frequency.value = hz
    const g = c.createGain()
    g.gain.setValueAtTime(0.0001, at)
    g.gain.exponentialRampToValueAtTime(level, at + attack)
    g.gain.exponentialRampToValueAtTime(0.0001, at + attack + decay)
    o.connect(g).connect(into)
    o.start(at)
    o.stop(at + attack + decay + 0.05)
    sources.push(o)
  }
  // The echo: the ping into a delay whose feedback runs through a low-pass; the dry ping goes straight out.
  const send = c.createGain()
  send.gain.value = v.echoLevel
  const delay = c.createDelay(2)
  delay.delayTime.value = v.echoDelay
  const lp = c.createBiquadFilter()
  lp.type = 'lowpass'
  lp.frequency.value = v.echoHz
  const back = c.createGain()
  back.gain.value = v.echoFeedback
  send.connect(delay).connect(lp)
  lp.connect(back).connect(delay)
  lp.connect(out)
  const bus = c.createGain()
  bus.connect(out)
  bus.connect(send)
  tone(t, v.hz, 1, v.attack, v.decay, bus)
  tone(t + v.pong.at, v.pong.hz, v.pong.level, v.attack * 3, v.pong.decay, bus)
  const nodes = [send, delay, lp, back, bus]
  // An unconnected timer that outlasts the last echo: its end lets go of the master (a source's end would cut the tail).
  const timer = c.createOscillator()
  timer.start(t)
  timer.stop(t + pingSecs(v))
  sources.push(timer)
  timer.onended = () => { for (const n of nodes) n.disconnect(); out.disconnect(); done() }
  return {
    sources,
    out,
    stop: (now) => {
      for (const s of sources) { try { s.stop(now) } catch { /* not started yet, or already ended */ } }
      for (const n of nodes) n.disconnect()
      out.disconnect()
    },
  }
}
