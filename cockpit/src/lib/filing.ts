// The proposals panel's lock while it files (theseus-6o8q8): pure, so `npm test` runs it. A write of M calls is
// `filing` from its first call to its last or its first refusal; while it is, every control that picks or answers is off
// (the kind filters only change the view, so they stay on), and the header says how far it has got.

/** Where a write is: how many proposals it files and how many are done (a call that files several counts them all when
 *  it returns, the daemon giving no progress inside a call); `null` while nothing is filing. */
export type Filing = { done: number; total: number } | null

/** A write of `total` calls begins. */
export const begin = (total: number): Filing => ({ done: 0, total })

/** A call that filed `by` proposals is done (never past the total). */
export const advance = (f: Filing, by = 1): Filing => (f ? { done: Math.min(f.done + by, f.total), total: f.total } : f)

/** The write ends, done or refused: the lock clears. */
export const end = (): Filing => null

/** The pick controls (checkboxes, select all) are off while filing, or when they were off for their own reason. */
export const pickOff = (f: Filing, own = false): boolean => own || f !== null

/** The header's words: `filing 3 of 12` (a call at a time), or nothing while idle. */
export const filingWords = (f: Filing): string => (f ? `filing ${f.done} of ${f.total}` : '')
