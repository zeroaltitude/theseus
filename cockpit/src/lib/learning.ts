// The learning ledger as the cockpit reads it (M5 25c): a report's day, and what a judgment's answer can be labeled.
// Pure, so `npm test` runs it.

type D = Record<string, any>

/** A local date as the core names a report's day: `2026-10-04`. */
export function localDay(d: Date): string {
  const p = (n: number) => String(n).padStart(2, '0')
  return `${d.getFullYear()}-${p(d.getMonth() + 1)}-${p(d.getDate())}`
}

/** What one answer's label buttons offer beside right and wrong: a yes-or-no question's true and false, and a
 *  choice's options, in its order. */
export function labelChoices(answer: D | null | undefined): { bools: boolean; options: string[] } {
  if (answer?.noul != null) return { bools: true, options: [] }
  if (answer?.choice != null) return { bools: false, options: (answer.probabilities ?? []).map((p: [string, number]) => p[0]) }
  return { bools: false, options: [] }
}
