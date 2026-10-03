// The time machine's world for the views: null while live; else the lists and gauges as they stood at the moment,
// folded from the shared ledger history (`timemachine.ts`). One folder for the page, so every view scrubbing at once
// reuses the same checkpoints, and a small cache of recent moments, so scrubbing back over a stretch folds nothing.
import { useMemo } from 'react'
import type { LedgerEntry, SessionInfo, TaskInfo } from '@protocol'
import { useRpc } from './rpc'
import { useHistoryRows } from './history'
import { Folder, useAsOf, worldAt, type World } from './timemachine'

/** The page's one folder and its recent worlds. A new ledger, or new live lists (they lend the fold their fixed
 *  fields), is a new world; moments in one second with the same rows behind them are the same one. */
class Worlds {
  private folder = new Folder()
  private rows: LedgerEntry[] | null = null
  private sessions: SessionInfo[] | null = null
  private tasks: TaskInfo[] | null = null
  private cache = new Map<string, World>()

  at(t: number, rows: LedgerEntry[], sessions: SessionInfo[], tasks: TaskInfo[] | null, calls: LedgerEntry[]): World {
    if (this.rows !== rows) {
      this.folder.update(rows)
      this.rows = rows
      this.cache.clear()
    }
    if (this.sessions !== sessions || this.tasks !== tasks) {
      this.sessions = sessions
      this.tasks = tasks
      this.cache.clear()
    }
    const key = `${this.folder.count(t)}:${Math.floor(t / 1000)}`
    const hit = this.cache.get(key)
    if (hit) return hit
    const w = worldAt(this.folder, t, { sessions, tasks: tasks ?? [] }, calls)
    if (this.cache.size > 96) this.cache.delete(this.cache.keys().next().value!)
    this.cache.set(key, w)
    return w
  }
}

const worlds = new Worlds()

/** The views that show the past when the time machine is set; the others show the present and say so. */
export const FOLDS = ['/ship', '/fleet', '/actions', '/money'] as const

/** The world at the time machine's moment, or null while it is live. */
export function useWorld(): World | null {
  const t = useAsOf((s) => s.t)
  const h = useHistoryRows(t === null)
  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 3000)
  const { data: tl } = useRpc<{ tasks: TaskInfo[] }>('task.list', {}, 3000)
  const calls = useMemo(() => h.rows.filter((r) => r.kind === 'provider.call'), [h.rows])
  return useMemo(() => (t === null || !sl ? null : worlds.at(t, h.rows, sl.sessions, tl?.tasks ?? null, calls)), [t, h.rows, sl, tl, calls])
}

/** Dev and bench builds: the fold at the present against the daemon's own lists. Every execution's state, every
 *  session's turns and cost, the questions waiting, and the holds should agree; each difference is listed. */
if (import.meta.env.DEV || import.meta.env.MODE === 'bench') {
  ;(window as unknown as { __timeMachine: unknown }).__timeMachine = {
    async checkNow() {
      const { call } = await import('./rpc')
      const { useLedgerHistory } = await import('./history')
      const rows = useLedgerHistory.getState().rows
      const [sl, el, cl, tl] = await Promise.all([
        call<{ sessions: SessionInfo[] }>('session.list'), call<{ executions: import('@protocol').ExecutionInfo[] }>('execution.list'),
        call<{ confirms: import('@protocol').ConfirmRequest[] }>('confirm.list'), call<{ tasks: TaskInfo[] }>('task.list', {}),
      ])
      const f = new Folder()
      f.update(rows)
      const w = worldAt(f, Date.now() + 1000, { sessions: sl.sessions, tasks: tl.tasks }, rows.filter((r) => r.kind === 'provider.call'))
      const diffs: string[] = []
      const we = new Map(w.executions.map((e) => [e.execution_id, e]))
      for (const e of el.executions) {
        const x = we.get(e.execution_id)
        if (!x) diffs.push(`execution ${e.execution_id}: not folded`)
        else if (x.state !== e.state) diffs.push(`execution ${e.execution_id}: folded ${x.state}, live ${e.state}`)
      }
      const ws = new Map(w.sessions.map((s) => [s.session_id, s]))
      for (const s of sl.sessions) {
        const x = ws.get(s.session_id)
        if (!x) { diffs.push(`session ${s.session_id}: not folded`); continue }
        if (x.turns !== s.turns) diffs.push(`session ${s.session_id}: turns folded ${x.turns}, live ${s.turns}`)
        if (Math.abs(x.cost_usd - s.cost_usd) > 0.0001) diffs.push(`session ${s.session_id}: cost folded ${x.cost_usd.toFixed(5)}, live ${s.cost_usd.toFixed(5)}`)
        if (!!x.external_text !== !!s.external_text) diffs.push(`session ${s.session_id}: hold folded ${!!x.external_text}, live ${!!s.external_text}`)
      }
      const wc = new Set(w.confirms.map((c) => c.correlation_id))
      for (const c of cl.confirms) if (!wc.has(c.correlation_id)) diffs.push(`confirm ${c.correlation_id}: not folded`)
      for (const c of w.confirms) if (!cl.confirms.some((x) => x.correlation_id === c.correlation_id)) diffs.push(`confirm ${c.correlation_id}: folded but not live`)
      return { rows: rows.length, executions: el.executions.length, sessions: sl.sessions.length, confirms: cl.confirms.length, diffs }
    },
    /** `worldAt` at `n` moments across the ledger, timed (ms): a fold from a checkpoint and the lists built. */
    async measure(n = 200) {
      const { call } = await import('./rpc')
      const { useLedgerHistory } = await import('./history')
      const [sl, tl] = await Promise.all([call<{ sessions: SessionInfo[] }>('session.list'), call<{ tasks: TaskInfo[] }>('task.list', {})])
      return measureFold(useLedgerHistory.getState().rows, sl.sessions, tl.tasks, n)
    },
  }
}

/** Fold timing, for the report's numbers: the time of `worldAt` at `n` moments across the ledger. */
export function measureFold(rows: LedgerEntry[], sessions: SessionInfo[], tasks: TaskInfo[], n = 200): { median: number; p95: number; max: number } {
  const f = new Folder()
  f.update(rows)
  const calls = rows.filter((r) => r.kind === 'provider.call')
  const t0 = rows.length ? rows[0].at_unix_ms : 0
  const t1 = rows.length ? rows[rows.length - 1].at_unix_ms : 0
  const times: number[] = []
  for (let i = 0; i < n; i++) {
    const t = t0 + ((t1 - t0) * ((i * 7919) % n)) / n
    const a = performance.now()
    worldAt(f, t, { sessions, tasks }, calls)
    times.push(performance.now() - a)
  }
  times.sort((a, b) => a - b)
  return { median: times[Math.floor(n / 2)], p95: times[Math.floor(n * 0.95)], max: times[n - 1] }
}
