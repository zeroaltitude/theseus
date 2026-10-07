// The page's endless decorations go still while nothing changes (theseus-jgme, the owner's F12): the header's sweep, the
// live dots' pings and the soft pulses ran forever in Live mode, about 0.7 of a core of a CPU rasteriser on an idle
// page. Now they run while the daemon says something (a push, a new ledger row, the link changing) and for `STIR_MS`
// after, one sweep and a little, then stand still: lit, not moved, as the Ship's waiting lantern does. The page wears
// `html.stirred` while they run (`index.css`); Calm and reduced motion still them whatever happens. Pure: the Shell
// pokes it, and a test drives it on a fake clock.

/** How long the decorations run after the last change: one sweep of the header (3.2 s) and a little. */
export const STIR_MS = 4000

/** The class the page wears while they run. */
export const STIR_CLASS = 'stirred'

/** The browser's timers; the Shell passes `window`'s, a test a fake. */
export interface StirClock {
  timer(cb: () => void, ms: number): number
  cancelTimer(id: number): void
}

export class Stir {
  private clock: StirClock
  private set: (on: boolean) => void
  private on = false
  private wait = 0

  /** @param set turns the decorations on or off (the page's class); called only when that changes. */
  constructor(clock: StirClock, set: (on: boolean) => void) {
    this.clock = clock
    this.set = set
  }

  /** Something changed: the decorations run, until `STIR_MS` after the last change. */
  poke() {
    if (!this.on) { this.on = true; this.set(true) }
    if (this.wait) this.clock.cancelTimer(this.wait)
    this.wait = this.clock.timer(() => { this.wait = 0; this.on = false; this.set(false) }, STIR_MS)
  }

  get stirred(): boolean {
    return this.on
  }

  dispose() {
    if (this.wait) this.clock.cancelTimer(this.wait)
    this.wait = 0
  }
}
