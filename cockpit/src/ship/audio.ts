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
//   the sea (theseus-pl0x): the ambient waves while sound is on, at the sea's height (`surf.ts`), ducked under a cue
//
// Browsers start audio only from a gesture: the one place a context is made or resumed is the Sound button's click
// (`turnOn`), and sound is off on every page load (the owner's call, 2026-10-07), so the page never asks for audio
// before a click. The click's bell and the sea wait for the browser's answer: `surf` asks for a height, and nothing of
// the sea is made until the context runs.
import type { Cue } from './sound'
import {
  DUCK, DUCK_ATTACK_S, DUCK_RELEASE_S, fadeTo, NOISE_S, pinkNoise, SURF_BACKWASH_S, SURF_BODY_HZ, SURF_FADE_IN_S,
  SURF_FADE_OUT_S, SURF_FOAM, SURF_HISS, SURF_HISS_HZ, SURF_RUMBLE_HZ, SURF_WASH_HZ, surfTau, surfVoice, surfWaves, WAVES_RATE,
  type Ramp, type SurfVoice,
} from './surf.ts'

/** The sea's nodes while it plays: the gains a change of height, a cue and a fade set, and its sources. */
interface Surf {
  level: GainNode
  wash: GainNode
  duck: GainNode
  fade: GainNode
  waves: AudioBufferSourceNode
  sources: AudioScheduledSourceNode[]
  ramp: Ramp
  height: number
  voice: SurfVoice
}

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
  /** The sea's height asked for (0: silent), the sea playing, and its buffers (made once a context, when first heard). */
  private surfWant = 0
  private surfNow: Surf | null = null
  private surfBufs: { noise: AudioBuffer[]; waves: AudioBuffer } | null = null

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

  /** Make (or resume) the audio context: only from a gesture, as browsers ask. Whether it runs, once the browser has
   *  answered: the sea comes in then, never on a context the browser holds. */
  start(): Promise<boolean> {
    try {
      if (!this.live) {
        const C = window.AudioContext ?? (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext
        if (!C) return Promise.resolve(false)
        this.attach((this.live = new C()))
      }
      const live = this.live
      const runs = () => {
        if (this.live !== live || live.state !== 'running') return false
        if (!this.surfNow) this.surfAt(this.surfWant, live.currentTime)
        return true
      }
      return live.state === 'running' ? Promise.resolve(runs()) : live.resume().then(runs, () => false)
    } catch {
      return Promise.resolve(false)
    }
  }

  /** The Sound button's click turning sound on: start (this click is the gesture), and once the browser lets it run,
   *  ring the bell once, softly, so the operator hears it work, as the sea fades in under it. `still`: sound is still
   *  on when the browser answers (a second click may have turned it off). Whether it runs. */
  async turnOn(still: () => boolean): Promise<boolean> {
    const runs = await this.start()
    if (runs && still()) this.play('bell')
    return runs
  }

  play(cue: Cue) {
    const c = this.ctx
    if (!c || !this.master || (this.live && this.live.state !== 'running')) return
    this.cueAt(cue, c.currentTime + 0.02)
  }

  private cueAt(cue: Cue, t: number) {
    const c = this.ctx!
    this.duck(cue, t)
    if (cue === 'oar') this.splash(c, t)
    else if (cue === 'bell') this.bell(c, t)
    else this.horn(c, t)
  }

  /** The sea at a height (`surfHeight`; 0 silent): it comes in over a second or two, follows the height as the sea
   *  does, and goes when it is 0. Until the context runs it is only remembered: the gesture starts it. */
  surf(height: number) {
    this.surfWant = height
    if (this.ctx) this.surfAt(height, this.ctx.currentTime)
  }

  /** Whether the sea plays (or comes in), at what height: for the dev builds' window hook and the tests. */
  get surfing(): { height: number; voice: SurfVoice } | null {
    return this.surfNow && { height: this.surfNow.height, voice: this.surfNow.voice }
  }

  dispose() {
    void this.live?.close().catch(() => {})
    this.live = null
    this.ctx = null
    this.surfNow = null
    this.surfBufs = null
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

  /** The sea rendered offline, in stereo: its heights and cues at their seconds (a height at 0 s brings it in with the
   *  fade-in, as the toggle does; `full` starts it at its height instead), to listen to before it ships. */
  static async renderSea(
    plan: { secs: number; heights: [t: number, height: number][]; cues?: [t: number, cue: Cue][]; full?: boolean },
    sampleRate = 44_100,
  ): Promise<AudioBuffer> {
    const off = new OfflineAudioContext(2, Math.ceil(plan.secs * sampleRate), sampleRate)
    const a = new ShipAudio()
    a.attach(off)
    for (const [t, h] of plan.heights) {
      a.surfAt(h, t)
      if (plan.full && t === 0 && a.surfNow) { a.surfNow.ramp = { t0: 0, v0: 1, t1: 0, v1: 1 }; a.fade(a.surfNow, 0) }
    }
    for (const [t, cue] of plan.cues ?? []) a.cueAt(cue, t)
    return off.startRendering()
  }

  /** The sea's height from `t` (the context's seconds). */
  private surfAt(height: number, t: number) {
    const c = this.ctx
    if (!c || !this.master) return
    const s = this.surfNow
    if (!(height > 0)) {
      if (!s) return
      // Going: a fade from wherever it is, then its sources stop and it lets go of the speakers.
      s.ramp = fadeTo(s.ramp, t, 0, SURF_FADE_OUT_S)
      this.fade(s, t)
      for (const src of s.sources) src.stop(s.ramp.t1 + 0.05)
      this.surfNow = null
      return
    }
    const voice = surfVoice(height)
    if (!s) {
      if (this.live && this.live.state !== 'running') return
      this.surfNow = this.surfBuild(c, t, height, voice)
      return
    }
    // The sea's height moves as the sea does: up over a few seconds, down slower.
    const tau = surfTau(s.height, height)
    for (const [p, v] of [[s.level.gain, voice.gain], [s.wash.gain, voice.wash * SURF_FOAM], [s.waves.playbackRate, voice.rate]] as const) {
      p.cancelScheduledValues(t)
      p.setTargetAtTime(v, t, tau)
    }
    s.height = height
    s.voice = voice
  }

  /** Set the sea's fade gain to its ramp. */
  private fade(s: Surf, t: number) {
    const g = s.fade.gain
    g.cancelScheduledValues(t)
    g.setValueAtTime(s.ramp.v0, t)
    g.linearRampToValueAtTime(s.ramp.v1, s.ramp.t1)
  }

  /** Duck the sea under a cue, and bring it back after. */
  private duck(cue: Cue, t: number) {
    const s = this.surfNow
    if (!s) return
    const { depth, hold } = DUCK[cue]
    const g = s.duck.gain
    g.cancelScheduledValues(t)
    g.setTargetAtTime(depth, t, DUCK_ATTACK_S)
    g.setTargetAtTime(1, t + hold, DUCK_RELEASE_S)
  }

  /** The sea's buffers: two loops of pink noise (one a side) and the waves' shape. Made once a context. */
  private surfBuffers(c: BaseAudioContext) {
    if (this.surfBufs) return this.surfBufs
    const noise = NOISE_S.map((secs, ch) => {
      const n = Math.round(secs * c.sampleRate)
      const b = c.createBuffer(1, n, c.sampleRate)
      b.getChannelData(0).set(pinkNoise(0x5e4 + ch * 7919, n))
      return b
    })
    const [swell, wash] = surfWaves()
    const waves = c.createBuffer(2, swell.length, WAVES_RATE)
    waves.getChannelData(0).set(swell)
    waves.getChannelData(1).set(wash)
    return (this.surfBufs = { noise, waves })
  }

  /**
   * The sea's graph, coming in at `t`: two noise loops into a stereo pair; the body (a low-pass), the wash (a broad
   * band-pass, drifting side to side) and the hiss (a high-pass, the wash a second later); their gains driven by the
   * waves' shape on the audio thread; then the height's level, a cue's duck and the fade, into the cues' master.
   */
  private surfBuild(c: BaseAudioContext, t: number, height: number, voice: SurfVoice): Surf {
    const bufs = this.surfBuffers(c)
    const pair = c.createChannelMerger(2)
    const sources: AudioScheduledSourceNode[] = []
    bufs.noise.forEach((b, ch) => {
      const src = c.createBufferSource()
      src.buffer = b
      src.loop = true
      src.connect(pair, 0, ch)
      src.start(t, Math.random() * b.duration)
      sources.push(src)
    })
    const filter = (type: BiquadFilterType, hz: number, q: number) => {
      const f = c.createBiquadFilter()
      f.type = type
      f.frequency.value = hz
      f.Q.value = q
      return f
    }
    const gain = (v: number) => { const g = c.createGain(); g.gain.value = v; return g }
    const body = gain(0)
    const foam = gain(0)
    const hiss = gain(0)
    const wash = gain(voice.wash * SURF_FOAM)
    const level = gain(voice.gain)
    const duck = gain(1)
    const fade = gain(0)
    const drift = c.createStereoPanner()
    // The body in the middle, as a sea's weight is (one side's noise, below the rumble a speaker only buzzes on); the
    // wash and the hiss from both sides, wide.
    sources[0].connect(filter('highpass', SURF_RUMBLE_HZ, 0.6)).connect(filter('lowpass', SURF_BODY_HZ, 0.6)).connect(body).connect(level)
    pair.connect(filter('bandpass', SURF_WASH_HZ, 0.4)).connect(foam).connect(wash)
    pair.connect(filter('highpass', SURF_HISS_HZ, 0.5)).connect(hiss).connect(wash)
    wash.connect(drift).connect(level)
    level.connect(duck).connect(fade).connect(this.master!)
    // The waves: the swell into the body's gain, the break into the wash's, and a second later, faint, the hiss's.
    const waves = c.createBufferSource()
    waves.buffer = bufs.waves
    waves.loop = true
    waves.playbackRate.value = voice.rate
    const split = c.createChannelSplitter(2)
    waves.connect(split)
    split.connect(body.gain, 0)
    split.connect(foam.gain, 1)
    const back = c.createDelay(2)
    back.delayTime.value = SURF_BACKWASH_S
    split.connect(back, 1).connect(gain(SURF_HISS)).connect(hiss.gain)
    waves.start(t, Math.random() * bufs.waves.duration)
    sources.push(waves)
    // The wash drifts across, once every minute or so.
    const lfo = c.createOscillator()
    lfo.frequency.value = 0.019
    lfo.connect(gain(0.35)).connect(drift.pan)
    lfo.start(t)
    sources.push(lfo)
    // Once it has gone, it lets go of the master.
    waves.onended = () => fade.disconnect()
    const s: Surf = { level, wash, duck, fade, waves, sources, ramp: fadeTo(null, t, 1, SURF_FADE_IN_S), height, voice }
    this.fade(s, t)
    return s
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
