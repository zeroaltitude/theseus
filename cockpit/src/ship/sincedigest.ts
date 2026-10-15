// The sixth plate in live mode reads the daemon's `work.digest` (theseus-qkoi3), not its own copy of the ledger: what
// finished, failed, was answered, was cancelled or woke since you last looked, worded once by the protocol (the same
// words `theseus digest` and Discord's /digest say), and the questions waiting now. The time machine and a replay still
// fold the ledger at their moment (`sinceOf`): a row after the moment is never read, and the digest knows only "now".
//
// What the digest does not tell, the fold still does (sessions that started, what the model calls cost): the plate keeps
// those cells from the fold and takes the rest from the digest. Pure: `Watch.tsx` fetches it and draws the plate.
import type { DigestKind, DigestLine, WorkDigestResult } from '@protocol'
import type { Since } from './since.ts'
import { andList, count, LINES, span, type Tone, type WatchLine } from './watch.ts'

const TONE: Record<DigestKind, Tone> = { reported: 'ok', failed: 'fault', answered: 'wait', cancelled: 'idle', woke: 'live', reminder: 'wait' }
const TAG: Record<DigestKind, string> = { reported: 'reported', failed: 'failed', answered: 'answered', cancelled: 'cancelled', woke: 'woke', reminder: 'reminder' }
/** Failures first, then what you may want to know, then the rest: each newest first. */
const ORDER: DigestKind[] = ['failed', 'answered', 'reported', 'cancelled', 'reminder', 'woke']

/** How many things the lines of one kind stand for (a folded burst is several). */
export const total = (lines: readonly DigestLine[], kind: DigestKind) => lines.filter((l) => l.kind === kind).reduce((n, l) => n + (l.count ?? 1), 0)

/** The digest's lines inside the stretch, as the plate's lines: failures first, each kind newest first. */
export function digestLines(lines: readonly DigestLine[], until: number, now: number): WatchLine[] {
  const inside = lines.filter((l) => l.at_ms <= until)
  return [...inside]
    .sort((a, b) => ORDER.indexOf(a.kind) - ORDER.indexOf(b.kind) || b.at_ms - a.at_ms)
    .map((l) => ({
      id: `digest:${l.kind}:${l.work.id}:${l.at_ms}`,
      tag: TAG[l.kind],
      text: l.words,
      figure: span(Math.max(0, now - l.at_ms)),
      tone: TONE[l.kind],
      title: `${l.words}, ${span(Math.max(0, now - l.at_ms))} ago.`,
      to: l.work.session_id ? { session: l.work.session_id } : undefined,
    }))
}

/** The plate `base` (the fold's) told by `digest` instead: its lines, its counts of tasks, failures and questions, its
 *  caption and its tone. `now` is the moment; the digest is not used for any other. */
export function sinceFromDigest(base: Since, digest: WorkDigestResult, now: number): Since {
  const inside = digest.lines.filter((l) => l.at_ms <= base.until)
  const reported = total(inside, 'reported')
  const failed = total(inside, 'failed')
  const answered = total(inside, 'answered')
  const cancelled = total(inside, 'cancelled')
  const waking = total(inside, 'woke') + total(inside, 'reminder')
  const waiting = digest.questions.length
  const lines = digestLines(inside, base.until, now)
  const parts = [
    reported ? count(reported, 'report', 'reports') : '',
    failed ? count(failed, 'failure', 'failures') : '',
    answered ? count(answered, 'question answered', 'questions answered') : '',
    cancelled ? count(cancelled, 'cancel', 'cancels') : '',
    waking ? count(waking, 'wake', 'wakes') : '',
  ].filter(Boolean)
  const quiet = !parts.length || base.away <= 0
  const tally = base.tally.map((t) => {
    switch (t.part) {
      case 'tasks':
        return { ...t, value: String(reported + cancelled), word: 'finished', sub: cancelled ? `${cancelled} cancelled` : t.sub,
          title: `${count(reported, 'report', 'reports')} and ${count(cancelled, 'cancel', 'cancels')} while you were away.`, tone: (cancelled ? 'fault' : reported ? 'ok' : 'idle') as Tone }
      case 'wrong':
        return { ...t, value: String(failed), sub: failed ? 'went wrong' : 'all clear', title: `${count(failed, 'failure', 'failures')} while you were away.`, tone: (failed ? 'fault' : 'ok') as Tone }
      case 'questions':
        return { ...t, value: String(answered), word: 'answered', sub: waiting ? `${waiting} still ${waiting === 1 ? 'waits' : 'wait'}` : 'none waiting',
          title: `${count(answered, 'question', 'questions')} answered while you were away; ${waiting} still waiting.`, tone: (waiting || answered ? 'wait' : 'idle') as Tone }
      default:
        return t
    }
  })
  return {
    ...base,
    tasks: { started: base.tasks.started, ended: reported + cancelled, failed },
    failures: failed,
    questions: { came: base.questions.came, went: answered, missed: answered, waiting },
    tally,
    quiet,
    value: base.away > 0 ? span(base.away) : '—',
    caption: base.away <= 0 ? base.caption : quiet ? 'nothing happened while you were away' : `while you were away: ${andList(parts)}`,
    tone: failed ? 'fault' : waiting || answered ? 'wait' : quiet ? 'idle' : 'ok',
    pulse: `${base.since}|digest|${digest.to}|${parts.join(',')}`,
    lines: lines.slice(0, LINES),
    more: Math.max(0, lines.length - LINES),
  }
}

/** The plate at the moment: the digest's when it is live and the daemon answered, else the fold's. The time machine and
 *  a replay (`past`) never read the digest: it knows only now, and a row after the moment is never read. */
export function plateOf(fold: Since, digest: WorkDigestResult | null | undefined, past: boolean, now: number): Since {
  return digest && !past ? sinceFromDigest(fold, digest, now) : fold
}
