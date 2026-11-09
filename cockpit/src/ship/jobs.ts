// Which jobs turn the gear (theseus-fdvz): the set `tool.started` and the action list agree is running.
//
// A stopped job sends `tool.ended` (status `cancelled`) at once, but the action list the page reads lags it: it is
// polled every 5 s when the fleet looks idle, and until that poll the job's action is still dispatched and unsettled,
// so a union with the list put the job straight back and the gear turned on for up to 5 s. The two push-side facts
// that a job is over, an end and a verified cancel, now outrank a stale page.

/** The action fields the set reads. */
export interface JobAction {
  correlation_id: string
  tool: string
  dispatched_at_ms?: number | null
  settled_at_ms?: number | null
}

/** A `tool.ended` that ends the job: the `background` placeholder says it went on, so it is not an end. */
export const endsJob = (status: unknown): boolean => status !== 'background'

/**
 * The running jobs: those `tool.started` said began (`live`) and the dispatched, unsettled `proc.run` actions, less
 * every job a push said ended (`ended`), a cancel verified gone (`cancelled`) or an action settled.
 */
export function jobsRunning(
  live: ReadonlySet<string>,
  ended: ReadonlySet<string>,
  cancelled: ReadonlyMap<string, number>,
  actions: readonly JobAction[],
): Set<string> {
  const run = new Set(live)
  for (const a of actions) {
    const over = !!a.settled_at_ms || ended.has(a.correlation_id) || cancelled.has(a.correlation_id)
    if (over) run.delete(a.correlation_id)
    else if (a.dispatched_at_ms && a.tool === 'proc.run') run.add(a.correlation_id)
  }
  for (const id of ended) run.delete(id)
  for (const id of cancelled.keys()) run.delete(id)
  return run
}
