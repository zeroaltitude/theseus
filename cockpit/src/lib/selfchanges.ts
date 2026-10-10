// The "What Theseus changed about itself" card's pure parts (theseus-pw1q.4): the kill switch's state in words, a
// row's numbers in a line, and how a row of the log reads.
import type { SelfLogRow, SelfState } from '@protocol'

/** The switch and the mode, as the card's pill says them, and the pill's tone. */
export function stateWords(s: SelfState): { label: string; tone: 'idle' | 'wait' | 'ok' } {
  if (s.mode === 'off') return { label: 'off', tone: 'idle' }
  if (s.halted) return { label: s.never_resumed ? 'halted (never resumed)' : 'halted', tone: 'wait' }
  return { label: 'running', tone: 'ok' }
}

/** What the switch says, in the CLI's words: who moved it and why. */
export function switchLine(s: SelfState): string {
  const mode = s.mode === 'off' ? 'mode off: nothing self-directed runs' : 'mode act'
  if (s.halted && s.never_resumed) return `${mode}; halted until the owner's first resume`
  const who = s.by ?? '?'
  const why = s.why ? `: ${s.why}` : ''
  return s.halted ? `${mode}; halted by ${who}${why}` : `${mode}; released by ${who}`
}

/** A row's numbers in one line: `cost_usd 1.25 · fixed 4`; empty when it carries none. */
export function numbersLine(n: unknown): string {
  if (n === null || n === undefined) return ''
  if (typeof n !== 'object') return String(n)
  return Object.entries(n as Record<string, unknown>)
    .map(([k, v]) => `${k} ${typeof v === 'object' ? JSON.stringify(v) : String(v)}`)
    .join(' · ')
}

/** A row's kind without its family's prefix when it is a self step's: `joined`, `pack.mode`. */
export function kindWords(r: Pick<SelfLogRow, 'kind'>): string {
  return r.kind.startsWith('self.') ? r.kind.slice('self.'.length) : r.kind
}
