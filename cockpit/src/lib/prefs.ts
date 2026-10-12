// The Ship's view prefs kept in this browser (theseus-onxwh): whether the watch panel is out or folded, and the depth
// gauge's size. Pure: the parse of what storage held (anything unknown is the default) and the keys; `usePref.ts`
// reads and writes them.

export const WATCH_KEY = 'cockpit.ship.watch'
export const DEPTH_KEY = 'cockpit.ship.depth'

/** The watch panel: out (the plates in view) or folded (a tab on the right edge). */
export type WatchFold = 'out' | 'in'
export const parseWatch = (raw: string | null): WatchFold => (raw === 'in' ? 'in' : 'out')

/** The depth gauge: small (the default), folded to a chip, or large (an overlay that says what each depth is). */
export type DepthSize = 'small' | 'min' | 'max'
export const parseDepth = (raw: string | null): DepthSize => (raw === 'min' || raw === 'max' ? raw : 'small')

/** The watch's next state when its control is pressed. */
export const flipWatch = (w: WatchFold): WatchFold => (w === 'out' ? 'in' : 'out')
