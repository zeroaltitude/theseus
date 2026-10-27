// Going home (theseus-j4cjc, the owner's 2026-10-27 scope on theseus-6ln2q): the Theseus mark at the top left is an
// anchor to the root URL, from every view and route. Pure, with no import at all, so `node --test` runs its test
// (`test/home.test.ts`); the components hold the anchor and hand this the presses.

/** The cockpit's root URL. `main.tsx`'s index route sends it on to the Ship, so a press on the mark lands exactly
 *  where a fresh load of `/` lands. */
export const HOME = '/'

/** The fields of a mouse press this reads. A `MouseEvent` has them all. */
export interface Press {
  button: number
  metaKey: boolean
  ctrlKey: boolean
  shiftKey: boolean
  altKey: boolean
  defaultPrevented?: boolean
}

/** A press the browser answers itself, by opening the address in another tab or window: the middle button, or a
 *  modified click. React Router's `Link` leaves those to the browser rather than routing them, and the reset below
 *  must leave them too: a new tab must not clear the moment this page is showing. */
export function opensElsewhere(e: Press): boolean {
  return !!e.defaultPrevented || e.button !== 0 || e.metaKey || e.ctrlKey || e.shiftKey || e.altKey
}

/** The page's own state a fresh load of the root URL starts without, and so the mark puts back:
 *  - `live`: the time machine's moment (`lib/timemachine.ts`). Scrubbed into the past, every view wears the amber
 *    frame and the bar's readouts read `then`; going home means going live. `TimeMachine` already follows `?t=` on
 *    every change of address, so the root URL clears the moment either way; doing it here clears it *before* the
 *    going, so no frame of the landing view draws in the past.
 *  - `clearSelection`: the ops cards picked (`ops/useSelection.ts`). This one nothing else does: the address keeps
 *    only the focused id, so a selection outlives its page's unmount and would still be picked on the way back.
 *
 *  What a reload keeps, this keeps: the look, calm, the nav accordion and the activity strip's fold are all kept in
 *  the browser, so a press on the mark leaves them alone. */
export interface HomeReset {
  live: () => void
  clearSelection: () => void
}

/** A press on the mark. The anchor does the going; this does the putting back, and says whether it did: a press that
 *  opens the root URL somewhere else leaves this page as it stands. */
export function pressedHome(e: Press, r: HomeReset): boolean {
  if (opensElsewhere(e)) return false
  r.live()
  r.clearSelection()
  return true
}
