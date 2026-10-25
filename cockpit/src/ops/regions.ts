// The Operations page's regions (theseus-jid7t.3): the command bar over the needs-you queue, the canvas (the lens) and
// the inspector. `[` folds the queue and `]` the inspector, kept in this browser as `lib/prefs.ts` keeps the Ship's
// folds; with nothing selected the inspector is a slim tab, so the canvas has the width; below 1280 px it lies over
// the canvas, and below 900 px the queue is the page. Pure: `npm test`
// runs it (test/ops-keys.test.ts).

export const QUEUE_KEY = 'cockpit.ops.queue'
export const INSPECTOR_KEY = 'cockpit.ops.inspector'
export const DOCK_KEY = 'cockpit.ops.dock'

export type Fold = 'open' | 'folded'
export const parseFold = (raw: string | null): Fold => (raw === 'folded' ? 'folded' : 'open')
export const flip = (f: Fold): Fold => (f === 'open' ? 'folded' : 'open')

export const WIDE_PX = 1280
export const NARROW_PX = 900

export interface Regions {
  /** `side`: a column left of the canvas; `page`: the whole page (a narrow screen); `folded`: a tab. */
  queue: 'side' | 'page' | 'folded'
  /** `side`: a column right of the canvas; `over`: over the canvas, while something is selected; `folded`: a tab;
   *  `idle`: nothing selected, a slim tab that gives the canvas its width; `none`: nothing selected on a narrow
   *  screen. */
  inspector: 'side' | 'over' | 'folded' | 'idle' | 'none'
  canvas: boolean
}

/** The address's `sel` for an inspector the owner opened by hand with nothing selected. It names no card; `Esc` clears
 *  it as it clears any selection, and a link to it reopens the empty panel. */
export const OPEN_SEL = '~'

/** What the idle tab's click selects: the first ask in Needs you, else nothing, with the panel open all the same. */
export const idleSel = (firstAsk: string | undefined): string => firstAsk ?? OPEN_SEL

/** Where each region goes at `width` px, with the folds as kept, whether anything is selected, and whether the owner
 *  opened the inspector by hand (the idle tab) with nothing selected: it then shows its empty state. */
export function regionsFor(width: number, queue: Fold, inspector: Fold, selected: boolean, byHand = false): Regions {
  const shows = selected || byHand
  if (width < NARROW_PX) return { queue: 'page', inspector: shows ? 'over' : 'none', canvas: false }
  const q = queue === 'folded' ? 'folded' : 'side'
  if (inspector === 'folded') return { queue: q, inspector: 'folded', canvas: true }
  if (width < WIDE_PX) return { queue: q, inspector: shows ? 'over' : 'none', canvas: true }
  return { queue: q, inspector: shows ? 'side' : 'idle', canvas: true }
}

/** Whether Coming up (the queue's foot) is on screen at `width` with the queue as kept: the queue region is not a tab.
 *  The page asks the calendar for it only then. */
export const comingShows = (width: number, queue: Fold): boolean => regionsFor(width, queue, 'open', false).queue !== 'folded'
