// Why each card on the Needs-you column waits on the owner (theseus-nuqut). Pure: `npm test` runs it
// (test/ops-task-tab.test.ts). A tool or budget ask carries its reason (`ConfirmRequest`), a router ask its
// question (`ThreadAsk`, read by `thread.ask_list` and heard by `thread.asked`), and a card that waits with no ask
// to answer says its state.
import type { OpsRow } from './board.ts'
import { askKeysOf, type QueueItem } from './queue.ts'

/** What one item waits for, in a phrase; never empty. */
export function reasonOf(i: QueueItem): string {
  const words = i.ask ? i.ask.question : i.confirm ? i.confirm.plain?.why || i.confirm.plain?.held || i.confirm.reason : i.title
  return words || i.title || i.kind
}

/** What a card with no ask on it waits for: its failure or its state. */
export const stateReason = (r: OpsRow): string => r.view?.report?.reason ?? r.view?.attention.label ?? r.record?.state.replace('_', ' ') ?? r.state ?? 'needs you'

export interface NeedsEntry {
  /** The row (task, conversation or router ask) it belongs to: `OpsRow.id`. */
  task: string
  title: string
  /** Why it waits, one phrase per ask, joined; never empty. */
  reason: string
}

/** One entry per card in the Needs column, with the reasons of the queue items that name it (`askKeysOf`, as the
 *  Board maps them) or, for a router ask no card owns, its own question. */
export function needsEntries(rows: readonly OpsRow[], queue: readonly QueueItem[]): NeedsEntry[] {
  const cards = rows.filter((r) => r.counted && r.column === 'needs')
  const named = new Map<string, OpsRow>()
  for (const r of rows) for (const k of [r.id, r.view?.id, r.recordId, r.task?.info?.execution_id]) if (k && !named.has(k)) named.set(k, r)
  const reasons = new Map<string, string[]>()
  for (const i of queue) {
    const card = askKeysOf(i).map((k) => named.get(k)).find(Boolean)
    if (card) reasons.set(card.id, [...(reasons.get(card.id) ?? []), reasonOf(i)])
  }
  return cards.map((r) => {
    const own = r.item ? [reasonOf(r.item)] : reasons.get(r.id)
    return { task: r.id, title: r.title, reason: own?.length ? own.join('; ') : stateReason(r) }
  })
}
