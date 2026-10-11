// The watch as a slide-out panel (theseus-onxwh): the Ship's right column keeps its place, a minimize control in its
// head slides it off the edge, and a tab hangs there while it is away. Everything stays mounted while folded (the
// look's clock and the day's scan keep running), only out of sight and out of the tab order. Kept in this browser.
import type { ReactNode } from 'react'
import { ChevronLeft, Minus } from 'lucide-react'
import { flipWatch, parseWatch, WATCH_KEY } from '@/lib/prefs'
import { HelpButton } from '@/components/Help'
import { usePref } from '@/lib/usePref'

export function WatchDock({ children }: { children: ReactNode }) {
  const [fold, setFold] = usePref(WATCH_KEY, parseWatch)
  const out = fold === 'out'
  return (
    <>
      <div data-ship-ui data-fold={fold} inert={!out} id="ship-watch-panel"
        className="ship-watch-slot ship-slide pointer-events-auto absolute right-3 top-[64px]">
        <div className="ship-slide-head">
          <span className="ship-engraved text-[9px]">The watch</span>
          <span className="ml-auto flex items-center gap-1"><HelpButton id="ship.watch" title="The watch" />
          <button type="button" className="ship-depth-ctl" onClick={() => setFold(flipWatch(fold))}
            title="Minimize the watch: it slides off to the right, and a tab brings it back" aria-label="Minimize the watch" aria-expanded={out} aria-controls="ship-watch-panel">
            <Minus size={10} aria-hidden />
          </button></span>
        </div>
        {children}
      </div>
      {!out && (
        <button type="button" data-ship-ui className="ship-slide-tab pointer-events-auto" onClick={() => setFold(flipWatch(fold))}
          title="Open the watch: what is working, waiting, slow, spent, wrong, and new since you last looked" aria-label="Open the watch" aria-expanded={out} aria-controls="ship-watch-panel">
          <ChevronLeft size={11} aria-hidden /> The watch
        </button>
      )}
    </>
  )
}
