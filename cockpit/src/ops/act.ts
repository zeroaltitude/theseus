// Every act on the Operations page is one protocol call through `useAct` (confirmed first where it says so, the page's
// reads read again), and goes through `enabled`: under the time machine it is off, with the banner's words.
import { useQueryClient } from '@tanstack/react-query'
import { useAsOf } from '@/lib/timemachine'
import { call } from '@/lib/rpc'
import { useAct } from '@/components/ConfirmCard'
import { enabled, PAST_WORDS, type ButtonAct, type Verb } from './keys'

export { PAST_WORDS }

/** One call's outcome for a fan-out or a drop: done (with the daemon's answer), or refused in its words (the client
 *  rejects with the RPC error itself, `{ code, message }`). */
export interface Sent { ok: boolean; words: string; result?: unknown }

/** The page's acts: `run` refuses a verb `enabled` says is off. `send` is one call of a bulk act or a drop: the sheet
 *  asked once, so no confirm, and no alert (the results toast says each refusal); `settle` reads the page again once
 *  the run is done. */
export function useOpsAct() {
  const past = useAsOf((s) => s.t !== null)
  const qc = useQueryClient()
  const { busy, run } = useAct()
  const act = (verb: Verb | ButtonAct, key: string, method: string, params: unknown, ask?: string): Promise<boolean> =>
    enabled(verb, past) ? run(key, method, params, ask) : Promise.resolve(false)
  const send = async (verb: ButtonAct, method: string, params: unknown): Promise<Sent> => {
    if (!enabled(verb, past)) return { ok: false, words: PAST_WORDS }
    try { return { ok: true, words: 'done', result: await call(method, params) } } catch (e) { return { ok: false, words: (e as { message?: string } | null)?.message ?? String(e) } }
  }
  return { busy, act, send, settle: () => qc.invalidateQueries(), past, on: (verb: Verb | ButtonAct) => enabled(verb, past) }
}
