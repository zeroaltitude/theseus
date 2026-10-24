// The Operations page's small shared state (theseus-jid7t.6): the toast with its Undo, and the profile chosen "for this
// turn only" per session, which the Join composer sends with its next message. In memory: a reload forgets both.
import { create } from 'zustand'

export interface Toast { text: string; undo?: () => void; id: number }

let n = 0
export const useOpsUi = create<{ toast: Toast | null; turnProfile: Record<string, string>; say: (text: string, undo?: () => void) => void; clear: () => void; setTurnProfile: (sid: string, name: string | null) => void }>((set) => ({
  toast: null,
  turnProfile: {},
  say: (text, undo) => set({ toast: { text, undo, id: ++n } }),
  clear: () => set({ toast: null }),
  setTurnProfile: (sid, name) => set((s) => {
    const { [sid]: _gone, ...rest } = s.turnProfile
    return { turnProfile: name ? { ...rest, [sid]: name } : rest }
  }),
}))
