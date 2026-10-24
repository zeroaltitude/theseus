// Every act on the Operations page is one protocol call through `useAct` (confirmed first where it says so, the page's
// reads read again), and goes through `enabled`: under the time machine it is off, with the banner's words.
import { useQueryClient } from '@tanstack/react-query'
import { useAsOf } from '@/lib/timemachine'
import { call } from '@/lib/rpc'
import { useAct } from '@/components/ConfirmCard'
import { enabled, type ButtonAct, type Verb } from './keys'
import { wordsOf } from './refusal'

export const PAST_WORDS = 'the past: acting needs the present (LIVE in the ship’s log below)'

/** What `send` answers: the call's result, or the daemon's refusal in its words (for a toast, not an alert). */
export type Sent<T> = { ok: true; result: T } | { ok: false; error: string }

/** The page's acts: `run` refuses a verb `enabled` says is off. `send` is the same gate for an act whose result the
 *  page needs (a drag's Undo reads `previous`), saying a refusal back instead of alerting it. `post` is the same call
 *  with its answer for an act that shows what the daemon said (a refusal in its words, an Undo): it rejects instead. */
export function useOpsAct() {
  const past = useAsOf((s) => s.t !== null)
  const qc = useQueryClient()
  const { busy, run } = useAct()
  const act = (verb: Verb | ButtonAct, key: string, method: string, params: unknown, ask?: string): Promise<boolean> =>
    enabled(verb, past) ? run(key, method, params, ask) : Promise.resolve(false)
  const send = async <T = unknown>(verb: Verb | ButtonAct, method: string, params: unknown): Promise<Sent<T>> => {
    if (!enabled(verb, past)) return { ok: false, error: PAST_WORDS }
    try { const result = await call<T>(method, params); void qc.invalidateQueries(); return { ok: true, result } } catch (e) { return { ok: false, error: wordsOf(e) } }
  }
  const post = <T,>(verb: Verb | ButtonAct, method: string, params: unknown): Promise<T> =>
    enabled(verb, past) ? call<T>(method, params) : Promise.reject(new Error(PAST_WORDS))
  return { busy, act, send, post, past, on: (verb: Verb | ButtonAct) => enabled(verb, past) }
}
