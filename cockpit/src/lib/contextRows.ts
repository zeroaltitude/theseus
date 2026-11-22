// The context compiles the instruments read: the per-call `context.compiled` row, which every compile writes and
// which carries the compilation's id, trigger and estimate. The daemon stopped writing `context.recompiled` (a
// summary of the same compilation, in the same frame; issue 46): an old store's such rows carry no estimate, are
// counted by nothing here, and are still said in words in the ledger (`summary.ts`). Pure, with no import but the
// protocol's types, so `node --test` runs its test (`test/contextrows.test.ts`).
import type { LedgerEntry } from '@protocol'

/** Whether a row is a context compile, as the instruments count them. */
export const isContextCompile = (kind: string): boolean => kind === 'context.compiled'

/** est_tokens per context compile, per session: [time, tokens]. */
export function contextSeries(rows: LedgerEntry[] | undefined): Map<string, [number, number][]> {
  const m = new Map<string, [number, number][]>()
  for (const r of rows ?? []) {
    if (!isContextCompile(r.kind)) continue
    const d = (r.data ?? {}) as Record<string, any>
    const sid = d.session_id ?? r.session_id ?? '?'
    if (typeof d.est_tokens !== 'number') continue
    const arr = m.get(sid) ?? []
    arr.push([r.at_unix_ms, d.est_tokens])
    m.set(sid, arr)
  }
  return m
}
