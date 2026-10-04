// A notified call's `security.v1` score (M5 step 24): `risk 12% (shadow)` on its notice, from the `judge.scored`
// push as its judgment lands, or from the judgment's `judge.call` row once the sink has written it. Pure, with no
// import but the protocol's types, so `node --test` runs its test (`cockpit/test/scores.test.ts`) as it is.
import type { JudgeScored, LedgerEntry } from '@protocol'

type D = Record<string, any>

/** One call's score: `risky` as a whole percent, and the pack's mode. */
export interface Score { percent: number; mode: string; judgment: string }

/** The words a notice shows: `risk 12% (shadow)`, as the CLI and Discord say them. */
export const scoreWords = (s: Score) => `risk ${s.percent}% (${s.mode})`

/** Each judged call's score, by its correlation id: its row's (the record), else its push's. */
export function scoresOf(rows: LedgerEntry[] | undefined, pushes: { method: string; params: unknown }[]): Map<string, Score> {
  const out = new Map<string, Score>()
  for (const p of pushes) {
    if (p.method !== 'judge.scored') continue
    const s = p.params as JudgeScored
    if (s.correlation_id) out.set(s.correlation_id, { percent: s.percent, mode: s.mode, judgment: s.judgment })
  }
  for (const r of rows ?? []) {
    if (r.kind !== 'judge.call') continue
    const d = r.data as D
    const call = d?.context?.call
    if (d?.pack !== 'security.v1' || typeof call !== 'string' || d?.outcome?.outcome !== 'answered') continue
    const risky = (d.answers as D[] | undefined)?.find((a) => a.question === 'risky')?.band?.value
    if (typeof risky !== 'number') continue
    out.set(call, { percent: Math.round(Math.min(1, Math.max(0, risky)) * 100), mode: String(d.mode ?? 'shadow'), judgment: String(d.id ?? '') })
  }
  return out
}
