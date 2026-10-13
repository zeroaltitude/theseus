// Health's imported sessions in words (theseus-revl), as `theseus health` says them: the sessions an import holds and the
// ones it erased, apart from the owner's own `sessions`. Pure, with no import but the protocol's types, so `node --test`
// runs its test.
import type { Health } from '@protocol'

const fmt = (n: number): string => n.toLocaleString('en-US')

/** `21,151 · erased 3`, or null when no import holds or has erased a session (an older daemon sends neither). */
export function importedWords(h: Pick<Health, 'imported'>): string | null {
  const held = h.imported?.sessions ?? 0
  const erased = h.imported?.erased ?? 0
  if (held === 0 && erased === 0) return null
  return [fmt(held), ...(erased > 0 ? [`erased ${fmt(erased)}`] : [])].join(' · ')
}

const usd = (n: number): string => `$${n.toFixed(2)}`

/** The OpenClaw sync's words (theseus-pjmj7), as `theseus health` says them: the last run's counts and the next run, with
 * the daemon's warning for a failed or a missed night. Null before a run. `when` writes a unix-ms time. */
export function syncWords(
  h: Pick<Health, 'imported'>,
  when: (ms: number) => string,
): { line: string; warn: string | null } | null {
  const s = h.imported?.sync
  if (!s) return null
  return {
    line: `last ran ${when(s.at_ms)} · ${s.sessions_new} new, ${s.sessions_appended} appended, ${fmt(s.messages)} messages, ${s.rejected} refused, people ${usd(s.people_usd)} · next ${when(s.next_ms)}`,
    warn: s.warn ?? null,
  }
}
