// External-text notices (theseus-g00vv): a session that read external text holds it, and what the hold does depends on
// the daemon's mode. Under `ask` a live session's calls that act wait for the owner (`external_waits`): that is an ask
// and the queue has it. Under `notify` nothing waits; the hold is a notice, which the owner may see, trust again or
// dismiss (`policy.notice_seen`, which only marks it seen), and which is never counted as needing them. Pure: `npm test`
// runs it (test/ops-notices.test.ts); the strip that draws it is `ops/Notices.tsx`.
//
// A dismissed notice leaves the strip and nothing else: the hold stays on the session, in the ledger, on the Actions page's
// holds and in the session deck, where Trust remains.
import type { ExternalText, SessionInfo } from '@protocol'

export interface Hold { session_id: string; title: string; external_text: ExternalText }

/** The execution states that mean a session's work ended: a hold on it asks nothing and says nothing. */
const ENDED_EXECUTION = ['complete', 'cancelled', 'failed', 'budget_exhausted']

const titleOfSession = (s: SessionInfo): string => s.title || s.label || s.session_id.slice(-6)

/** The holds that wait (the queue's), and the notices (the strip's, oldest first), from the sessions. A hold is read
 *  as waiting only when the daemon says so (`external_waits === true`); a told hold is neither; the hold of a session whose
 *  execution ended is neither. */
export function holdsOf(sessions: readonly SessionInfo[]): { waiting: Hold[]; notices: Hold[] } {
  const waiting: Hold[] = []
  const notices: Hold[] = []
  for (const s of sessions) {
    const x = s.external_text
    if (!x) continue
    const hold = { session_id: s.session_id, title: titleOfSession(s), external_text: x }
    if (s.external_waits === true) waiting.push(hold)
    else if (s.external_told === true || ENDED_EXECUTION.includes(s.execution_state ?? '')) continue
    else notices.push(hold)
  }
  return { waiting, notices: oldestFirst(notices) }
}

/** The notices in the time machine: the holds the fold carries, read-only. The fold knows what each session held, not the
 *  daemon's mode then nor whether the owner had been told, so none of them is read as an ask; those of a session
 *  that had ended (`endedSessions`) are left out, as now. */
export function pastNotices(holds: readonly Hold[], endedSessions: ReadonlySet<string>): Hold[] {
  return oldestFirst(holds.filter((h) => !endedSessions.has(h.session_id)))
}

const oldestFirst = (hs: Hold[]): Hold[] => hs.sort((a, b) => a.external_text.since_ms - b.external_text.since_ms || (a.session_id < b.session_id ? -1 : 1))

/** What the daemon does with a trust, in its words. */
export const TRUST_WORDS = 'its calls that act go back to their postures until it reads external text again; the read and the trust stay in the ledger'

const VIA: Readonly<Record<string, string>> = {
  quote: 'a quoted file', 'task.report': 'a task’s report', 'task.create': 'the session that started it', job: 'a job that session started',
  program: 'a listed program’s job', egress: 'an L1 job that reached out',
}

/** A notice in words: what was read, from where, which session, and what Trust does. `redact` masks what was read and the titles. */
export interface NoticeFacts {
  what: string
  tool: string
  nodeId: string
  /** How it came here: `via` in words, else "read here". */
  how: string
  fromSession?: string
  title: string
  sessionId: string
  since: number
  trusts: string
}

export function noticeFacts(h: Hold, redact = false): NoticeFacts {
  const x = h.external_text
  const mask = (s: string) => (redact ? '▒▒▒' : s)
  return {
    what: mask(x.query != null ? `“${x.query}”` : x.url), tool: x.tool, nodeId: x.node_id,
    how: x.via ? VIA[x.via] ?? x.via : 'read here', fromSession: x.from_session, title: mask(h.title), sessionId: h.session_id, since: x.since_ms, trusts: TRUST_WORDS,
  }
}

/** The one call that marks a notice seen. It writes the daemon's "told" mark and nothing else. */
export const seenCall = (h: Pick<Hold, 'session_id'>) => ({ method: 'policy.notice_seen', params: { session_id: h.session_id } })

/** The call and its question for "Trust again". */
export const trustCall = (h: Hold) => ({ method: 'policy.trust', params: { session_id: h.session_id }, ask: `Trust ${h.title} again? ${TRUST_WORDS}.` })

/** "Dismiss all from before notify (N)": the owner's click, one `policy.notice_seen` per session, after one question. */
export function dismissAll(notices: readonly Hold[]): { label: string; ask: string; calls: Array<ReturnType<typeof seenCall>> } {
  const ids = [...new Set(notices.map((n) => n.session_id))]
  return {
    label: `Dismiss all from before notify (${ids.length})`,
    ask: `Mark ${ids.length} notice${ids.length === 1 ? '' : 's'} seen? They leave this strip only: the holds stay on their sessions, in the ledger and on the Actions page.`,
    calls: ids.map((session_id) => seenCall({ session_id })),
  }
}
