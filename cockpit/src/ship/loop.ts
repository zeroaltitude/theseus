// The Ship's loop: when a frame is drawn (theseus-wp2d). The engine (`engine.ts`) draws; this decides when. It is
// pure (no DOM, no three.js), so `npm test` drives it on a fake clock.
//
// - Something moves (the camera, a vessel settling, a flare, a sail, a lantern, a gear, a stream, a current): a frame
//   at every display frame, the full rate.
// - Nothing else moves, in Live mode: the sea's swell alone, about `IDLE_FPS` frames a second. Each wait is a timer,
//   then a display frame, so the swell lands on the display's beat.
// - Nothing moves and no swell (Calm, or `?swell=0`): no frame until something changes.
// - A hidden tab schedules nothing: no timer and no display frame until it shows again, when one frame is drawn.

/** The swell's frame rate while nothing else moves in Live mode. Picked by measuring (theseus-wp2d): the waves move
 *  about 15 px a second at the fleet's view, so each frame steps them about a pixel; and each frame is a full-screen
 *  composite, so the rate sets the idle cost (on a CPU rasteriser at 1080p, 2.4 to 3.7 cores at this rate). At 60 Hz
 *  each wait ends on the fourth display frame. */
export const IDLE_FPS = 15

const IDLE_MS = 1000 / IDLE_FPS
/** The timer ends this long before the swell's next frame is due: the display frame it asks for comes after it. */
const SLACK_MS = 10

/** The browser's clock and its two kinds of wait; the engine passes `window`'s, a test a fake. */
export interface Clock {
  now(): number
  /** `requestAnimationFrame`. */
  frame(cb: (when: number) => void): number
  cancelFrame(id: number): void
  timer(cb: () => void, ms: number): number
  cancelTimer(id: number): void
  /** `document.hidden`. */
  hidden(): boolean
}

/** What the loop tells a frame. */
export interface Tick {
  /** Something changed since the last frame (a `request`): the whole scene is drawn, not only the sea. */
  changed: boolean
  /** This frame follows a busy one at the next display frame, so the time between them is a frame's time. */
  paced: boolean
}

export class Loop {
  private clock: Clock
  private draw: (when: number, tick: Tick) => boolean
  private swell: () => boolean
  private raf = 0
  private wait = 0
  /** The pending display frame was asked for by a busy frame. */
  private chained = false
  private changed = true
  private last = 0
  private disposed = false

  /**
   * @param draw draws a frame, and says whether something moves (true: the next display frame is wanted).
   * @param swell whether the sea rolls now: Live mode, and not `?swell=0`.
   */
  constructor(clock: Clock, draw: (when: number, tick: Tick) => boolean, swell: () => boolean) {
    this.clock = clock
    this.draw = draw
    this.swell = swell
  }

  /** Something changed: it is drawn at the next display frame (at once, not at the swell's next beat). */
  request() {
    if (this.disposed) return
    this.changed = true
    if (this.clock.hidden()) return
    this.clearWait()
    if (!this.raf) this.raf = this.clock.frame(this.tick)
  }

  /** The tab was hidden or shown: hidden, nothing waits; shown, one frame, and the loop goes on from there. */
  visibility() {
    if (this.disposed) return
    if (this.clock.hidden()) {
      this.clearWait()
      if (this.raf) this.clock.cancelFrame(this.raf)
      this.raf = 0
      this.chained = false
      return
    }
    this.request()
  }

  /** Whether a display frame or a timer is pending (for tests and the bench). */
  get pending(): { frame: boolean; timer: boolean } {
    return { frame: this.raf !== 0, timer: this.wait !== 0 }
  }

  dispose() {
    this.disposed = true
    this.clearWait()
    if (this.raf) this.clock.cancelFrame(this.raf)
    this.raf = 0
  }

  private clearWait() {
    if (this.wait) this.clock.cancelTimer(this.wait)
    this.wait = 0
  }

  private tick = (when: number) => {
    this.raf = 0
    if (this.disposed) return
    const tick = { changed: this.changed, paced: this.chained }
    this.changed = false
    this.chained = false
    this.last = when
    const busy = this.draw(when, tick)
    if (this.disposed || this.raf || this.clock.hidden()) return
    if (busy) {
      this.raf = this.clock.frame(this.tick)
      this.chained = true
    } else if (this.swell()) {
      const ms = Math.max(0, this.last + IDLE_MS - SLACK_MS - this.clock.now())
      this.wait = this.clock.timer(this.beat, ms)
    }
  }

  private beat = () => {
    this.wait = 0
    if (this.disposed || this.clock.hidden() || this.raf) return
    this.raf = this.clock.frame(this.tick)
  }
}
