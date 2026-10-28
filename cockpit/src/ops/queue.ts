// The needs-you queue (theseus-jid7t.3): one list of everything waiting on the owner, the longest waiting first (the
// TUI's tab order), each item answered in place. Pure: `npm test` runs it (test/ops-queue.test.ts). The page passes in
// what it already holds: `confirm.list`'s questions, the work board's views, the task records, the thread groups, and
// the router's questions the push brought.
//
// - A tool approval, a budget ask and a task change are `confirm.list`'s; each is one card, answered by `work.answer`.
// - A task change proposed and not yet in the list (its record's `proposal`) is a change too, by its card's id.
// - A question with choices is the board's `question` view (`kind: ask`); an approval's question view is the same
//   question as its confirm, so it is not listed twice.
// - A failure is a task's or conversation's view that failed and still asks for someone (`execution.dismiss` clears
//   it): retry, dismiss or open. Under the time machine it is an execution that ended failed, blocked or out of budget.
// - A seal recommended and not yet answered, a message the router holds while it asks which thread it is for.
// The cursor is an item's key, never an index: a new item slides in where its age puts it and the cursor stays on the
// item it was on (`keepCursor`).
import type { ConfirmRequest, ExecutionInfo, TaskRecord, ThreadAsk, ThreadGroup, WorkView } from '@protocol'

export type ItemKind = 'approval' | 'budget' | 'change' | 'question' | 'failure' | 'seal' | 'ask'

export interface QueueItem {
  key: string
  kind: ItemKind
  /** When it came to need the owner: the queue's order, longest first. */
  since: number
  /** The verb and what it does, in a phrase. */
  title: string
  /** Whose it is: the session, task or thread it belongs to. */
  owner: string
  sessionId?: string
  expiresAt?: number
  confirm?: ConfirmRequest
  view?: WorkView
  execution?: ExecutionInfo
  thread?: ThreadGroup
  ask?: ThreadAsk
  proposal?: { record: TaskRecord }
}

export interface QueueInput {
  confirms: readonly ConfirmRequest[]
  views: readonly WorkView[]
  records?: readonly TaskRecord[]
  groups?: readonly ThreadGroup[]
  /** The router's open questions, each with when the page heard it. */
  asks?: ReadonlyArray<{ ask: ThreadAsk; heard_ms: number }>
  /** Under the time machine: the executions as they stood (the board holds no views then). */
  executions?: readonly ExecutionInfo[]
  /** The time machine shows the past (`world != null`). Absent, it is guessed from the views, as before the page passed it. */
  past?: boolean
  /** A session's title, for the owner column. */
  titleOf?: (sessionId: string) => string
}

const FAILED_VIEW = ['failed']
const FAILED_EXEC = ['failed', 'blocked', 'budget_exhausted']

function confirmKind(c: ConfirmRequest): ItemKind {
  return c.budget ? 'budget' : c.change ? 'change' : 'approval'
}

/** Every item waiting on the owner, longest first; on a tie, by key, so the order is the same on every read. */
export function queueOf(q: QueueInput): QueueItem[] {
  const title = q.titleOf ?? ((s: string) => s.slice(-6))
  const out: QueueItem[] = []
  const asked = new Set<string>()
  // An `owner_ask` is in the list too: its board view carries the choices, and it is a question, not an approval.
  const choosing = new Map(q.views.flatMap((v) => (v.kind === 'question' && v.question?.kind === 'ask' ? [[v.question.id, v] as const] : [])))
  for (const c of q.confirms) {
    asked.add(c.correlation_id)
    const ask = choosing.get(c.correlation_id)
    const kind = ask ? 'question' : confirmKind(c)
    out.push({
      key: `q:${c.correlation_id}`, kind, since: c.requested_at_ms, owner: c.task ? `task ${c.task.title ?? c.task.short}` : title(c.session_id),
      title: ask?.question?.prompt ?? c.plain?.what ?? (kind === 'budget' ? 'Reset its spend to $0 and go on' : kind === 'change' ? 'Accept the task change' : `Approve ${c.tool}`),
      sessionId: c.session_id, expiresAt: c.expires_at_ms || undefined, confirm: c, view: ask,
    })
  }
  for (const t of q.records ?? []) {
    const p = t.proposal
    if (!p || asked.has(p.card)) continue
    asked.add(p.card)
    out.push({
      key: `q:${p.card}`, kind: 'change', since: p.at_ms, owner: t.title, sessionId: t.session,
      title: p.abandon ? `Abandon ${t.title}` : p.objective != null ? 'Change its objective' : 'Change its acceptance', proposal: { record: t },
    })
  }
  for (const v of q.views) {
    if (v.kind === 'question' && v.question) {
      if (asked.has(v.question.id) || v.question.kind !== 'ask') continue
      asked.add(v.question.id)
      out.push({
        key: `q:${v.question.id}`, kind: 'question', since: v.attention.since_ms || v.question.asked_at_ms, owner: v.session_id ? title(v.session_id) : v.root,
        title: v.question.prompt, sessionId: v.session_id, expiresAt: v.question.expires_at_ms, view: v,
      })
    } else if ((v.kind === 'task' || v.kind === 'conversation') && FAILED_VIEW.includes(v.state) && v.attention.level === 'needs_you' && !v.dismissed) {
      out.push({
        key: `f:${v.id}`, kind: 'failure', since: v.attention.since_ms || v.state_since_ms, owner: v.title || (v.session_id ? title(v.session_id) : v.id),
        title: v.report?.reason ?? v.attention.label ?? 'it failed', sessionId: v.session_id, view: v,
      })
    }
  }
  if (q.past ?? q.views.length === 0) {
    for (const e of q.executions ?? []) {
      if (!FAILED_EXEC.includes(e.state)) continue
      out.push({
        key: `f:${e.execution_id}`, kind: 'failure', since: e.attention?.since_ms ?? e.updated_at_ms, owner: title(e.session_id),
        title: e.ended_reason ?? e.state.replace('_', ' '), sessionId: e.session_id, execution: e,
      })
    }
  }
  for (const g of q.groups ?? []) {
    if (!g.seal_pending) continue
    out.push({ key: `s:${g.slug}`, kind: 'seal', since: g.last_active_ms, owner: g.title, title: `Seal it: ${g.seal_pending}`, sessionId: g.session_id, thread: g })
  }
  for (const { ask, heard_ms } of q.asks ?? []) {
    out.push({ key: `a:${ask.ask_id}`, kind: 'ask', since: heard_ms, owner: ask.place ?? title(ask.session_id), title: ask.question, sessionId: ask.session_id, expiresAt: ask.expires_ms, ask })
  }
  return out.sort((a, b) => a.since - b.since || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0))
}

/** The ids a card may be named by that an item belongs to, most specific first: what the item is about (its question's
 *  parent, its failed execution, its proposed record, its approval's execution or task), then its session, which a
 *  seal names as its thread's head. The Board's one work set maps each item to the first card these name (theseus-odj0m);
 *  an item no card is named by is a row of its own. */
export function askKeysOf(i: QueueItem): string[] {
  const keys = [i.view?.parent, i.proposal?.record.id, i.confirm?.task?.task_id, i.sessionId, i.view?.id, i.confirm?.execution_id, i.execution?.execution_id]
  return keys.filter((k): k is string => !!k)
}

/** The Needs-you list's count in its own words: asks, not cards. The counter above it counts pieces of work (Q1 A), so
 *  the two numbers never claim to be the same thing. */
export const asksWord = (n: number): string => `${n} ${n === 1 ? 'ask' : 'asks'}`

/** The cursor after the queue changed: on the item it was on while that item is there (a new item never moves it);
 *  when its item left, the one that took its place, else the last; none on an empty queue. */
export function keepCursor(before: readonly string[], after: readonly QueueItem[], cursor: string | null): string | null {
  if (!after.length) return null
  if (cursor == null) return null
  if (after.some((i) => i.key === cursor)) return cursor
  // The items that were after it, in their old order: the first still here takes its place.
  const at = before.indexOf(cursor)
  const keys = new Set(after.map((i) => i.key))
  for (const k of at < 0 ? [] : before.slice(at + 1)) if (keys.has(k)) return k
  return after[after.length - 1].key
}

/** `tab` and `⇧tab`: the next or the previous item, round the end. */
export function step(items: readonly QueueItem[], cursor: string | null, by: 1 | -1): string | null {
  if (!items.length) return null
  const i = items.findIndex((x) => x.key === cursor)
  if (i < 0) return items[by > 0 ? 0 : items.length - 1].key
  return items[(i + by + items.length) % items.length].key
}

/** The keys new since `before`: the counter pulses once when the queue rises. */
export function arrived(before: readonly string[], after: readonly QueueItem[]): string[] {
  const was = new Set(before)
  return after.filter((i) => !was.has(i.key)).map((i) => i.key)
}

/** The work.answer params of a question's numbered choice (`1`-`6`): the choice's option id, `choice:N`, counted
 *  from one on the key and from zero on the wire. Null when the question has no such choice. */
export function choiceOption(item: QueueItem, n: number): string | null {
  const choices = item.view?.question?.choices ?? []
  return n >= 1 && n <= Math.min(6, choices.length) ? `choice:${n - 1}` : null
}

/** What is due in the next three hours, soonest first: the wakes, routine runs, watches' checks and deferred starts
 *  the board holds (the calendar's events join it in a later row). */
export function comingUp(views: readonly WorkView[], now: number, within = 3 * 3_600_000): WorkView[] {
  return views.filter((v) => v.next_at_ms != null && v.next_at_ms >= now && v.next_at_ms <= now + within && !v.paused)
    .sort((a, b) => (a.next_at_ms ?? 0) - (b.next_at_ms ?? 0) || (a.id < b.id ? -1 : 1))
}

/** The item a selection names: by its key, its question, its view, its session or its thread. */
export function itemFor(items: readonly QueueItem[], sel: string | null): QueueItem | undefined {
  if (!sel) return undefined
  return items.find((i) => i.key === sel || i.key.slice(2) === sel || i.view?.id === sel || i.thread?.slug === sel)
    ?? items.find((i) => i.sessionId === sel)
}

/** The grant's key, as the TUI reads it (`GrantOffer::key`): `e` for edits and a mode, `a` for a command. */
export const grantKey = (c: Pick<ConfirmRequest, 'grant_offer'>): 'a' | 'e' | null =>
  !c.grant_offer ? null : c.grant_offer.kind === 'edits' || c.grant_offer.kind === 'mode' ? 'e' : 'a'

export type Answer =
  | { verb: 'approve' | 'grant' }
  | { verb: 'decline'; note?: string }
  | { verb: 'choice'; n: number; note?: string }
  | { verb: 'retry' | 'dismiss' | 'seal' | 'seal_dismiss' }
  | { verb: 'thread_answer'; key: string }

/** One answer to an item as the one protocol call that makes it, with the question to confirm first where the page
 *  asks one; null when the item has no such answer (a retry of a conversation). Every question is `work.answer`, as every surface answers it. */
export function answerOf(i: QueueItem, a: Answer): { method: string; params: Record<string, unknown>; ask?: string } | null {
  const qid = i.confirm?.correlation_id ?? i.proposal?.record.proposal?.card ?? i.view?.question?.id
  switch (a.verb) {
    case 'approve': {
      if (!qid || i.kind === 'question') return null
      const yes = i.confirm ? (i.confirm.options?.[0]?.id ?? (i.confirm.budget ? 'reset' : i.confirm.change ? 'accept' : i.confirm.tool === 'extend.ack' ? 'ack' : 'approve')) : 'accept'
      return { method: 'work.answer', params: { question_id: qid, option: yes } }
    }
    case 'grant':
      return i.confirm?.grant_offer ? { method: 'work.answer', params: { question_id: qid, option: 'approve_grant' } } : null
    case 'decline':
      if (!qid || i.kind === 'question') return null
      return { method: 'work.answer', params: { question_id: qid, option: 'decline', note: a.note?.trim() || undefined } }
    case 'choice': {
      const option = a.n === 0 ? 'choice:other' : choiceOption(i, a.n)
      if (!option || !qid) return null
      return { method: 'work.answer', params: { question_id: qid, option, note: a.n === 0 ? a.note?.trim() || undefined : undefined } }
    }
    case 'retry': {
      const exe = i.view?.id ?? i.execution?.execution_id
      return i.kind === 'failure' && exe && (i.view?.kind ?? i.execution?.kind) === 'task'
        ? { method: 'task.retry', params: { execution_id: exe }, ask: `Try ${i.owner} again? A new task starts from its brief.` } : null
    }
    case 'dismiss': {
      const exe = i.view?.id ?? i.execution?.execution_id
      return i.kind === 'failure' && exe ? { method: 'execution.dismiss', params: { execution_id: exe } } : null
    }
    case 'seal':
      return i.thread ? { method: 'thread.seal', params: { slug: i.thread.slug, reason: i.thread.seal_pending }, ask: `Seal ${i.thread.title}? Its notes stop reaching any other thread’s recall.` } : null
    case 'seal_dismiss':
      return i.thread ? { method: 'thread.seal_dismiss', params: { slug: i.thread.slug } } : null
    case 'thread_answer':
      return i.ask ? { method: 'thread.answer', params: { ask_id: i.ask.ask_id, choice: a.key } } : null
  }
}
