// The simple Ops page's keys (GitHub issue 17): the Operations page's key table (`ops/keys.ts`), the part of it this page
// has a place for, and the same rules for which key means what. There is no second table: `SIMPLE_VERBS` only names
// the entries of the one table that work here, `resolve` here walks that table in its own order, and the help is its
// `overlay()` kept to those entries. A key the Ship's page has and this page has not (the lenses, the graph's picks,
// the Timeline's marks, the filters, the dock) does nothing here and is not listed.
// Pure: `npm test` runs it (test/simple-ops-keys.test.ts).
import type { SessionInfo } from '../../protocol.gen/index.ts'
import type { OpsRow } from '../../ops/board.ts'
import { grantKey, type QueueItem } from '../../ops/queue.ts'
import { KEYS, overlay, PRIORITY, type Ctx, type KeyDef, type Verb } from '../../ops/keys.ts'
import type { WorkSet } from './workset.ts'
import { selectedOf } from './workset.ts'

/** The verbs of the Operations page's table that this page has a handler for: moving, answering, steering, the menus. */
export const SIMPLE_VERBS = [
  'down', 'up', 'inspect', 'open', 'back', 'next', 'prev',
  'approve', 'decline', 'choice', 'grant', 'retry', 'dismiss',
  'pause', 'stop', 'stop_all', 'cancel', 'not_this', 'cancel_wake', 'new_task',
  'model_menu', 'mode_menu', 'compose', 'parent', 'last_turn',
  'scroll_up', 'scroll_down', 'scroll_end', 'keys',
] as const satisfies readonly Verb[]
export type SimpleVerb = (typeof SIMPLE_VERBS)[number]
const SIMPLE: ReadonlySet<string> = new Set(SIMPLE_VERBS)
export const isSimple = (v: Verb): v is SimpleVerb => SIMPLE.has(v)

/** What the selection is, as the keys see it: the ask, the card, the session, and the contexts they offer. */
export interface Focus {
  focusItem?: QueueItem
  focusRow?: OpsRow
  focusSession?: SessionInfo
  /** The running execution a stop acts on: a conversation's. */
  exe?: string
  ctxs: Set<Ctx>
}

/** The Ship page's own reading of its selection (`views/Ops.tsx`: `focusItem`, `focusRow`, `focusSession`, `ctxs`), over
 *  the simple page's selection: an ask is the card it belongs to, and a card is the ask that waits on it. */
export function focusOf(w: WorkSet, sessions: readonly SessionInfo[], sel: string | null): Focus {
  const { item, row, session } = selectedOf(w, sessions, sel)
  const focusItem = item ?? (row ? w.queue.find((i) => (row.view != null && i.view?.id === row.view.id) || (row.sessionId != null && i.sessionId === row.sessionId)) : undefined)
  const focusRow = row ?? (item?.sessionId ? w.rows.find((r) => r.sessionId === item.sessionId && r.kind !== 'job') : undefined)
  const focusSession = (focusRow && (focusRow.kind === 'task' || focusRow.kind === 'conversation') ? focusRow.session : undefined) ?? session
    ?? (item?.sessionId ? sessions.find((x) => x.session_id === item.sessionId) : undefined)
  const living = focusRow?.view?.state === 'running' || focusRow?.view?.state === 'waiting'
  const exe = focusRow?.kind === 'conversation' ? focusRow.view?.id ?? focusRow.session?.execution_id : undefined
  const c = new Set<Ctx>()
  if (focusItem || focusRow || session) c.add('card')
  if (focusItem?.confirm || focusItem?.proposal) c.add('answer')
  if (focusItem?.view?.question?.choices?.length) c.add('choice')
  if (focusItem?.confirm?.grant_offer) c.add('grant')
  if (focusItem?.kind === 'failure') c.add('failure')
  if (focusRow?.kind === 'task' && focusRow.column !== 'done') c.add('task')
  if (focusRow?.kind === 'conversation' && living) c.add('running')
  if (focusRow?.kind === 'job') c.add('job')
  if (focusRow?.kind === 'wake') c.add('wake')
  if (focusSession) c.add('session')
  return { focusItem, focusRow, focusSession, exe, ctxs: c }
}

/** What a key does here: the table's entry of the most specific context that claims it, among those this page has.
 *  A grant is its offer's own key (`a` for a command, `e` for edits): the other one falls through. Nothing fires
 *  while a person types. */
export function resolveHere(key: string, f: Focus, typing: boolean): (KeyDef & { verb: SimpleVerb }) | null {
  if (typing) return null
  const find = (skipGrant: boolean) => {
    for (const c of PRIORITY) {
      if (c !== 'page' && c !== 'filter' && !f.ctxs.has(c)) continue
      if (skipGrant && c === 'grant') continue
      const hit = KEYS.find((k) => k.ctx === c && isSimple(k.verb) && k.keys.includes(key))
      if (hit) return hit as KeyDef & { verb: SimpleVerb }
    }
    return null
  }
  const hit = find(false)
  return hit?.verb === 'grant' && f.focusItem?.confirm && grantKey(f.focusItem.confirm) !== key ? find(true) : hit
}

/** The help: the Operations page's overlay, kept to the keys that work here. */
export function helpHere(): Array<{ ctx: Ctx; heading: string; lines: Array<{ verb: Verb; keys: string; words: string }> }> {
  return overlay().map((g) => ({ ...g, lines: g.lines.filter((l) => SIMPLE.has(l.verb)) })).filter((g) => g.lines.length > 0)
}
