// The owner card's proposals on the Context page (theseus-slvl.6): pure parts only; the panel is
// `components/CardProposals.tsx`. A proposal is one question (Keep, Edit, Not this) that rests on imported episodes.
import type { CardListResult, CardProposal } from '@protocol'

/** The words under a question: how many sources, the kinds of evidence, and when it asks until. */
export function restsOn(p: CardProposal): string {
  const kinds = [...new Set(p.sources.map((s) => s.kind))].sort()
  const n = p.support === 1 ? '1 source' : `${p.support} sources`
  return kinds.length ? `${n} · ${kinds.join(', ')}` : n
}

/** What an unanswered proposal says about its end: "asks until 2026-10-28" or "expires today". */
export function asksUntil(p: CardProposal, now: number): string {
  if (p.expires_ms <= now) return 'expires at the next read'
  const days = Math.ceil((p.expires_ms - now) / 86_400_000)
  return days <= 1 ? 'asks until tomorrow' : `asks for ${days} more days`
}

/** What the header line says: how many wait, how many were answered, and the job's spend. */
export function summary(r: CardListResult): string {
  const spend = `$${r.spent_today_usd.toFixed(2)} of $${r.day_usd.toFixed(2)} today, $${r.spent_total_usd.toFixed(2)} in all`
  const nightly = r.done ? 'the card is done: no nightly run' : r.every_days === 0 ? 'no nightly run' : `nightly at most every ${r.every_days} days`
  return `${r.open.length} waiting (at most ${r.max_open}) · ${r.kept_total} kept · ${r.declined_total} declined · ${spend} · ${nightly}`
}

/** What Not this asks first. */
export function declineQuestion(p: CardProposal): string {
  return `Not this: “${p.text}”? It will not be proposed again for its ${p.sources.length} sources.`
}

/** An answered proposal as a line: the words kept (or proposed), and how it ended. */
export function answeredLine(p: CardProposal): string {
  const how = p.state === 'kept' ? (p.kept_text ? 'kept, edited' : 'kept') : p.answered_by === 'expiry' ? 'declined, expired' : 'declined'
  return `${p.kept_text ?? p.text} — ${how}`
}
