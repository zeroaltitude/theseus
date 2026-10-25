// The full-window control shared by both graphs (theseus-uu3b2): the address holds the state (`lib/fullgraph.ts`), the
// graph stays mounted and only its panel's box changes, and the graph fits again once the box has its new size.
import { useCallback, useEffect, useRef, type ReactNode } from 'react'
import { useSearchParams } from 'react-router'
import { Maximize2, Minimize2 } from 'lucide-react'
import { fullKey, gChord, parseFull, scheduleRefit, writeFull, type RefitClock } from '@/lib/fullgraph'

/** What this module asks of the graph's instance, whatever its node type. */
export interface Fitter { fitView: (o?: { padding?: number }) => Promise<boolean> }
export const FIT = { padding: 0.2 }

const BROWSER_CLOCK: RefitClock = {
  frame: (cb) => { const id = requestAnimationFrame(cb); return () => cancelAnimationFrame(id) },
  after: (ms, cb) => { const id = setTimeout(cb, ms); return () => clearTimeout(id) },
}

/** `panel`: props for the graph's `Panel` (its box, and a dialog while large); `button`: its header control; `onInit`:
 *  for the `ReactFlow` inside, which this fits on entering and on leaving. */
export function useFullGraph(name: string): { full: boolean; panel: { className: string; dialog?: string }; button: ReactNode; onInit: (rf: Fitter) => void } {
  const [params, setParams] = useSearchParams()
  const full = parseFull(params)
  const rf = useRef<Fitter | null>(null)
  const set = useCallback((on: boolean) => setParams((p) => writeFull(p, on), { replace: !on }), [setParams])

  useEffect(() => {
    let g = 0 // when the last plain `g` was pressed: an `f` inside its window is `g f`, the shell's jump to the Fleet
    const k = (e: KeyboardEvent) => {
      const step = gChord(g, e, Date.now())
      g = step.at
      const a = fullKey(e, full, step.afterG)
      if (!a) return
      if (e.key === 'Escape') e.stopPropagation()
      set(a === 'enter')
    }
    document.addEventListener('keydown', k, true)
    return () => document.removeEventListener('keydown', k, true)
  }, [full, set])

  // Fit after the box has its new size: two frames, and once more for the observer that is late (`scheduleRefit`).
  const first = useRef(true)
  useEffect(() => {
    if (first.current) { first.current = false; return }
    return scheduleRefit(() => { void rf.current?.fitView(FIT) }, BROWSER_CLOCK)
  }, [full])

  const button = (
    <button type="button" className="text-ink-faint hover:text-ink" onClick={() => set(!full)} aria-pressed={full}
      title={full ? 'Back to the panel (Esc)' : 'Fill the window (f)'} aria-label={full ? 'Back to the panel' : 'Fill the window'}>
      {full ? <Minimize2 size={13} aria-hidden /> : <Maximize2 size={13} aria-hidden />}
    </button>
  )
  return {
    full, button, onInit: (r) => { rf.current = r },
    panel: full ? { className: 'fixed inset-0 z-30 !rounded-none', dialog: name } : { className: '' },
  }
}
