// The route footer and the correction layer in words (theseus-q31l): where routing ran a turn, the owner's one-tap
// corrections of it (`route.correct`, judged by the core as any surface's: the owner, from a private place), and
// the live layer `route.corrections` lists. Pure: `test/routefooter.test.ts` runs it.
import type { RouteCorrectionInfo, RouteCorrectionsResult, TurnRoute } from '@protocol'

/** `routed: quick · haiku`, `routed: correction · fable`, with where the session ran before routing when it moved;
 *  none when routing read nothing for the turn. */
export function routeLine(route: TurnRoute | undefined, profile: string): string | null {
  if (!route) return null
  const what = route.source === 'correction' ? 'correction' : (route.mode ?? route.reason)
  const from = route.from && route.from !== profile ? ` (from ${route.from})` : ''
  return `routed: ${what} · ${profile}${from}`
}

/** The footer's controls, as `route.correct`'s `to`: a stronger model, a cheaper one, then each other profile. */
export function correctionChoices(profiles: string[], ran: string): { to: string; text: string }[] {
  return [
    { to: 'stronger', text: '⬆ stronger' },
    { to: 'cheaper', text: '⬇ cheaper' },
    ...profiles.filter((p) => p !== ran).map((p) => ({ to: p, text: p })),
  ]
}

/** The layer's head: how many of its bound, under which pack, at what share of words in common. */
export function layerHead(r: RouteCorrectionsResult): string {
  const state = r.enabled ? 'on' : 'off'
  const retired = r.retired ? ` · ${r.retired} retired` : ''
  return `${state} · ${r.entries.length} of ${r.max_entries} under ${r.pack} · close at ${Math.round(r.similarity * 100)}% of words in common${retired}`
}

/** One entry: where a close message runs, and the corrected message's first words. */
export function correctionLine(e: RouteCorrectionInfo, words = 8): string {
  const shown = e.words.slice(0, words).join(' ')
  const more = e.words.length > words ? ` +${e.words.length - words}` : ''
  return `→ ${e.to} · ${shown}${more}`
}
