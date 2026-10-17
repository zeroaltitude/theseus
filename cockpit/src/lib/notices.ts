// Desktop notices (theseus-go4ig): the daemon runs the notification policy (`theseus_protocol::notices::policy`) on the
// work board and pushes each notice as `work.notice` on the page's `work.watch`, as it does for the chat and the TUI.
// The page only decides how a notice reaches the owner (`deliver`, a port of the Rust function of that name) and makes
// the notice of a set reminder's `wake.noticed`, which every client makes for itself. Tested by test/notices.test.ts.
import type { Delivery, Viewer, WakeNoticed, WorkNotice, WorkRef } from '@protocol'

/** A set reminder's notice (`reminder_notice`, purpose `remind`): an interrupt naming its note's first line. A model's
 *  own check-ins and routines are the digest's, and the page does not ping for them. */
export function reminderNotice(n: WakeNoticed): WorkNotice | undefined {
  if (n.purpose !== 'remind') return undefined
  const title = n.note.split('\n')[0].trim()
  const work: WorkRef = { id: n.wake_id, kind: 'wake', session_id: n.session_id, title }
  const at = n.due_at_ms + n.late_ms
  return { urgency: 'interrupt', why: 'woke', work, root: work, line: `⏰ ${title}`, position: at, at_ms: at }
}

/** How a notice reaches a viewer (`notices::deliver`): nothing for a quiet notice, for the work in focus while the
 *  owner is present, or for what was seen (a reminder aside); a ping for an interrupt, silent for a finish or an
 *  overdue milestone; a badge for the rest. */
export function deliver(n: WorkNotice, v: Viewer): Delivery {
  if (n.urgency === 'quiet') return { as: 'nothing' }
  const inFocus = v.focused != null && (v.focused === n.work.id || v.focused === n.work.session_id)
  if (v.away_ms === 0 && inFocus) return { as: 'nothing' }
  if (v.seen > 0 && n.position <= v.seen && n.why !== 'reminder') return { as: 'nothing' }
  if (n.urgency === 'interrupt') return { as: 'ping', sound: v.sound && !v.quiet && n.why !== 'finished' && n.why !== 'overdue' }
  return { as: 'badge' }
}

/** A notice's desktop tag: its question's id, so the answer's retraction (`retracts`) closes it. */
export const tagOf = (n: WorkNotice): string => n.question ?? `${n.work.id}:${n.why}`
