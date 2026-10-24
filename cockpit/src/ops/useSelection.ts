// The selection's one home on the page (theseus-jid7t.4): a set of ids kept in memory, shared by the Graph and the
// Board, the bulk bar, and ops-act's palette (its "the selection" scope reads it here). The rules are
// `ops/selection.ts`'s; the address keeps only the focused id (`?sel=`).
import { create } from 'zustand'
import { afterRun, clear, selectAll, toggle, type RunResult, type Selection } from './selection'

export interface SelectionStore {
  ids: Selection
  toggle: (id: string) => void
  /** Replace it: ⌘A's everything shown, a lasso's catch. */
  set: (ids: Iterable<string>) => void
  clear: () => void
  /** After a bulk run: only the failed items stay picked, so a retry targets exactly them. */
  keepFailed: (results: readonly RunResult[]) => void
}

export const useSelection = create<SelectionStore>((set) => ({
  ids: clear(),
  toggle: (id) => set((s) => ({ ids: toggle(s.ids, id) })),
  set: (ids) => set({ ids: selectAll([...ids]) }),
  clear: () => set({ ids: clear() }),
  keepFailed: (results) => set((s) => ({ ids: afterRun(s.ids, results) })),
}))
