// A stand-in for Web Audio, for `npm test` (node has none): the nodes `audio.ts` makes, their connections, their sources'
// starts and stops, and each param's automation, which `valueAt` works out at any second as a browser would for the
// events the sea uses (set, linear ramp, set target, cancel). It makes no sound.

type Ev = { kind: 'set' | 'linear' | 'target' | 'exp'; t: number; v: number; tau?: number }

export class FakeParam {
  value: number
  events: Ev[] = []
  constructor(v = 0) { this.value = v }
  setValueAtTime(v: number, t: number) { this.push({ kind: 'set', t, v }); return this }
  linearRampToValueAtTime(v: number, t: number) { this.push({ kind: 'linear', t, v }); return this }
  exponentialRampToValueAtTime(v: number, t: number) { this.push({ kind: 'exp', t, v }); return this }
  setTargetAtTime(v: number, t: number, tau: number) { this.push({ kind: 'target', t, v, tau }); return this }
  cancelScheduledValues(t: number) { this.events = this.events.filter((e) => e.t < t); return this }
  private push(e: Ev) {
    // In time order, a tie after the events already at that time.
    let i = this.events.length
    while (i > 0 && this.events[i - 1].t > e.t) i--
    this.events.splice(i, 0, e)
  }
  /** The param's value at second `t` (its own; any node connected into it is not counted). */
  valueAt(t: number): number {
    let cur = this.value
    let prevT = 0
    let tgt: { v: number; tau: number; from: number; at: number } | null = null
    const now = (x: number) => (tgt ? tgt.v + (tgt.from - tgt.v) * Math.exp(-(x - tgt.at) / tgt.tau) : cur)
    for (const e of this.events) {
      if (e.kind === 'linear' || e.kind === 'exp') {
        const from = now(prevT)
        if (t < e.t) {
          const f = (t - prevT) / (e.t - prevT || 1)
          return e.kind === 'linear' ? from + (e.v - from) * f : from * (e.v / from) ** f
        }
        cur = e.v; tgt = null; prevT = e.t
        continue
      }
      if (e.t > t) break
      if (e.kind === 'set') { cur = e.v; tgt = null } else tgt = { v: e.v, tau: e.tau!, from: now(e.t), at: e.t }
      prevT = e.t
    }
    return now(t)
  }
}

export class FakeNode {
  outs: { to: FakeNode | FakeParam; out?: number; in?: number }[] = []
  disconnected = false
  readonly ctx: FakeContext
  readonly kind: string
  constructor(ctx: FakeContext, kind: string) { this.ctx = ctx; this.kind = kind; ctx.nodes.push(this) }
  connect<T extends FakeNode | FakeParam>(to: T, out?: number, inp?: number): T | undefined {
    this.outs.push({ to, out, in: inp })
    return to instanceof FakeNode ? to : undefined
  }
  disconnect() { this.outs = []; this.disconnected = true }
}

export class FakeSource extends FakeNode {
  started: { t: number; offset?: number } | null = null
  stopped: number | null = null
  loop = false
  buffer: FakeBuffer | null = null
  playbackRate = new FakeParam(1)
  frequency = new FakeParam(440)
  detune = new FakeParam(0)
  type = 'sine'
  onended: (() => void) | null = null
  start(t = 0, offset?: number) { this.started = { t, offset } }
  stop(t = 0) { this.stopped = t }
}

export class FakeBuffer {
  readonly numberOfChannels: number
  readonly length: number
  readonly sampleRate: number
  private data: Float32Array[]
  constructor(ch: number, n: number, sr: number) {
    this.numberOfChannels = ch; this.length = n; this.sampleRate = sr
    this.data = Array.from({ length: ch }, () => new Float32Array(n))
  }
  get duration() { return this.length / this.sampleRate }
  getChannelData(i: number) { return this.data[i] }
}

/** The contexts made, for a test to count (a context is made only from a gesture). */
export const made: FakeContext[] = []

export class FakeContext {
  /** Whether the browser holds audio (no gesture yet): a context starts suspended, and `resume` does nothing. */
  static held = false
  /** Whether the browser answers `resume` late: a context starts suspended, and each `resume`'s answer waits here
   *  until a test lets it go (a browser's resume can take longer than any fixed wait). */
  static slow: (() => void)[] | null = null
  state: 'suspended' | 'running' | 'closed' = FakeContext.held || FakeContext.slow ? 'suspended' : 'running'
  onstatechange: (() => void) | null = null
  currentTime = 0
  sampleRate = 48_000
  nodes: FakeNode[] = []
  destination: FakeNode
  constructor() { made.push(this); this.destination = new FakeNode(this, 'destination') }
  resume() {
    const answer = () => { if (!FakeContext.held && this.state === 'suspended') { this.state = 'running'; this.onstatechange?.() } }
    const slow = FakeContext.slow
    if (slow) return new Promise<void>((done) => slow.push(() => { answer(); done() }))
    answer()
    return Promise.resolve()
  }
  close() { this.state = 'closed'; return Promise.resolve() }
  createGain() { const n = new FakeNode(this, 'gain') as FakeNode & { gain: FakeParam }; n.gain = new FakeParam(1); return n }
  createBiquadFilter() {
    const n = new FakeNode(this, 'biquad') as FakeNode & { type: string; frequency: FakeParam; Q: FakeParam }
    n.type = 'lowpass'; n.frequency = new FakeParam(350); n.Q = new FakeParam(1)
    return n
  }
  createStereoPanner() { const n = new FakeNode(this, 'panner') as FakeNode & { pan: FakeParam }; n.pan = new FakeParam(0); return n }
  createDelay() { const n = new FakeNode(this, 'delay') as FakeNode & { delayTime: FakeParam }; n.delayTime = new FakeParam(0); return n }
  createChannelMerger() { return new FakeNode(this, 'merger') }
  createChannelSplitter() { return new FakeNode(this, 'splitter') }
  createBufferSource() { return new FakeSource(this, 'buffer-source') }
  createOscillator() { return new FakeSource(this, 'oscillator') }
  createBuffer(ch: number, n: number, sr: number) { return new FakeBuffer(ch, n, sr) }
  /** The sources started and not yet stopped by `t`. */
  playing(t: number) { return this.nodes.filter((n): n is FakeSource => n instanceof FakeSource && !!n.started && (n.stopped === null || n.stopped > t)) }
}

/** Give the page a Web Audio of stand-ins: `window.AudioContext`, as `audio.ts` looks for it. */
export function installFakeAudio() {
  ;(globalThis as unknown as { window: unknown }).window = { AudioContext: FakeContext }
}
