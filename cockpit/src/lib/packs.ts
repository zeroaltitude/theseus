// The ladder as the cockpit reads it (M5 26a): health's pack lines, and a `pack.mode` row in words. Pure, so
// `npm test` runs it.

/** Health's judge line, `route.v1: live (owner: decision of 2026-10-04)`, as its pack and its mode's first word. */
export function packLine(line: string): { pack: string; mode: string; why: string } {
  const i = line.indexOf(': ')
  if (i < 0) return { pack: line, mode: '', why: '' }
  const rest = line.slice(i + 2)
  const rolled = rest.startsWith('rolled back')
  const mode = rolled ? 'rolled back' : rest.split(/[ (]/)[0]
  return { pack: line.slice(0, i), mode, why: rest.slice(mode.length).trim() }
}

type Row = {
  mode: string
  from: string
  share?: number
  who: string
  via: string
  why: string
  forced: boolean
  numbers?: string
  rule?: string
  words?: string
  declined: boolean
  report?: string
}

/** A mode with its share: `canary 0.2`, `rolled back`. */
export function modeWords(mode: string, share?: number | null): string {
  if (mode === 'canary' && share != null) return `canary ${Number.isInteger(share) ? share.toFixed(1) : share}`
  if (mode === 'rolled_back') return 'rolled back'
  return mode
}

/** One `pack.mode` row, as the CLI's `theseus packs` says it. */
export function rowWords(r: Row): string {
  let out = `${modeWords(r.from)} → ${modeWords(r.mode, r.share)} by ${r.who} (${r.via}): ${r.why}`
  if (r.declined) out += ' · declined, no mode written'
  if (r.forced) out += ` · forced${r.numbers ? `: ${r.numbers}` : ''}`
  if (r.report) out += ` · cites ${r.report}`
  if (r.rule && r.words) out += ` · ${r.rule}: ${r.words}`
  return out
}

/** A canary's share as typed: above 0 and at most 1, else null. */
export function share(text: string): number | null {
  const n = Number(text)
  return Number.isFinite(n) && n > 0 && n <= 1 ? n : null
}
