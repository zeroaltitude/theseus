// What each Operations card shows, field by field (theseus-jid7t.3). The cards render these and nothing else, so the
// parity test (test/ops-parity.test.ts) can hold every field the old pages showed to a function that makes it. Pure:
// it imports only the protocol's types and other pure modules.
import type { ConfirmRequest, ThreadRow } from '@protocol'
import { progressLine } from '../lib/progressLine.ts'
import { ago, until, usd, when } from '../lib/figures.ts'
import { boardNeeds, details, historyLinks, isWarm, workingSetLines, type GroupedThread } from '../lib/threadboard.ts'
import { alive, type OpsRow } from './board.ts'
import { isEndedState } from './state.ts'
import type { QueueItem } from './queue.ts'

/** A board card: every field of the Tasks page's card (`TaskCard`), and the model chip. A field the row has no data
 *  for is absent. */
export interface CardFacts {
  /** `⏸ paused` while the owner holds it, else its state for the state pill. */
  pill: string
  paused: boolean
  title: string
  /** Its deck. */
  href?: string
  place?: string
  /** The raw report target, the place pill's tooltip. */
  placeTip?: string
  short: string
  model?: string
  progress?: string
  doing?: string
  /** An ended task's outcome when it is not plain done (`failed`, `cancelled`). */
  outcome?: string
  /** An ended task's report line, or `no report line`. */
  report?: string
  /** An ended task's spend. */
  cost?: string
  /** When it last changed, as `4m ago`. */
  activity: string
  waitsOn?: string
  /** The deck link's words: `open · join` while it lives, `open` after. */
  open?: string
  /** `Pause` or `Resume`, for a working task. */
  pause?: 'Pause' | 'Resume'
  /** `Stop`, for a living task. */
  stop?: string
  /** It needs you: its approvals and question show inline, answered in place. */
  approvals: boolean
  /** Its open question, with its choices. */
  question?: string
  /** When it is due, for a scheduled row. */
  due?: string
}

export function cardFacts(r: OpsRow, now: number): CardFacts {
  const v = r.view
  // Ended: in Done, or a failure still asking for someone (it shows under Needs you).
  const done = r.column === 'done' || (v != null && isEndedState(v.state))
  const living = alive(r) && !done
  const info = r.task?.info
  const cost = v?.cost ?? (info ? { spent_usd: info.spent_usd, limit_usd: info.limit_usd } : undefined)
  const q = v?.question
  const isTask = r.kind === 'task'
  return {
    pill: r.paused ? '⏸ paused' : r.state ?? '—',
    paused: r.paused,
    title: r.title,
    href: r.sessionId ? `/session/${r.sessionId}` : undefined,
    place: r.place,
    placeTip: r.task?.target ?? r.place,
    short: r.short,
    model: r.model,
    progress: living && v ? progressLine(v, now) : undefined,
    doing: living && r.column !== 'needs' ? v?.doing?.label : undefined,
    outcome: done && v?.report?.outcome && v.report.outcome !== 'done' ? v.report.outcome : undefined,
    report: done && isTask ? r.task?.reportLine ?? 'no report line' : undefined,
    cost: done && cost ? usd(cost.spent_usd) : undefined,
    activity: ago(r.activityMs, now),
    waitsOn: living && info?.waiting_on ? `waits on ${info.waiting_on}` : undefined,
    open: r.sessionId ? (living ? 'open · join' : 'open') : undefined,
    pause: isTask && living && r.column !== 'needs' && r.column !== 'scheduled' ? (r.paused ? 'Resume' : 'Pause') : undefined,
    stop: isTask && living ? 'Stop' : undefined,
    approvals: r.column === 'needs' && !done,
    question: r.column === 'needs' && q ? `${q.prompt}${q.choices?.length ? ` · ${q.choices.join(' · ')}` : ''}` : undefined,
    due: r.dueMs != null ? `due ${when(r.dueMs, now)}${r.dueMs > now ? `, ${until(r.dueMs, now)}` : ''}` : undefined,
  }
}

/** A Coming up row's two times: when it is (`20:00`, `tomorrow 08:00`) and how far off (`in 2h 46m`). */
export function comingTimes(at: number | null | undefined, now: number): { at: string; rel: string } {
  return { at: when(at, now), rel: until(at ?? now, now) }
}

/** A thread in the list: every field of the Threads page's group header. */
export interface ThreadFacts {
  sealed: boolean
  title: string
  href: string
  state: string
  /** `warm` or `cold`, with its tooltip; absent before the first message. */
  warmth?: 'warm' | 'cold'
  warmthTip?: string
  /** `● 2 need you`, and whether it counts on the board (a paused thread does not). */
  needs?: string
  needsCounted: boolean
  pausedTip?: string
  /** Last activity, context, today's dollars, persona. */
  details: string
  seal?: string
  /** Its open items: the attention pill, the title, the kind and its deck. */
  items: Array<{ id: string; label: string; title: string; kind: string; href?: string }>
}

export function threadFacts(t: GroupedThread, now: number): ThreadFacts {
  const g = t.group
  const warm = t.lastActiveMs === 0 ? undefined : isWarm(t.lastActiveMs, now)
  return {
    sealed: g.sealed === true,
    title: g.title,
    href: `/threads/${g.slug}`,
    state: g.state,
    warmth: warm === undefined ? undefined : warm ? 'warm' : 'cold',
    warmthTip: warm === undefined ? undefined : warm ? 'Its cache is warm: a message costs little.' : 'Its cache is cold: a message rewrites its context.',
    needs: t.needsYou > 0 ? `● ${t.needsYou} need${t.needsYou === 1 ? 's' : ''} you` : undefined,
    needsCounted: boardNeeds(t) > 0,
    pausedTip: g.state === 'paused' && t.needsYou > 0 ? 'Paused: not in the board’s count' : undefined,
    details: details(t, now).filter((d) => d !== g.state && d !== 'warm' && d !== 'cold').join(' · '),
    seal: g.seal_pending ? `● Seal recommended: ${g.seal_pending}` : undefined,
    items: t.items.map((v) => ({ id: v.id, label: v.attention.label, title: v.title || v.attention.label, kind: v.kind, href: v.session_id ? `/session/${v.session_id}` : undefined })),
  }
}

/** A thread's panel: every field of the thread's own page (`/threads/:slug`). */
export interface ThreadPanelFacts {
  title: string
  state: string
  goal?: string
  details?: string
  context?: string
  today: string
  sealPending?: string
  sealedBy?: string
  topics?: string
  links: Array<{ label: string; slug: string }>
  card: string[]
  workingSet: string[]
  openWork: Array<{ id: string; title: string }>
  /** The session whose recent turns it shows; none before the first message. */
  turnsOf?: string
}

export function threadPanelFacts(row: ThreadRow, mine: GroupedThread | undefined, now: number): ThreadPanelFacts {
  const t = row.thread
  return {
    title: t.sealed ? `🔒 ${t.title}` : t.title,
    state: t.state,
    goal: t.goal || undefined,
    details: mine ? details(mine, now).join(' · ') : undefined,
    context: row.context_tokens != null ? `${row.context_tokens.toLocaleString()} tokens in context` : undefined,
    today: `$${row.today_usd.toFixed(2)} today`,
    sealPending: t.seal_pending ? `● Seal recommended: ${t.seal_pending}` : undefined,
    sealedBy: t.sealed ? `Sealed by ${t.sealed.by}: ${t.sealed.reason}` : undefined,
    topics: t.topics?.length ? `Topics: ${t.topics.join(', ')}` : undefined,
    links: historyLinks(t),
    card: t.card ?? [],
    workingSet: workingSetLines(t.working_set ?? {}),
    openWork: (mine?.items ?? []).map((v) => ({ id: v.id, title: v.title || v.attention.label })),
    turnsOf: t.session_id,
  }
}

/** A queue item's compact card: the verb and what it does, whose it is, its age, and how much of its time is left. */
export interface ItemFacts {
  glyph: string
  kind: string
  title: string
  owner: string
  age: string
  /** The share of its time left, 0 to 1, for the expiry bar; absent when it does not expire. */
  left?: number
  /** The full card is `ConfirmCard`'s, every field. */
  confirm?: ConfirmRequest
}

/** Each kind's glyph and word: shape always goes with colour. */
export const ITEM_LOOK: Readonly<Record<QueueItem['kind'], { glyph: string; word: string; tone: 'wait' | 'fault' | 'think' | 'money' | 'live' }>> = {
  approval: { glyph: '◆', word: 'approval', tone: 'wait' },
  budget: { glyph: '$', word: 'budget', tone: 'money' },
  change: { glyph: '✎', word: 'task change', tone: 'think' },
  question: { glyph: '?', word: 'question', tone: 'wait' },
  failure: { glyph: '✗', word: 'failure', tone: 'fault' },
  seal: { glyph: '🔒', word: 'seal', tone: 'think' },
  ask: { glyph: '↪', word: 'which thread', tone: 'live' },
}

export function itemFacts(i: QueueItem, now: number): ItemFacts {
  const asked = i.confirm?.requested_at_ms ?? i.since
  const span = i.expiresAt && asked ? i.expiresAt - asked : 0
  return {
    glyph: ITEM_LOOK[i.kind].glyph,
    kind: ITEM_LOOK[i.kind].word,
    title: i.title,
    owner: i.owner,
    age: ago(i.since, now),
    left: span > 0 && i.expiresAt ? Math.max(0, Math.min(1, (i.expiresAt - now) / span)) : undefined,
    confirm: i.confirm,
  }
}
