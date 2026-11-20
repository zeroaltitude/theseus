// What the Ship reads, and when (GitHub issue 37). Pure but for the reader's one `call`: `npm test` runs it
// (test/ship-readplan.test.ts).
//
// - The Ship always opens on Live, and reads the nodes of the live ships only. A ship at rest is read when its state
//   is chosen: its newest `STATE_READ_N` by last activity, and the rest only when "Load all" is pressed.
// - A conversation that waits on the operator or works is busy, and a busy session reads as live (`busyOf`), so a
//   waiting question never leaves the sea because its conversation is old.
// - What is drawn is what was read: `drawnIds` keeps the ships whose nodes arrived, and the visitors (the selected
//   session, one the address flies to, a flaring ship).
import type { NodeInfo, SessionInfo } from '@protocol'
import { busyOf, shownIds, stateOf, type ShipFilter } from '../lib/sessionState.ts'

/** How many ships of a state at rest are read before "Load all": the newest by last activity. */
export const STATE_READ_N = 20

/** How many nodes of one session are read (`node.list`'s newest). */
export const SESSION_NODES_N = 2000

/** How many sessions are read at once. */
export const READ_PARALLEL = 4

/** The filter the Ship opens on, whatever the address says: the choice is the address's `st`, never kept. */
export function openAddress(search: URLSearchParams): { filter: ShipFilter; search: URLSearchParams; dropped: boolean } {
  if (!search.has('st')) return { filter: 'live', search, dropped: false }
  const next = new URLSearchParams(search)
  next.delete('st')
  return { filter: 'live', search: next, dropped: true }
}

export interface ReadPlan {
  /** The sessions whose nodes are read for this filter, newest first: the conversations and their tasks. */
  ids: string[]
  /** The conversations the filter shows. */
  total: number
  /** The conversations the plan leaves for "Load all". */
  rest: number
}

type Planned = Pick<SessionInfo, 'session_id' | 'kind' | 'state' | 'parent_session_id' | 'last_active_ms' | 'execution_state' | 'pending_confirms' | 'attention'>

/** The sessions to read under `filter`: every live one, and of the other states at most `STATE_READ_N` conversations
 *  (the newest) unless `all`. A task is read with the conversation that started it. */
export function readPlan(sessions: Planned[], filter: ShipFilter, all: boolean): ReadPlan {
  const lived = sessions.map((s) => ({ ...s, state: busyOf(s) ? ('live' as const) : stateOf(s) }))
  const shown = shownIds(lived, filter)
  const byId = new Map(lived.map((s) => [s.session_id, s]))
  const newest = (a: Planned, b: Planned) => b.last_active_ms - a.last_active_ms || a.session_id.localeCompare(b.session_id)
  const convs = lived.filter((s) => s.kind !== 'task' && shown.has(s.session_id))
  const sea = convs.filter((s) => s.state === 'live')
  const rest = convs.filter((s) => s.state !== 'live').sort(newest)
  const taken = all ? rest : rest.slice(0, STATE_READ_N)
  const chosen = new Set([...sea, ...taken].map((s) => s.session_id))
  const rootOf = (s: Planned): string => {
    let r = s
    for (let i = 0; i < 16 && r.parent_session_id; i++) {
      const p = byId.get(r.parent_session_id)
      if (!p) break
      r = p
    }
    return r.session_id
  }
  const ids = lived.filter((s) => shown.has(s.session_id) && chosen.has(s.kind === 'task' ? rootOf(s) : s.session_id)).sort(newest).map((s) => s.session_id)
  return { ids, total: convs.length, rest: rest.length - taken.length }
}

type Fleetish = Parameters<typeof shownIds>[0][number]

/** The sessions the Ship draws: those the filter shows whose nodes were read, and the visitors, read or not. */
export function drawnIds(sessions: Fleetish[], filter: ShipFilter, visitors: (string | null | undefined)[], read: ReadonlySet<string>): Set<string> {
  const out = new Set<string>()
  for (const id of shownIds(sessions, filter)) if (read.has(id)) out.add(id)
  // The visitors and the sessions that started them: `shownIds` with nothing matching names only these.
  for (const id of shownIds(sessions.map((s) => ({ ...s, state: 'retired' as const })), 'live', visitors)) out.add(id)
  return out
}

/** The count line when some ships of a state wait for "Load all": "20 of 312 quiet ships shown". */
export function readLine(word: string, total: number, rest: number): string | null {
  if (rest <= 0) return null
  return `${total - rest} of ${total} ${word ? `${word} ` : ''}ships shown`
}

export interface Reader {
  /** Read these sessions' nodes, four at a time in the order given; one already asked for is not read again. */
  enqueue(ids: string[]): void
  /** Stop: nothing more is started and nothing more is merged. */
  stop(): void
}

export interface ReaderOpts {
  call: (method: string, params: Record<string, unknown>) => Promise<{ nodes: NodeInfo[] }>
  merge: (sid: string, nodes: NodeInfo[]) => void
  /** How much of the work asked for so far is done, 0 to 1 (`null` when it is all done). */
  progress: (fraction: number | null) => void
  /** Nothing is queued or running. Called after every batch, and after an `enqueue` that had nothing to read. */
  idle: () => void
}

/** The page's one reader of nodes by session. */
export function createReader(o: ReaderOpts): Reader {
  const asked = new Set<string>()
  const queue: string[] = []
  let active = 0
  let total = 0
  let done = 0
  let gone = false
  const finish = () => {
    if (active > 0 || queue.length > 0) return
    total = 0
    done = 0
    o.progress(null)
    o.idle()
  }
  const pump = () => {
    while (!gone && active < READ_PARALLEL && queue.length > 0) {
      const sid = queue.shift()!
      active++
      o.call('node.list', { session_id: sid, kind: null, n: SESSION_NODES_N })
        .then((r) => { if (!gone) o.merge(sid, r.nodes) })
        .catch(() => { asked.delete(sid) }) // a session that went away: asked again when the plan next names it
        .finally(() => {
          active--
          done++
          if (gone) return
          o.progress(done / Math.max(1, total))
          pump()
          finish()
        })
    }
  }
  return {
    enqueue(ids) {
      if (gone) return
      for (const id of ids) {
        if (asked.has(id)) continue
        asked.add(id)
        queue.push(id)
        total++
      }
      pump()
      finish()
    },
    stop() { gone = true },
  }
}
