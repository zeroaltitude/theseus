// The notification policy for the page (theseus-go4ig): `theseus_protocol::notices`'s table, the rules the TUI and herdr
// already read, for the cockpit's desktop notices. Pure: no clock (the caller passes `at_ms` through the views) and no
// browser. It ports the rules the page can see (a question, its reminder, a failure or block, a flagged or long finish,
// a task's report, an answer's retraction, and a set reminder's `wake.noticed`) and `deliver`; the rest of the table
// (spend, forecast, wakes of a model's own) is the digest's, and informs at most, so the page does not ping for it.
// Tested by test/notices.test.ts, which holds the table: progress makes no notice.
import type { Question, WakeNoticed, Viewer, WorkRef, WorkState, WorkView } from '@protocol'

export type Urgency = 'quiet' | 'inform' | 'interrupt'
export type Why = 'asked' | 'reminder' | 'answered' | 'expired' | 'failed' | 'blocked' | 'finished' | 'reported' | 'woke'
export type Delivery = { as: 'ping'; sound: boolean } | { as: 'badge' } | { as: 'nothing' }

export interface Notice { urgency: Urgency; why: Why; work: WorkRef; line: string; question?: string; retracts?: string; position: number; at_ms: number }
export interface Rules { long_run_ms: number; remind_at: number; finished: 'flagged' | 'long_roots' | 'everything' }

/** The defaults of `Rules::default`. */
export const RULES: Rules = { long_run_ms: 300_000, remind_at: 0.5, finished: 'long_roots' }

const ENDED: WorkState[] = ['done', 'failed', 'cancelled', 'expired']
const ended = (s: WorkState) => ENDED.includes(s)

/** How people name a piece of work: its title, or its kind and the end of its id. */
export function nameOf(r: WorkRef): string {
  return r.title || `${r.kind} ${r.id.slice(-6)}`
}

/** A span as people say it: `40 s`, `14 min`, `2 h 5 min`. */
export function spanOf(ms: number): string {
  const s = Math.floor(ms / 1000)
  if (s < 60) return `${s} s`
  if (s < 3600) return `${Math.floor(s / 60)} min`
  const m = Math.floor((s % 3600) / 60)
  return m === 0 ? `${Math.floor(s / 3600)} h` : `${Math.floor(s / 3600)} h ${m} min`
}

const ref = (v: WorkView): WorkRef => ({ id: v.id, kind: v.kind, session_id: v.session_id, title: v.title })
const openQuestion = (v?: WorkView): Question | undefined => (v && !ended(v.state) ? v.question : undefined)
const subject = (q: Question, who: string) => (q.tool ? `${who}'s ${q.tool}` : `${who}'s question`)
const asker = (v: WorkView, q: Question) => nameOf(v.kind === 'question' ? q.asked_by : ref(v))

/** When a question's reminder is due: `remind_at` of its life after it was asked; none if it never expires. */
export function remindDue(q: Question, sinceMs: number, rules: Rules = RULES): number | undefined {
  if (!q.expires_at_ms) return undefined
  const asked = q.asked_at_ms > 0 ? q.asked_at_ms : sinceMs
  return asked + Math.floor((q.expires_at_ms - asked) * rules.remind_at)
}

/** The view's reminder time, if its open question has one still ahead. */
export function nextReminder(v: WorkView, rules: Rules = RULES): number | undefined {
  const q = openQuestion(v)
  const at = q && remindDue(q, v.state_since_ms, rules)
  return at !== undefined && at > v.at_ms ? at : undefined
}

function askedLine(v: WorkView, q: Question, hm: (ms: number) => string): string {
  const who = asker(v, q)
  const line = q.kind === 'budget'
    ? `● ${who} reached its spend limit: reset it?`
    : `● ${who} asks: ${q.prompt.replace(/\?+$/, '')}?`
  return q.expires_at_ms ? `${line} · expires ${hm(q.expires_at_ms)}` : line
}

/** The notification policy, first match wins: a question appears (1), its reminder (2), a failure or block (3), a root
 *  the owner flagged or that worked long goes to rest (5), an answer (6), a task's report (8). Anything else, progress
 *  above all, is no notice. */
export function policy(prev: WorkView | undefined, next: WorkView, rules: Rules, hm: (ms: number) => string): Notice | undefined {
  const prevQ = openQuestion(prev)
  const nextQ = openQuestion(next)
  const gone = prevQ && nextQ?.id !== prevQ.id ? prevQ : undefined
  const name = nameOf(ref(next))
  const say = (urgency: Urgency, why: Why, line: string, question?: Question): Notice =>
    ({ urgency, why, work: ref(next), line, question: question?.id, retracts: gone?.id, position: next.position, at_ms: next.at_ms })

  if (nextQ && prevQ?.id !== nextQ.id) return say('interrupt', 'asked', askedLine(next, nextQ, hm), nextQ)
  if (prev && nextQ) {
    const due = remindDue(nextQ, next.state_since_ms, rules)
    const expires = nextQ.expires_at_ms ?? Infinity
    if (due !== undefined && prev.at_ms < due && due <= next.at_ms && next.at_ms < expires) {
      const left = Math.ceil((expires - next.at_ms) / 60_000)
      return say('interrupt', 'reminder', `● still waiting, ${left} min left: ${subject(nextQ, asker(next, nextQ))}`, nextQ)
    }
  }
  const blocked = (v: WorkView, q?: Question) => v.state === 'needs_you' && !q && v.doing?.what !== 'asking'
  if (next.state === 'failed' && prev?.state !== 'failed' && next.kind !== 'job' && next.kind !== 'system') {
    return say('interrupt', 'failed', `✗ ${name} ${next.attention.label || 'failed'}`)
  }
  if (blocked(next, nextQ) && !(prev && blocked(prev, prevQ))) {
    return say('interrupt', 'blocked', `✗ ${name} ${next.attention.label || 'blocked'}`)
  }
  const root = !next.parent && (next.kind === 'conversation' || next.kind === 'task')
  if (prev && root && prev.attention.level === 'working' && (next.state === 'ready' || next.state === 'done')) {
    const ran = Math.max(0, next.at_ms - prev.attention.since_ms)
    const flagged = !!(next.flagged || prev.flagged)
    const loud = rules.finished === 'everything' || flagged || (rules.finished === 'long_roots' && ran >= rules.long_run_ms)
    const word = next.state === 'ready' ? 'ready' : 'done'
    if (loud) return say('interrupt', 'finished', `✓ ${name} is ${word} after ${spanOf(ran)}`)
    if (!gone) return say('inform', 'finished', `✓ ${name} is ${word}`)
  }
  if (gone) {
    const expired = next.state === 'expired' || (!!gone.expires_at_ms && next.at_ms >= gone.expires_at_ms)
    const who = asker(prev ?? next, gone)
    return expired
      ? say('inform', 'expired', `⌛ ${subject(gone, who)} expired, ${gone.on_expiry ?? 'not run'}`, gone)
      : say('quiet', 'answered', `✓ answered: ${subject(gone, who)}`, gone)
  }
  if (prev && next.kind === 'task' && ended(next.state)) {
    const r = next.report
    const fresh = r && JSON.stringify(prev.report) !== JSON.stringify(r)
    if (fresh || (!ended(prev.state) && next.state === 'done')) {
      const words = r ? ` reported: ${r.first_line ?? r.reason ?? r.outcome}` : ' reported'
      return say(next.flagged || prev.flagged ? 'interrupt' : 'inform', 'reported', `📋 ${name}${fresh ? words : ' reported'}`)
    }
  }
  return undefined
}

/** A set reminder that fired (`wake.noticed` with purpose `remind`): the owner asked for it, so it interrupts. Other
 *  wakes (a model's check-in, a routine) are the digest's. */
export function reminderNotice(n: WakeNoticed): Notice | undefined {
  if (n.purpose !== 'remind') return undefined
  const title = n.note.split('\n')[0].trim()
  const at = n.due_at_ms + n.late_ms
  return { urgency: 'interrupt', why: 'woke', work: { id: n.wake_id, kind: 'wake', session_id: n.session_id, title }, line: `⏰ ${title}`, position: at, at_ms: at }
}

/** How a notice reaches a viewer: nothing for the work in focus while present, for what was seen (a reminder aside) or
 *  for a quiet notice; a badge for an inform; a ping for an interrupt, silent for a finish. */
export function deliver(n: Notice, v: Viewer): Delivery {
  if (n.urgency === 'quiet') return { as: 'nothing' }
  const inFocus = v.focused != null && (v.focused === n.work.id || v.focused === n.work.session_id)
  if (v.away_ms === 0 && inFocus) return { as: 'nothing' }
  if (v.seen > 0 && n.position <= v.seen && n.why !== 'reminder') return { as: 'nothing' }
  if (n.urgency === 'interrupt') return { as: 'ping', sound: v.sound && !v.quiet && n.why !== 'finished' }
  return { as: 'badge' }
}

/** What the page keeps between pushes: each view as last seen, so a change is read against the one before. */
export class Notifier {
  private held = new Map<string, WorkView>()

  /** The first snapshot: the state the page opened on is no change. */
  seed(views: WorkView[]) {
    for (const v of views) this.held.set(v.id, v)
  }

  /** A view changed: its notice, if the table gives one. A view not newer than the one held is ignored (the position rule). */
  observe(v: WorkView, rules: Rules, hm: (ms: number) => string): Notice | undefined {
    const prev = this.held.get(v.id)
    if (prev && v.position <= prev.position) return undefined
    this.held.set(v.id, v)
    return policy(prev, v, rules, hm)
  }

  forget(ids: string[]) {
    for (const id of ids) this.held.delete(id)
  }

  /** The earliest reminder still ahead among the held views. */
  nextDue(rules: Rules): number | undefined {
    let best: number | undefined
    for (const v of this.held.values()) {
      const at = nextReminder(v, rules)
      if (at !== undefined && (best === undefined || at < best)) best = at
    }
    return best
  }

  /** Reminders due by `now`: each view read against itself as of its time, once (the held copy moves on). */
  due(now: number, rules: Rules, hm: (ms: number) => string): Notice[] {
    const out: Notice[] = []
    for (const v of [...this.held.values()]) {
      const at = nextReminder(v, rules)
      if (at === undefined || at > now) continue
      const later = { ...v, at_ms: at }
      const n = policy(v, later, rules, hm)
      this.held.set(v.id, later)
      if (n) out.push(n)
    }
    return out
  }
}
