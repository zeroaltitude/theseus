// The full-window control shared by both graphs (theseus-uu3b2): the address holds the state (`lib/fullgraph.ts`), the
// graph stays mounted and only its panel's box changes, and the graph fits again once the box has its new size.
import { useCallback, useEffect, useRef, type ReactNode } from 'react'
import { useSearchParams } from 'react-router'
import { Maximize2, Minimize2 } from 'lucide-react'
import { fullKey, parseFull, writeFull } from '@/lib/fullgraph'

/** What this module asks of the graph's instance, whatever its node type. */
export interface Fitter { fitView: (o?: { padding?: number }) => Promise<boolean> }
export const FIT = { padding: 0.2 }

/** `panel`: props for the graph's `Panel` (its box, and a dialog while large); `button`: its header control; `onInit`:
 *  for the `ReactFlow` inside, which this fits on entering and on leaving. */
export function useFullGraph(name: string): { full: boolean; panel: { className: string; dialog?: string }; button: ReactNode; onInit: (rf: Fitter) => void } {
  const [params, setParams] = useSearchParams()
  const full = parseFull(params)
  const rf = useRef<Fitter | null>(null)
  const set = useCallback((on: boolean) => setParams((p) => writeFull(p, on), { replace: !on }), [setParams])

  useEffect(() => {
    const k = (e: KeyboardEvent) => {
      const a = fullKey(e, full)
      if (!a) return
      if (e.key === 'Escape') e.stopPropagation()
      set(a === 'enter')
    }
    document.addEventListener('keydown', k, true)
    return () => document.removeEventListener('keydown', k, true)
  }, [full, set])

  // Fit after the box has its new size: two frames, one for the class to land and one for the layout.
  const first = useRef(true)
  useEffect(() => {
    if (first.current) { first.current = false; return }
    let b = 0
    const a = requestAnimationFrame(() => { b = requestAnimationFrame(() => { void rf.current?.fitView(FIT) }) })
    return () => { cancelAnimationFrame(a); cancelAnimationFrame(b) }
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
