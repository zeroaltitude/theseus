// Every act on the Operations page is one protocol call through `useAct` (confirmed first where it says so, the page's
// reads read again), and goes through `enabled`: under the time machine it is off, with the banner's words.
import { useAsOf } from '@/lib/timemachine'
import { useAct } from '@/components/ConfirmCard'
import { enabled, PAST_WORDS, type ButtonAct, type Verb } from './keys'

export { PAST_WORDS }

/** The page's acts: `run` refuses a verb `enabled` says is off. */
export function useOpsAct() {
  const past = useAsOf((s) => s.t !== null)
  const { busy, run } = useAct()
  const act = (verb: Verb | ButtonAct, key: string, method: string, params: unknown, ask?: string): Promise<boolean> =>
    enabled(verb, past) ? run(key, method, params, ask) : Promise.resolve(false)
  return { busy, act, past, on: (verb: Verb | ButtonAct) => enabled(verb, past) }
}
