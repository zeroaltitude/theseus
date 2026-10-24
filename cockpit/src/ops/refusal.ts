// A refusal's words, for a toast or a banner (theseus-jid7t.5). Pure.

/** An RPC refusal's words: the client rejects with the daemon's `{ code, message }`, not an Error. */
export const wordsOf = (e: unknown): string => (typeof e === 'object' && e !== null && typeof (e as { message?: unknown }).message === 'string' ? (e as { message: string }).message : String(e))

/** The daemon's REFUSED code (`calendar.window` to a connection that is not the owner's, in a private place). */
export const REFUSED_CODE = -32005
/** Whether a failed read is the refusal, which no wait changes: the calendar read stops its re-reads on it. */
export const latches = (e: unknown): boolean => typeof e === 'object' && e !== null && (e as { code?: unknown }).code === REFUSED_CODE
