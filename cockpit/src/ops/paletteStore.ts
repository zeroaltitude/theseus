// What the Operations page publishes to the shell's ⌘K palette while it is mounted (theseus-jid7t.6): the targets of
// each scope, the entities, and what choosing one does. The palette lives in the shell, the page's state in the page;
// this is the seam between them.
import { create } from 'zustand'
import type { ProfileInfo } from '@protocol'
import type { Price } from '../lib/money.ts'
import { nextScope, type Entity, type OpsTarget, type Scope } from './palette.ts'

export interface OpsPaletteFeed {
  targets: Readonly<Record<Scope, readonly OpsTarget[]>>
  entities: readonly Entity[]
  profiles: readonly ProfileInfo[]
  prices: ReadonlyMap<string, Price>
  go: (e: Entity) => void
  newTask: () => void
  selfChanges: () => void
}

export const useOpsPalette = create<{ feed: OpsPaletteFeed | null; scope: Scope; publish: (f: OpsPaletteFeed | null) => void; cycle: () => void }>((set) => ({
  feed: null,
  scope: 'one',
  publish: (feed) => set({ feed }),
  cycle: () => set((s) => ({ scope: nextScope(s.scope) })),
}))
