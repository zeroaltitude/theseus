// The activity strip's lines, folded (theseus-hnof.5). The strip at every page's foot merges the narrative's sentences
// with the ledger's rows, newest first. A busy daemon says the same thing over and over (Jev's shadow verdict on every
// probe, the dev page served again), and 500 copies of one line pushed every other line out. So lines that say the
// same thing fold into one, with a count: the same kind and the same words, numbers aside ("served the dev page 6×"
// and "… 8×" are one line). A fold sits where its newest line sits, says when the first one was, and keeps every line
// it folds, so a click lays them all out again: nothing is dropped, only stacked.
//
// Pure, with no import, so `node --test` runs it (test/activity.test.ts).

/** One line of the strip: a narrative sentence or a ledger row, as Shell's strip builds it. */
export interface StripLine { key: string; at: number; part: string; tone: string; session: string | null; text: string }

/** Lines that say one thing: the newest of them, how many, the first and last times, the sessions they came from
 *  (newest first), and every line, newest first. */
export interface Fold { line: StripLine; count: number; first: number; last: number; sessions: string[]; members: StripLine[] }

/** What makes two lines the same: their kind, and their words with every number read as one. */
export function foldKey(l: Pick<StripLine, 'part' | 'text'>): string {
  return `${l.part}\u0000${l.text.replace(/\d+(?:[.,:]\d+)*/g, '#')}`
}

/** The strip's lines (newest first) folded: one fold for every set of lines that say the same thing, at the place of
 *  its newest line. The counts add up to the lines given. */
export function foldRepeats(lines: readonly StripLine[]): Fold[] {
  const byKey = new Map<string, Fold>()
  const out: Fold[] = []
  for (const l of lines) {
    const k = foldKey(l)
    const f = byKey.get(k)
    if (!f) {
      const g: Fold = { line: l, count: 1, first: l.at, last: l.at, sessions: l.session ? [l.session] : [], members: [l] }
      byKey.set(k, g)
      out.push(g)
      continue
    }
    f.count++
    f.members.push(l)
    if (l.at < f.first) f.first = l.at
    if (l.at > f.last) f.last = l.at
    if (l.session && !f.sessions.includes(l.session)) f.sessions.push(l.session)
  }
  return out
}

/** The key the strip's state is kept under in this browser. */
export const STRIP_KEY = 'cockpit.activity'

/** Whether the strip opens: folded unless this browser kept it open (theseus-hnof.5: on the Ship and on the data
 *  pages alike, it starts folded, and it stays as it was left). */
export function stripOpen(kept: string | null | undefined): boolean {
  return kept === 'open'
}
