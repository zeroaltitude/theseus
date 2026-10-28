// What a card may honestly say about a question's wait (theseus-lrc3m). Since theseus-v2dnc a call's question is held past
// its life, never expired: its `expires_at_ms` is the time of its first notice, not a deadline, though the live
// `confirm.requested` still carries it (the Discord card is drawn from the same request). So a card decides "held" by the
// question's kind, never by that field, and says how long it has waited instead of a deadline that has not passed. Pure: no DOM.
import type { ConfirmRequest } from '@protocol'

/** The questions that still expire at their life: an `owner_ask` applies its default, and an extension's ack and a pack's
 *  promotion are declined (the daemon's `held_past_life` is the complement of these and the budget question). */
const EXPIRING: readonly string[] = ['owner_ask', 'extend.ack', 'pack.promote']

/** Whether the question is held until it is answered: a call's, not a budget question (which says so itself) and not one
 *  that expires. */
export const isHeld = (c: ConfirmRequest): boolean => !c.budget && !EXPIRING.includes(c.tool)

/** The deadline a card may show, in epoch ms: none (0) for a held question, whatever `expires_at_ms` says. */
export const deadlineOf = (c: ConfirmRequest): number => (isHeld(c) ? 0 : c.expires_at_ms)

/** How long a held question has waited, as the card words it: "waiting 3 h", "waiting 12 min", "waiting 30 s". None for a
 *  question that is not held, which counts down to its deadline instead. */
export function waitedLabel(c: ConfirmRequest, now: number): string | null {
  if (!isHeld(c)) return null
  const ms = Math.max(0, now - c.requested_at_ms)
  const waited = ms >= 3_600_000 ? `${Math.round(ms / 3_600_000)} h` : ms >= 60_000 ? `${Math.round(ms / 60_000)} min` : `${Math.round(ms / 1000)} s`
  return `waiting ${waited}`
}
