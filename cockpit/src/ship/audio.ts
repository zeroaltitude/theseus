// The Ship's sounds (theseus-hnof.2, the owner's C4): made in the browser with Web Audio, from oscillators and noise, with
// no recording and no third-party asset. Quiet: the master gain sits well under ordinary speech, and each sound is short.
//
//   oar:  a soft oar splash: the blade's low knock (a sine falling 150 to 85 Hz), the water (white noise through a
//         band-pass sweeping down from 1.4 kHz), and two drops (high sines rising)
//   bell: a ship's bell, struck twice ("two bells"): a bell's inharmonic partials (hum, prime, tierce, quint, nominal and
//         above, after the classic church-bell ratios) on E5, each ringing down at its own rate, the second strike softer
//   horn: a low horn: three saws (G2, a hair sharp of it, and D3) through a low-pass that opens and closes, a slow
//         vibrato, and a sag in pitch as it ends
//
// Browsers start audio only from a gesture: `start` is called from the click that turns sound on (and, after a reload,
// from the first click on the page).
import type { Cue } from './sound'

const BELL_PARTIALS: [ratio: number, amp: number, decay: number][] = [
  [0.5, 0.32, 3.4], [1, 0.62, 2.8], [1.183, 0.26, 2.0], [1.506, 0.2, 1.6], [2.0, 0.3, 1.5], [2.514, 0.11, 1.0], [2.662, 0.09, 0.85],
  [3.011, 0.07, 0.65], [4.166, 0.045, 0.42],
]

export class ShipAudio {
  /** The live context (the page's speakers), or none when rendering offline. */
  private live: AudioContext | null = null
  private ctx: BaseAudioContext | null = null
  private master: GainNode | null = null
  private noise: AudioBuffer | null = null

  /** Whether sound can play now (its context runs). */
  get running(): boolean {
    return this.live?.state === 'running'
  }

  /** The master gain, its gentle low-pass and the noise, on a context. */
  private attach(ctx: BaseAudioContext) {
    this.ctx = ctx
    this.master = ctx.createGain()
    this.master.gain.value = 0.28
    // A gentle low-pass on everything: no sound of the Ship is sharp.
    const lp = ctx.createBiquadFilter()
    lp.type = 'lowpass'
    lp.frequency.value = 7000
    this.master.connect(lp).connect(ctx.destination)
    const n = ctx.sampleRate * 2
    this.noise = ctx.createBuffer(1, n, ctx.sampleRate)
    const d = this.noise.getChannelData(0)
    let seed = 0x5eed
    for (let i = 0; i < n; i++) { seed = (seed * 1664525 + 1013904223) >>> 0; d[i] = seed / 2147483648 - 1 }
  }

  /** Make (or resume) the audio context: from a gesture, as browsers ask. Whether it runs. */
  start(): boolean {
    try {
      if (!this.live) {
        const C = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
        if (!C) return false
        this.live = new C()
        this.attach(this.live)
      }
      if (this.live.state === 'suspended') void this.live.resume()
      return this.live.state === 'running'
    } catch {
      return false
    }
  }

  play(cue: Cue) {
    const c = this.ctx
    if (!c || !this.master || (this.live && this.live.state !== 'running')) return
    const t = c.currentTime + 0.02
    if (cue === 'oar') this.splash(c, t)
    else if (cue === 'bell') this.bell(c, t)
    else this.horn(c, t)
  }

  dispose() {
    void this.live?.close().catch(() => {})
    this.live = null
    this.ctx = null
  }

  /** A cue rendered offline, with no speakers and no page open to the daemon: to listen to it, or keep it. */
  static async render(cue: Cue, sampleRate = 44_100): Promise<AudioBuffer> {
    const secs = cue === 'bell' ? 4.3 : cue === 'horn' ? 1.9 : 0.6
    const off = new OfflineAudioContext(1, Math.ceil(secs * sampleRate), sampleRate)
    const a = new ShipAudio()
    a.attach(off)
    a.play(cue)
    return off.startRendering()
  }

  private env(c: BaseAudioContext, t: number, peak: number, attack: number, decay: number): GainNode {
    const g = c.createGain()
    g.gain.setValueAtTime(0.0001, t)
    g.gain.exponentialRampToValueAtTime(peak, t + attack)
    g.gain.exponentialRampToValueAtTime(0.0001, t + attack + decay)
    return g
  }

  private splash(c: BaseAudioContext, t: number) {
    const out = this.master!
    const j = 0.92 + Math.random() * 0.16
    // The blade's knock.
    const knock = c.createOscillator()
    knock.type = 'sine'
    knock.frequency.setValueAtTime(150 * j, t)
    knock.frequency.exponentialRampToValueAtTime(85 * j, t + 0.09)
    knock.connect(this.env(c, t, 0.34, 0.008, 0.12)).connect(out)
    knock.start(t)
    knock.stop(t + 0.16)
    // The water.
    const water = c.createBufferSource()
    water.buffer = this.noise
    const bp = c.createBiquadFilter()
    bp.type = 'bandpass'
    bp.Q.value = 0.9
    bp.frequency.setValueAtTime(1400 * j, t)
    bp.frequency.exponentialRampToValueAtTime(480 * j, t + 0.36)
    water.connect(bp).connect(this.env(c, t, 0.42, 0.025, 0.38)).connect(out)
    water.start(t, Math.random() * 1.4, 0.5)
    // Two drops.
    for (const [dt, f] of [[0.12, 2200], [0.2, 3000]] as const) {
      const drop = c.createOscillator()
      drop.type = 'sine'
      drop.frequency.setValueAtTime(f * j, t + dt)
      drop.frequency.exponentialRampToValueAtTime(f * j * 1.45, t + dt + 0.025)
      drop.connect(this.env(c, t + dt, 0.05, 0.004, 0.035)).connect(out)
      drop.start(t + dt)
      drop.stop(t + dt + 0.06)
    }
  }

  private bell(c: BaseAudioContext, t: number) {
    const out = this.master!
    const f = 659.25
    for (const [strike, loud] of [[0, 1], [0.44, 0.78]] as const) {
      const s = t + strike
      for (const [ratio, amp, decay] of BELL_PARTIALS) {
        const o = c.createOscillator()
        o.type = 'sine'
        o.frequency.value = f * ratio * (1 + (Math.random() - 0.5) * 0.002)
        o.connect(this.env(c, s, amp * 0.3 * loud, 0.004, decay)).connect(out)
        o.start(s)
        o.stop(s + decay + 0.1)
      }
      // The clapper: a short knock of noise, high in the bell's metal.
      const k = c.createBufferSource()
      k.buffer = this.noise
      const hp = c.createBiquadFilter()
      hp.type = 'bandpass'
      hp.frequency.value = 3200
      hp.Q.value = 2.5
      k.connect(hp).connect(this.env(c, s, 0.05 * loud, 0.002, 0.03)).connect(out)
      k.start(s, Math.random(), 0.05)
    }
  }

  private horn(c: BaseAudioContext, t: number) {
    const out = this.master!
    const hold = 0.95
    const g = c.createGain()
    g.gain.setValueAtTime(0.0001, t)
    g.gain.exponentialRampToValueAtTime(0.3, t + 0.18)
    g.gain.setValueAtTime(0.3, t + hold)
    g.gain.exponentialRampToValueAtTime(0.0001, t + hold + 0.55)
    const lp = c.createBiquadFilter()
    lp.type = 'lowpass'
    lp.Q.value = 1.1
    lp.frequency.setValueAtTime(300, t)
    lp.frequency.linearRampToValueAtTime(680, t + 0.22)
    lp.frequency.linearRampToValueAtTime(420, t + hold + 0.5)
    lp.connect(g).connect(out)
    const vib = c.createOscillator()
    vib.frequency.value = 4.6
    const vibDepth = c.createGain()
    vibDepth.gain.value = 0.35
    vib.connect(vibDepth)
    vib.start(t)
    vib.stop(t + hold + 0.6)
    for (const [f, a] of [[98, 0.5], [98.7, 0.38], [146.8, 0.16]] as const) {
      const o = c.createOscillator()
      o.type = 'sawtooth'
      o.frequency.setValueAtTime(f, t)
      o.frequency.setValueAtTime(f, t + hold)
      o.frequency.exponentialRampToValueAtTime(f * 0.965, t + hold + 0.5)
      vibDepth.connect(o.frequency)
      const og = c.createGain()
      og.gain.value = a
      o.connect(og).connect(lp)
      o.start(t)
      o.stop(t + hold + 0.6)
    }
  }
}
