// The Operations page's regions (theseus-jid7t.3): the command bar over the needs-you queue, the canvas (the lens) and
// the inspector. `[` folds the queue and `]` the inspector, kept in this browser as `lib/prefs.ts` keeps the Ship's
// folds; with nothing selected the inspector is a slim tab, so the canvas has the width; below 1280 px it lies over
// the canvas, and below 900 px the queue is the page. Pure: `npm test`
// runs it (test/ops-keys.test.ts).

export const QUEUE_KEY = 'cockpit.ops.queue'
export const INSPECTOR_KEY = 'cockpit.ops.inspector'

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

/** Where each region goes at `width` px, with the folds as kept and whether anything is selected. */
export function regionsFor(width: number, queue: Fold, inspector: Fold, selected: boolean): Regions {
  if (width < NARROW_PX) return { queue: 'page', inspector: selected ? 'over' : 'none', canvas: false }
  const q = queue === 'folded' ? 'folded' : 'side'
  if (inspector === 'folded') return { queue: q, inspector: 'folded', canvas: true }
  if (width < WIDE_PX) return { queue: q, inspector: selected ? 'over' : 'none', canvas: true }
  return { queue: q, inspector: selected ? 'side' : 'idle', canvas: true }
}
