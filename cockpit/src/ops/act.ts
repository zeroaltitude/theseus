// Every act on the Operations page is one protocol call through `useAct` (confirmed first where it says so, the page's
// reads read again), and goes through `enabled`: under the time machine it is off, with the banner's words.
import { call } from '@/lib/rpc'
import { useAsOf } from '@/lib/timemachine'
import { useAct } from '@/components/ConfirmCard'
import { enabled, type ButtonAct, type Verb } from './keys'

export const PAST_WORDS = 'the past: acting needs the present (LIVE in the ship’s log below)'

/** The page's acts: `run` refuses a verb `enabled` says is off. `post` is the same call with its answer: for an act
 *  that shows what the daemon said (a refusal in its words, an Undo), not an alert. */
export function useOpsAct() {
  const past = useAsOf((s) => s.t !== null)
  const { busy, run } = useAct()
  const act = (verb: Verb | ButtonAct, key: string, method: string, params: unknown, ask?: string): Promise<boolean> =>
    enabled(verb, past) ? run(key, method, params, ask) : Promise.resolve(false)
  const post = <T,>(verb: Verb | ButtonAct, method: string, params: unknown): Promise<T> =>
    enabled(verb, past) ? call<T>(method, params) : Promise.reject(new Error(PAST_WORDS))
  return { busy, act, post, past, on: (verb: Verb | ButtonAct) => enabled(verb, past) }
}
