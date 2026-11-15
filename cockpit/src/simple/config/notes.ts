// What the controls said, kept where the page can show it: a control's row can leave the list the moment its act is done
// (an import erased, a proposal kept), and with it any line beside its button, so every answer is also put here. Newest
// first, a few kept, each dismissable.
import { create } from 'zustand'

export interface Note { id: number; ok: boolean; text: string }

let next = 0
const KEEP = 5

export const useNotes = create<{ notes: Note[]; add: (ok: boolean, text: string) => void; drop: (id: number) => void }>((set) => ({
  notes: [],
  add: (ok, text) => set((s) => ({ notes: [{ id: ++next, ok, text }, ...s.notes].slice(0, KEEP) })),
  drop: (id) => set((s) => ({ notes: s.notes.filter((n) => n.id !== id) })),
}))
