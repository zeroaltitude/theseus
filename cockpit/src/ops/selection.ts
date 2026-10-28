// The Operations page's selection and its bulk acts (theseus-jid7t.4). Pure: `npm test` runs it
// (test/ops-selection.test.ts). The selection is a set of ids of mixed kinds (a row's `ses_`/`act_`/`wak_` id, a step's
// `tsk_`), kept in memory (`useSelection`, `ops/useSelection.ts`); the address keeps only the focused one (`?sel=`).
//
// A bulk act is a client fan-out of single calls, never a batch method: each item is one existing protocol call, so
// each has its own ledger row and its own refusal, and reaches every task card as the same act from any surface would.
// Approve follows the owner's decision 4: only tool approvals that are not of the always-ask kind (`floor`), that this
// connection may answer, and only after a review sheet; a floor approval is listed as asking one by one, never sent.
import type { OpsRow } from './board.ts'
import { alive } from './board.ts'
import { isEndedState, isRunning } from './state.ts'
import { answerOf, type QueueItem } from './queue.ts'

export type Selection = ReadonlySet<string>

/** The focused card or node in or out of the selection. */
export function toggle(sel: Selection, id: string): Set<string> {
  const next = new Set(sel)
  if (!next.delete(id)) next.add(id)
  return next
}

/** Everything shown, after the filters and the search. */
export const selectAll = (visible: readonly string[]): Set<string> => new Set(visible)

export const clear = (): Set<string> => new Set()

export type BulkVerb = 'pause' | 'stop' | 'cancel' | 'rehome' | 'approve' | 'decline' | 'attach'
export const BULK_VERBS: readonly BulkVerb[] = ['pause', 'stop', 'cancel', 'rehome', 'approve', 'decline', 'attach']

/** One call of a fan-out: the selected id it is for (a failure keeps that id selected), its own key, what it is in
 *  words, and the call. A re-home's and an attach's call waits on the home or project the sheet picks. */
export interface BulkItem { id: string; key: string; label: string; method?: string; params?: Record<string, unknown>; row?: OpsRow; item?: QueueItem }

export interface BulkOffer {
  verb: BulkVerb
  /** The bar's words: `Pause 2 · resume 1`, `Cancel 3`. */
  label: string
  items: BulkItem[]
  /** Why no selected item admits it, when none does. */
  reason?: string
  /** It asks once, with the list, before it runs. */
  asks: boolean
  /** Approve's floor approvals: listed in the review sheet as asking one by one, never sent. */
  oneByOne?: QueueItem[]
}

const WHY: Readonly<Record<BulkVerb, string>> = {
  pause: 'nothing selected is a working task',
  stop: 'nothing selected is a running conversation',
  cancel: 'nothing selected is a living task or a wake',
  rehome: 'nothing selected is a living task',
  approve: 'nothing selected waits on a tool approval this page may answer',
  decline: 'nothing selected waits on an answer this page may give',
  attach: 'nothing selected is a living task or a conversation',
}

const WORD: Readonly<Record<BulkVerb, string>> = { pause: 'Pause', stop: 'Stop', cancel: 'Cancel', rehome: 'Re-home', approve: 'Approve', decline: 'Decline', attach: 'Attach' }

const ASKS: ReadonlySet<BulkVerb> = new Set(['stop', 'cancel', 'rehome', 'decline'])

/** A tool approval bulk Approve may send (decision 4): not a budget ask, not a task change, not of the always-ask kind,
 *  and one this connection may answer. */
export const bulkApprovable = (i: QueueItem): boolean => i.kind === 'approval' && !!i.confirm && !i.confirm.floor && i.confirm.may_answer !== false

/** Its floor approvals: the review sheet lists them as asking one by one. */
export const floorApproval = (i: QueueItem): boolean => i.kind === 'approval' && i.confirm?.floor === true

const declinable = (i: QueueItem): boolean => (i.kind === 'approval' || i.kind === 'budget' || i.kind === 'change') && !!i.confirm && i.confirm.may_answer !== false

const living = (r: OpsRow) => alive(r) && !(r.view && isEndedState(r.view.state))

/** The queue's items a selected row owns: its approvals, budget asks, changes and questions. */
export function itemsOf(rows: readonly OpsRow[], queue: readonly QueueItem[]): QueueItem[] {
  const sessions = new Set(rows.flatMap((r) => (r.sessionId ? [r.sessionId] : [])))
  const views = new Set(rows.flatMap((r) => (r.view ? [r.view.id] : [])))
  return queue.filter((i) => (i.sessionId != null && sessions.has(i.sessionId)) || (i.view != null && views.has(i.view.id)))
}

/** Each bulk verb, with the selected items that admit it and its count, or the reason none does. */
export function bulkVerbs(rows: readonly OpsRow[], queue: readonly QueueItem[]): BulkOffer[] {
  const items = itemsOf(rows, queue)
  const one = (r: OpsRow, key: string, label: string, method?: string, params?: Record<string, unknown>): BulkItem => ({ id: r.id, key: `${r.id}:${key}`, label, method, params, row: r })
  const ask = (i: QueueItem, verb: 'approve' | 'decline'): BulkItem => {
    const c = answerOf(i, { verb })!
    const owner = rows.find((r) => r.sessionId === i.sessionId || (i.view && r.view?.id === i.view.id))
    return { id: owner?.id ?? i.key, key: `${i.key}:${verb}`, label: `${i.title} · ${i.owner}`, method: c.method, params: c.params, item: i }
  }
  const pausable = rows.filter((r) => r.kind === 'task' && living(r) && r.column !== 'needs' && r.column !== 'scheduled')
  const offers: Record<BulkVerb, BulkItem[]> = {
    pause: pausable.map((r) => one(r, r.paused ? 'resume' : 'pause', `${r.paused ? 'resume' : 'pause'} ${r.title}`, r.paused ? 'task.resume' : 'task.pause', { task: r.id })),
    stop: rows.filter((r) => r.kind === 'conversation' && isRunning(r.view?.state) && (r.view?.id ?? r.session?.execution_id))
      .map((r) => one(r, 'stop', `stop the turn of ${r.title}`, 'execution.stop', { execution_id: r.view?.id ?? r.session?.execution_id })),
    cancel: rows.flatMap((r) => (r.kind === 'task' && living(r) ? [one(r, 'cancel', `cancel ${r.title}`, 'task.cancel', { task: r.id })]
      : r.kind === 'wake' && living(r) ? [one(r, 'cancel', `cancel the wake ${r.title}`, 'wake.cancel', { wake: r.id })] : [])),
    rehome: rows.filter((r) => r.kind === 'task' && living(r)).map((r) => one(r, 'rehome', r.title)),
    approve: items.filter(bulkApprovable).map((i) => ask(i, 'approve')),
    decline: items.filter(declinable).map((i) => ask(i, 'decline')),
    attach: rows.filter((r) => ((r.kind === 'task' && living(r)) || r.kind === 'conversation') && r.sessionId).map((r) => one(r, 'attach', r.title)),
  }
  return BULK_VERBS.map((verb) => {
    const its = offers[verb]
    const n = its.length
    const resume = its.filter((i) => i.method === 'task.resume').length
    const label = n === 0 ? (verb === 'attach' ? 'Attach to project…' : WORD[verb])
      : verb === 'pause' ? [n > resume ? `Pause ${n - resume}` : '', resume ? `${n > resume ? 'resume' : 'Resume'} ${resume}` : ''].filter(Boolean).join(' · ')
      : verb === 'attach' ? `Attach ${n} to project…` : `${WORD[verb]} ${n}`
    return { verb, label, items: its, reason: n ? undefined : WHY[verb], asks: ASKS.has(verb), oneByOne: verb === 'approve' ? items.filter(floorApproval) : undefined }
  })
}

/** One item's outcome: done, or refused in the daemon's words. */
export interface RunResult { id: string; key: string; label: string; ok: boolean; words: string }

/** Run each item through `run`, at most `limit` at a time, in order; one item's refusal never stops the others. The
 *  results are in the items' order. */
export async function fanOut<T extends { id: string; key: string; label: string }>(items: readonly T[], run: (item: T) => Promise<{ ok: boolean; words: string }>, limit = 4): Promise<RunResult[]> {
  const out: RunResult[] = new Array(items.length)
  let next = 0
  const lane = async () => {
    while (next < items.length) {
      const i = next++
      const it = items[i]
      let r: { ok: boolean; words: string }
      try { r = await run(it) } catch (e) { r = { ok: false, words: (e as { message?: string } | null)?.message ?? String(e) } }
      out[i] = { id: it.id, key: it.key, label: it.label, ok: r.ok, words: r.words }
    }
  }
  await Promise.all(Array.from({ length: Math.max(1, Math.min(limit, items.length)) }, lane))
  return out
}

/** The selection after a run: the failed items stay selected, so a retry targets exactly them. */
export function afterRun(sel: Selection, results: readonly RunResult[]): Set<string> {
  const failed = new Set(results.filter((r) => !r.ok).map((r) => r.id))
  return new Set([...sel].filter((id) => failed.has(id)))
}
