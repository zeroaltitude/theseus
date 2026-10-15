// What the Agenda page reads (theseus-p17fd): one `work.agenda` for the window shown and one `wake.list`, read again
// when the window changes or when the push (`work.changed`) says a wake, routine, watch, project or deferred task moved;
// the work board's views come from the same `work.watch` the Threads views keep (`useThreadBoard`); each routine's last
// runs are one `routine.runs` read, again only when its view's signature (`runsSignature`) changes. Nothing is polled:
// a page left alone makes no request (test/agenda.test.ts reads this file for a timer and for a refetch interval).
import { useEffect, useRef, useState } from 'react'
import type { AgendaResult, RoutineRunsResult, RunRow, WakeInfo, WakeListResult, WorkChanged, WorkView } from '@protocol'
import { client, useConn } from './rpc'
import { page, RUNS_SHOWN, runsSignature, touchesFuture } from './agenda'

/** The push's bursts are one read: a frame that moves three wakes tells three views at once. */
const DEBOUNCE_MS = 300

export interface Future { agenda: AgendaResult | null; wakes: WakeInfo[]; error: string | null }

/** `work.agenda` over `[from_ms, to_ms)` and `wake.list`, kept current by the push alone. */
export function useFuture(from_ms: number, to_ms: number, held: ReadonlySet<string>): Future {
  const [state, setState] = useState<Future>({ agenda: null, wakes: [], error: null })
  const open = useConn((s) => s.status === 'open')
  const heldRef = useRef(held)
  useEffect(() => { heldRef.current = held }, [held])
  useEffect(() => {
    if (!open) return
    let dead = false
    let timer: ReturnType<typeof setTimeout> | null = null
    const read = async () => {
      try {
        const [first, wl] = await Promise.all([
          client.call<AgendaResult>('work.agenda', { from_ms, to_ms }),
          client.call<WakeListResult>('wake.list', {}),
        ])
        // The daemon answers at most AGENDA_MAX lines. A busy series (a routine every five minutes) can fill them
        // before the week is out, so the rest is read from where the lines stopped, only when it says `more`.
        const agenda = await page(first, (from) => client.call<AgendaResult>('work.agenda', { from_ms: from, to_ms }))
        if (!dead) setState({ agenda, wakes: wl.wakes, error: null })
      } catch (e) {
        if (!dead) setState((s) => ({ ...s, error: e instanceof Error ? e.message : String(e) }))
      }
    }
    const later = () => {
      if (timer) return
      timer = setTimeout(() => { timer = null; void read() }, DEBOUNCE_MS)
    }
    void read()
    const off = client.onNotify((method, params) => {
      if (method === 'work.changed' && touchesFuture(params as WorkChanged, heldRef.current)) later()
      else if (method === 'events.lost') later()
    })
    return () => { dead = true; off(); if (timer) clearTimeout(timer) }
  }, [from_ms, to_ms, open])
  return state
}

/** The last runs of each routine: a `routine.runs` read when a routine appears and whenever its signature changes. */
export function useRoutineRuns(routines: readonly WorkView[]): Record<string, RunRow[]> {
  const [runs, setRuns] = useState<Record<string, RunRow[]>>({})
  const seen = useRef<Record<string, string>>({})
  const open = useConn((s) => s.status === 'open')
  useEffect(() => {
    if (!open) { seen.current = {}; return }
    for (const v of routines) {
      const sig = runsSignature(v)
      const id = v.session_id ?? v.id
      if (seen.current[id] === sig) continue
      seen.current[id] = sig
      client.call<RoutineRunsResult>('routine.runs', { id, limit: RUNS_SHOWN })
        .then((r) => setRuns((cur) => ({ ...cur, [id]: r.runs })))
        .catch(() => { delete seen.current[id] })
    }
  }, [routines, open])
  return runs
}
