// The Agenda's words for a mark: one line for the hover and the tooltip (theseus-p17fd). Kept out of the strip's component file.
import { clock, ms, usd } from './format'
import type { Mark } from './agenda'

/** What a mark is, in a line: the hover and the tooltip. */
export function markLine(m: Mark): string {
  const when = clock(m.at_ms)
  const what = m.kind === 'past' ? `${m.outcome === 'ok' ? '✓' : m.outcome === 'failed' ? '✗' : m.outcome === 'running' ? 'running' : 'skipped'} ${when}${m.duration_ms != null ? ` · ${ms(m.duration_ms)}` : ''}${m.cost_usd != null ? ` · ${usd(m.cost_usd, 3)}` : ''}`
    : m.kind === 'watch' ? `watch until ${m.end_ms ? clock(m.end_ms) : 'it is met'}`
    : m.kind === 'milestone' ? `milestone ${when}` : m.kind === 'start' ? `starts ${when}` : m.kind === 'run' ? `run ${m.occurrence ?? ''} ${when}` : `${m.kind === 'check' ? 'check-in' : 'reminder'} ${when}`
  return `${m.title} · ${what}${m.line ? `\n${m.line}` : ''}`
}
