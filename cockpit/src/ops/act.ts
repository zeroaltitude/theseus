// Every act on the Operations page is one protocol call through `useAct` (confirmed first where it says so, the page's
// reads read again), and goes through `enabled`: under the time machine it is off, with the banner's words.
import { useQueryClient } from '@tanstack/react-query'
import { useAsOf } from '@/lib/timemachine'
import { call } from '@/lib/rpc'
import { useAct } from '@/components/ConfirmCard'
import { enabled, PAST_WORDS, type ButtonAct, type Verb } from './keys'
import { wordsOf } from './refusal'

export { PAST_WORDS }

/** One call's outcome (a drop, a drag and its Undo, an event's verb, one call of a bulk fan-out): done, with the
 *  daemon's answer, or refused in its words (for a toast, not an alert). */
export type Sent<T = unknown> = { ok: true; words: string; result: T } | { ok: false; words: string }

/** The page's acts: `run` refuses a verb `enabled` says is off. `send` is the same gate for an act whose answer the
 *  page shows (a drag's Undo reads `previous`; a bulk run counts its refusals): no confirm (the sheet asked once), no
 *  alert, and no read again: the caller calls `settle` once, after a fan-out's last call. `post` is `send` for an
 *  act that wants the answer or a rejection with the daemon's words. */
export function useOpsAct() {
  const past = useAsOf((s) => s.t !== null)
  const qc = useQueryClient()
  const { busy, run } = useAct()
  const act = (verb: Verb | ButtonAct, key: string, method: string, params: unknown, ask?: string): Promise<boolean> =>
    enabled(verb, past) ? run(key, method, params, ask) : Promise.resolve(false)
  const send = async <T = unknown>(verb: Verb | ButtonAct, method: string, params: unknown): Promise<Sent<T>> => {
    if (!enabled(verb, past)) return { ok: false, words: PAST_WORDS }
    try { return { ok: true, words: 'done', result: await call<T>(method, params) } } catch (e) { return { ok: false, words: wordsOf(e) } }
  }
  const post = async <T,>(verb: Verb | ButtonAct, method: string, params: unknown): Promise<T> => {
    const r = await send<T>(verb, method, params)
    if (!r.ok) throw new Error(r.words)
    return r.result
  }
  return { busy, act, send, post, settle: () => qc.invalidateQueries(), past, on: (verb: Verb | ButtonAct) => enabled(verb, past) }
}
