// A graph that fills the window (theseus-uu3b2): its state is one address parameter, `?graph=full`, for the Fleet's
// graph and the Actions view's task graph alike, and its keys are decided here so a test can hold them.

export const FULL_PARAM = 'graph'
const FULL = 'full'

/** Whether the address asks for the full-window graph; any other value is the panel. */
export function parseFull(params: URLSearchParams): boolean {
  return params.get(FULL_PARAM) === FULL
}

/** The address with the graph large or small; every other parameter stays. */
export function writeFull(params: URLSearchParams, full: boolean): URLSearchParams {
  const next = new URLSearchParams(params)
  if (full) next.set(FULL_PARAM, FULL)
  else next.delete(FULL_PARAM)
  return next
}

export interface KeyLike { key: string; ctrlKey?: boolean; metaKey?: boolean; altKey?: boolean; target?: EventTarget | { tagName?: string; isContentEditable?: boolean } | null }

const TYPING = new Set(['INPUT', 'TEXTAREA', 'SELECT'])

/** How long after a `g` the next key is part of its chord: the shell's `g` then a letter (the `GO` table) and the Ops
 *  page's own sequences use the same window. */
export const G_CHORD_MS = 900

/** One step of the g-window over a keydown: `at` is when the open window began (0 when none), the result says whether
 *  this key is inside it and where the window stands after it. A plain `g` opens it, any other key closes it. */
export function gChord(at: number, e: KeyLike, now: number): { afterG: boolean; at: number } {
  const afterG = at > 0 && now - at < G_CHORD_MS
  const t = e.target as { tagName?: string; isContentEditable?: boolean } | null | undefined
  const typing = !!t && (TYPING.has((t.tagName ?? '').toUpperCase()) || !!t.isContentEditable)
  const opens = e.key === 'g' && !typing && !e.ctrlKey && !e.metaKey && !e.altKey
  return { afterG, at: opens ? now : 0 }
}

/** What a key does to the graph: Escape leaves full screen (even from a field); `f` fills the window, not while typing,
 *  not with a modifier, and not right after a `g` (`afterG`: `g f` is the shell's jump to the Fleet, which must not
 *  also push `?graph=full` first); anything else nothing. */
export function fullKey(e: KeyLike, full: boolean, afterG = false): 'enter' | 'leave' | null {
  if (e.key === 'Escape') return full ? 'leave' : null
  if (e.key !== 'f' || e.ctrlKey || e.metaKey || e.altKey) return null
  const t = e.target as { tagName?: string; isContentEditable?: boolean } | null | undefined
  if (t && (TYPING.has((t.tagName ?? '').toUpperCase()) || t.isContentEditable)) return null
  if (afterG) return null
  return full ? 'leave' : 'enter'
}

/** What a refit waits on: animation frames and timers, so a test can hold the order (the browser's are `window`'s). */
export interface RefitClock {
  frame: (cb: () => void) => () => void
  after: (ms: number, cb: () => void) => () => void
}

/** How long after the first fit the second comes. React Flow's ResizeObserver delivers the panel's new size after
 *  the two frames on a cold or loaded page, and a fit made before it fits the old box (theseus-4qx8o). */
export const REFIT_RETRY_MS = 250

/** Fit the graph once its box has its new size: after two frames (one for the class to land, one for the layout), and
 *  once more `REFIT_RETRY_MS` later, for the observer that has not told the graph yet. Both fits are idempotent. Returns
 *  the cancel, which stops whichever has not run. */
export function scheduleRefit(fit: () => void, clock: RefitClock, retryMs = REFIT_RETRY_MS): () => void {
  let stop2 = () => {}
  let stopRetry = () => {}
  const stop1 = clock.frame(() => {
    stop2 = clock.frame(() => {
      fit()
      stopRetry = clock.after(retryMs, fit)
    })
  })
  return () => { stop1(); stop2(); stopRetry() }
}
