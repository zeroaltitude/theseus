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
