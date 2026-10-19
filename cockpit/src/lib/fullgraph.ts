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

/** What a key does to the graph: Escape leaves full screen (even from a field); `f` fills the window, not while typing
 *  and not with a modifier; anything else nothing. */
export function fullKey(e: KeyLike, full: boolean): 'enter' | 'leave' | null {
  if (e.key === 'Escape') return full ? 'leave' : null
  if (e.key !== 'f' || e.ctrlKey || e.metaKey || e.altKey) return null
  const t = e.target as { tagName?: string; isContentEditable?: boolean } | null | undefined
  if (t && (TYPING.has((t.tagName ?? '').toUpperCase()) || t.isContentEditable)) return null
  return full ? 'leave' : 'enter'
}
