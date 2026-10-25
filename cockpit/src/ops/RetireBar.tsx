// The checkbox group that brings retired nodes back (theseus-99l9z), for the Fleet, the Graph lens and the Actions
// page's task graph: `useRetire(view)` holds the choice (the address first, then this browser, `ops/retire.ts`), and
// `RetireBar` draws the five checkboxes, the cutoff and the "+N older" chip.
import { useCallback, useMemo } from 'react'
import { useSearchParams } from 'react-router'
import { cn } from '@/lib/format'
import { CLASSES, CLASS_WORDS, DAY_CHOICES, olderWords, readChoice, writeParams, writeStored, type Choice, type RetireClass, type Store } from './retire'

const storage = (): Store | undefined => { try { return localStorage } catch { return undefined } }

export function useRetire(view: string): { choice: Choice; set: (c: Choice) => void } {
  const [params, setParams] = useSearchParams()
  // This browser's copy is read when the address says nothing, and again on every change of the address.
  const choice = useMemo(() => readChoice(params, storage(), view), [params, view])
  const set = useCallback((c: Choice) => {
    writeStored(storage(), view, c)
    setParams((p) => writeParams(p, c), { replace: true })
  }, [view, setParams])
  return { choice, set }
}

export function RetireBar({ choice, set, counts, hidden, className }: { choice: Choice; set: (c: Choice) => void; counts: Readonly<Record<RetireClass, number>>; hidden: number; className?: string }) {
  return (
    <span className={cn('flex flex-wrap items-center gap-x-2 gap-y-1', className)} data-testid="retire-bar">
      <span title={`${hidden} hidden: nothing is deleted, and the counters count them`} className="num rounded px-1.5 ring-1 ring-line text-ink-dim" data-testid="retire-chip">{olderWords(hidden)}</span>
      {CLASSES.map((c) => (
        <label key={c} className="flex items-center gap-1 text-ink-dim" title={`show the ${CLASS_WORDS[c]} ones`}>
          <input type="checkbox" checked={choice.show[c]} onChange={(e) => set({ ...choice, show: { ...choice.show, [c]: e.target.checked } })} />
          {CLASS_WORDS[c]} <span className="num text-ink-faint">{counts[c]}</span>
        </label>
      ))}
      <label className="flex items-center gap-1 text-ink-dim" title="older than this many days is not recent">
        recent
        <select value={choice.days} onChange={(e) => set({ ...choice, days: Number(e.target.value) })} className="rounded bg-transparent ring-1 ring-line">
          {[...new Set([...DAY_CHOICES, choice.days])].sort((a, b) => a - b).map((d) => <option key={d} value={d}>{d} d</option>)}
        </select>
      </label>
    </span>
  )
}
