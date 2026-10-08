// How routing placed a turn, in words (M5 25e; the effort since route.v3, theseus-qe3v): a `route.decided` row read
// as the ledger's line and the turn view's chip. Pure, with no import but the protocol's types, so `node --test` runs
// its test (`test/route.test.ts`).
import type { LedgerEntry } from '@protocol'

type D = Record<string, any>

/** What a `route.decided` row says of its turn: the mode Jev judged, why the turn ran where it ran, the effort Jev
 *  answered and why the turn ran at its effort, and the effort its requests carried when Jev's applied. */
export interface TurnRouted {
  mode?: string
  reason: string
  profile?: string
  from?: string
  effort?: string
  effortReason?: string
  /** Jev's effort was set on the requests (`applied`, or `clamped` to `[routing] effort_bounds`). */
  effortApplied?: string
}

/** A `route.decided` row's fields, as the turn view reads them. */
export function routedOf(r: LedgerEntry): TurnRouted {
  const d = (r.data ?? {}) as D
  const str = (v: unknown) => (typeof v === 'string' ? v : undefined)
  return {
    mode: str(d.mode), reason: str(d.reason) ?? '', profile: str(d.profile), from: str(d.from),
    effort: str(d.effort), effortReason: str(d.effort_reason),
    effortApplied: d.effort_applied === true ? str(d.effort_ran) : undefined,
  }
}

/** The effort in words: `effort max (Jev)`, `effort high (Jev; clamped from max)`, or Jev's answer and why it did not
 *  apply (`Jev: max, unsure`); nothing when Jev answered none. */
export function effortWords(t: TurnRouted): string {
  if (t.effortApplied) {
    return t.effortReason === 'clamped' ? `effort ${t.effortApplied} (Jev; clamped from ${t.effort})` : `effort ${t.effortApplied} (Jev)`
  }
  if (!t.effort) return ''
  return `Jev: ${t.effort}, ${(t.effortReason ?? '').replace('_', ' ')}`.replace(/, $/, '')
}

/** The ledger's line for a `route.decided` row: `chat → sonnet (verdict) · effort high (Jev) · waited 12 ms`. */
export function routeLine(r: LedgerEntry): string {
  const d = (r.data ?? {}) as D
  const t = routedOf(r)
  const where = t.profile && t.from && t.profile !== t.from ? `${t.from} → ${t.profile}` : t.profile ?? ''
  const head = `${t.mode ?? 'no verdict'} · ${where} (${t.reason})${d.detour ? ' · this message alone' : ''}`
  const effort = effortWords(t)
  const wait = typeof d.wait_ms === 'number' ? ` · waited ${d.wait_ms} ms${d.late ? ', late' : ''}` : ''
  return `${head}${effort ? ` · ${effort}` : ''}${wait}`
}
