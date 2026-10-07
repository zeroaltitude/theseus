// The flow of ledger rows (theseus-hnof.5): rows a second over the last two minutes, from the total's growth between
// polls of the one-row read. The heartbeat bar's planks and the activity strip's flow share it.
import { useMemo } from 'react'
import { useLedger } from './derive'
import { useHistory } from './hooks'

export function useFlow() {
  const { data: tail } = useLedger(1, 2000)
  const totals = useHistory(tail?.total, 60, 2000)
  const flow = useMemo(() => totals.slice(1).map((t, i) => Math.max(0, (t - totals[i]) / 2)), [totals])
  return { flow, total: tail?.total }
}
