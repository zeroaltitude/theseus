// The work board's views on the page: `work.watch`'s snapshot, then `work.changed`, kept by the position rule.
// Pure: `npm test` runs it (test/workboard.test.ts), and it imports only the protocol's types.
import type { WorkChanged, WorkView, WorkWatchResult } from '@protocol'

/** What the page holds of the board. */
export interface BoardState { views: Record<string, WorkView>; position: number }

export const EMPTY: BoardState = { views: {}, position: 0 }

function applyViews(views: Record<string, WorkView>, incoming: WorkView[]): Record<string, WorkView> {
  let next = views
  for (const v of incoming) {
    const old = next[v.id]
    if (old && v.position <= old.position) continue
    if (next === views) next = { ...views }
    next[v.id] = v
  }
  return next
}

/** `work.watch`'s snapshot: the truth for everything it names; a view it lacks that is not newer than it is gone. */
export function applySnapshot(s: BoardState, snap: WorkWatchResult): BoardState {
  const named = new Set(snap.views.map((v) => v.id))
  const kept: Record<string, WorkView> = {}
  for (const [id, v] of Object.entries(s.views)) if (named.has(id) || v.position > snap.position) kept[id] = v
  return { views: applyViews(kept, snap.views), position: Math.max(s.position, snap.position) }
}

/** One `work.changed`: views by the position rule, and the ids that left. */
export function applyChanged(s: BoardState, c: WorkChanged): BoardState {
  let views = applyViews(s.views, c.views)
  if (c.gone?.some((id) => id in views)) {
    views = { ...views }
    for (const id of c.gone) delete views[id]
  }
  return { views, position: Math.max(s.position, c.position) }
}
