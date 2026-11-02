// How routing placed a turn, in words (M5 25e; the effort since route.v3, theseus-qe3v): a `route.decided` row read
// as the ledger's line and the turn view's chip. Pure, with no import but the protocol's types, so `node --test` runs
// its test (`test/route.test.ts`).
import type { LedgerEntry } from '@protocol'

type D = Record<string, any>

/** The word a surface adds when the turn's model was the operator's and not Jev's verdict (theseus-h66v4.3): the route's
 *  own `reason`, never the model compared with a default. The TypeScript mirror of `TurnRoute::pin_footer`
 *  (`crates/theseus-protocol/src/route.rs`), and the one place the cockpit decides it: the route footer and the turn chip
 *  both call it. */
export function pinFooter(reason: string | undefined): 'pinned' | null {
  return reason === 'pinned' ? 'pinned' : null
}

/** The mode in words (theseus-h66v4.3), the mirror of `TurnRoute::mode_words`: on a pinned turn the model was not routed,
 *  so the mode is Jev's shadow read (`Jev: quick`); on every other reason it is the bare mode. */
export function modeWords(reason: string | undefined, mode: string): string {
  return pinFooter(reason) ? `Jev: ${mode}` : mode
}

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
  return `Jev: ${t.effort}, ${(t.effortReason ?? '').replaceAll('_', ' ')}`.replace(/, $/, '')
}

/** The turn chip's head: Jev's mode, or `no verdict`; for a pinned turn `pinned`, with the mode worded as Jev's read
 *  (`pinned · Jev: quick`), since the model was not routed (theseus-h66v4.3). The effort follows in the chip. */
export function chipHead(t: TurnRouted): string {
  const pin = pinFooter(t.reason)
  if (!pin) return t.mode ?? 'no verdict'
  return t.mode ? `${pin} · ${modeWords(t.reason, t.mode)}` : pin
}

/** The ledger's line for a `route.decided` row: `chat → sonnet (verdict) · effort high (Jev) · waited 12 ms`. */
export function routeLine(r: LedgerEntry): string {
  const d = (r.data ?? {}) as D
  const t = routedOf(r)
  const where = t.profile && t.from && t.profile !== t.from ? `${t.from} → ${t.profile}` : t.profile ?? ''
  const head = `${t.mode ?? 'no verdict'} · ${where} (${t.reason})${d.detour ? ' · this message alone' : ''}`
  const effort = effortWords(t)
  const wait = typeof d.wait_ms === 'number' ? ` · waited ${d.wait_ms} ms${d.late ? ', late' : ''}` : ''
  // A running task's checkpoint (theseus-ypnxt.9), or a routed turn's (theseus-a1wvq): why it was asked, and what
  // a switch cost cold.
  const check = d.source === 'task checkpoint' || d.source === 'turn checkpoint' ? ` · ${d.source}: ${d.trigger ?? 'asked'}${d.cache_usd != null ? `, ${d.cold_tokens} cold tokens (~$${Number(d.cache_usd).toFixed(4)})` : ''}` : ''
  return `${head}${effort ? ` · ${effort}` : ''}${check}${wait}`
}
