// The ship's log's marks: what the timeline marks along its track, folded from the ledger in order. Pure, with no
// import but the protocol's types, so `node --test` runs its test (`cockpit/test/marks.test.ts`) as it is.
import type { LedgerEntry } from '@protocol'

type D = Record<string, any>

export type MarkKind = 'turn' | 'failed' | 'ask' | 'answer' | 'cancel' | 'start' | 'install' | 'crash' | 'stop'

export interface Mark { at: number; kind: MarkKind; label: string; session?: string | null }

const clip = (s: string, n = 60) => (s.length > n ? `${s.slice(0, n - 1)}…` : s)

/** A start's build, as its `server.started` row names it (theseus-9o5n): its version and commit, or null from a
 *  binary before builds were named. */
function buildOf(d: D): { key: string; word: string } | null {
  const b = d.build as D | undefined
  if (!b || typeof b.version !== 'string') return null
  const commit = typeof b.commit === 'string' ? b.commit : ''
  return { key: `${b.version}+${commit}`, word: `${b.version}${commit ? ` (${commit.slice(0, 7)})` : ''}` }
}

/** What the timeline marks: turns, approvals asked and answered, cancels and their verdicts, and the daemon's own
 *  starts and stops (a start with no stop before it followed a crash or a kill). A start whose build differs from
 *  the start before it is an install, a new binary, not a restart of the same one; a build named after starts that
 *  named none is one too, since the binary before it could not name one. */
export function marksOf(rows: LedgerEntry[]): Mark[] {
  const out: Mark[] = []
  let stopped = true
  let stopWhy = ''
  // The last start's build: undefined before the first start, null for a binary that named none.
  let build: { key: string; word: string } | null | undefined
  for (const r of rows) {
    const d = (r.data ?? {}) as D
    switch (r.kind) {
      case 'turn.started': out.push({ at: r.at_unix_ms, kind: 'turn', label: `turn on ${d.model ?? 'a model'}${d.continuation ? ' (continuation)' : ''}`, session: r.session_id }); break
      case 'turn.failed': out.push({ at: r.at_unix_ms, kind: 'failed', label: `turn failed: ${clip(String(d.reason ?? ''), 80)}`, session: r.session_id }); break
      case 'tool.confirm_requested': out.push({ at: r.at_unix_ms, kind: 'ask', label: `asked: ${d.tool ?? 'a call'}`, session: r.session_id ?? d.session_id }); break
      case 'budget.asked': out.push({ at: r.at_unix_ms, kind: 'ask', label: 'asked: a spend reset', session: r.session_id }); break
      case 'action.confirm_answered': out.push({ at: r.at_unix_ms, kind: 'answer', label: `${d.approved ? 'approved' : 'declined'} by ${d.by ?? 'someone'}${d.trust ? ' (and trusted)' : ''}`, session: r.session_id }); break
      case 'action.cancel_verified': out.push({ at: r.at_unix_ms, kind: 'cancel', label: `cancel verified: ${d.tool ?? ''} (${d.verified_by ?? ''}${typeof d.killed === 'number' ? `, ${d.killed} processes` : ''})`, session: r.session_id }); break
      case 'action.cancel_uncertain': out.push({ at: r.at_unix_ms, kind: 'cancel', label: `cancel not verified: ${d.tool ?? ''}`, session: r.session_id }); break
      case 'action.cancel_unsupported': out.push({ at: r.at_unix_ms, kind: 'cancel', label: `cancel unsupported: ${d.tool ?? ''}`, session: r.session_id }); break
      case 'server.stopping':
        stopped = true
        stopWhy = typeof d.signal === 'string' ? ` (${d.signal})` : ''
        out.push({ at: r.at_unix_ms, kind: 'stop', label: `the daemon stopped${stopWhy}` })
        break
      case 'server.started': {
        const now = buildOf(d)
        const after = stopped ? (stopWhy ? `, after a stop by ${stopWhy.trim().slice(1, -1)}` : '') : ', with no stop before it: a crash or a kill'
        if (build !== undefined && (now?.key ?? '') !== (build?.key ?? '')) {
          out.push({ at: r.at_unix_ms, kind: 'install', label: `the daemon started on a new build, ${now?.word ?? 'one that names none'}, after ${build?.word ?? 'one that named none'}${after}` })
        } else {
          out.push(stopped
            ? { at: r.at_unix_ms, kind: 'start', label: `the daemon started${after}` }
            : { at: r.at_unix_ms, kind: 'crash', label: `the daemon started${after}` })
        }
        build = now
        stopped = false
        stopWhy = ''
        break
      }
    }
  }
  return out
}
