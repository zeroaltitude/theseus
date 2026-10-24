// A refusal's words, for a toast or a banner (theseus-jid7t.5). Pure.

/** An RPC refusal's words: the client rejects with the daemon's `{ code, message }`, not an Error. */
export const wordsOf = (e: unknown): string => (typeof e === 'object' && e !== null && typeof (e as { message?: unknown }).message === 'string' ? (e as { message: string }).message : String(e))
