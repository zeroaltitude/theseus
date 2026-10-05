// The cockpit's data layer (theseus-45n5): one protocol connection, reads through TanStack Query, and the pushes
// (the narrative, and the sessions being watched) collected in small stores.
//
// The fleet follows the push (theseus-in3): the connection watches every execution (`executions.watch`), and the
// reads the push keeps fresh (`PUSHED`) are read again when it says something changed, never on a timer. The other
// reads are still polled; the report of theseus-in3 lists them.
import { useEffect } from 'react'
import { useQuery, type QueryClient, type UseQueryOptions } from '@tanstack/react-query'
import { create } from 'zustand'
import { ProtocolClient, type NarrativeLine, type NarrativeWatchResult, type Status } from '@protocol'

// Served by the daemon, the page connects to its own address. The dev page (`npm run dev`) connects straight to the
// daemon named by THESEUS_DEV_DAEMON (vite.config.ts), never through a proxy, which would relay other pages and
// other users' processes (theseus-88im). That daemon's `[web] dev_origin` must name this page.
export const client = new ProtocolClient(
  import.meta.env.DEV ? `ws://${import.meta.env.VITE_THESEUS_DEV_DAEMON as string}/ws` : undefined,
)

// ---------------------------------------------------------------- the connection

interface ConnState {
  status: Status
  openedAt: number | null
  reconnects: number
  /** Round trips of recent calls, newest last (ms): the link's own latency. */
  rtts: number[]
}

export const useConn = create<ConnState>(() => ({ status: 'connecting', openedAt: null, reconnects: 0, rtts: [] }))

client.onStatus = (s) => {
  useConn.setState((st) => ({
    status: s,
    openedAt: s === 'open' ? Date.now() : st.openedAt,
    reconnects: s === 'open' && st.openedAt !== null ? st.reconnects + 1 : st.reconnects,
  }))
}

/** Calls that resolve when work ends, not when the daemon answers: never a link round trip. */
const LONG = new Set(['turn.submit', 'narrative.watch', 'session.watch'])

/** One call, timed: its round trip feeds the link-latency readout (not the long calls). */
export async function call<T = unknown>(method: string, params?: unknown): Promise<T> {
  const t0 = performance.now()
  const out = await client.call<T>(method, params)
  if (!LONG.has(method)) {
    const dt = performance.now() - t0
    useConn.setState((st) => ({ rtts: [...st.rtts.slice(-59), dt] }))
  }
  return out
}

/** The reads the push keeps fresh (theseus-in3): each execution's change, and each question's, says when to read
 * them again, so they are never polled. */
export const PUSHED = new Set(['session.list', 'execution.list', 'confirm.list', 'task.list', 'budget.list', 'policy.explain'])

/** Paused (the Observatory's live/paused, theseus-vm3n.6): no read runs on a timer and the ledger's follow waits, so
 * the views hold still to be read. The push still comes, as it did there. The heartbeat bar's refresh reads
 * everything once, paused or not. */
export const usePaused = create<{ paused: boolean }>(() => ({ paused: false }))

/** A read. `interval` in ms; 0 reads once. A read the push keeps fresh (`PUSHED`) ignores its interval: the push
 * reads it again. Disabled while the link is down, and polled only while not paused. */
export function useRpc<T>(
  method: string,
  params?: unknown,
  interval = 2000,
  opts?: Partial<UseQueryOptions<T>>,
) {
  const open = useConn((s) => s.status === 'open')
  const paused = usePaused((s) => s.paused)
  const pushed = PUSHED.has(method)
  return useQuery<T>({
    queryKey: [method, params ?? null],
    queryFn: () => call<T>(method, params),
    refetchInterval: !pushed && interval > 0 ? interval : false,
    enabled: open && (opts?.enabled ?? true),
    staleTime: pushed ? Infinity : interval > 0 ? interval / 2 : 30_000,
    ...opts,
    // A caller's own interval stops too.
    ...(paused ? { refetchInterval: false as const } : {}),
  })
}

/** Follow the push (theseus-in3): on every (re)connect watch every execution, and read the pushed reads again when
 * an execution or a question changes, at most every 250 ms. After `events.lost`, the same: everything is read again. */
export function bindPush(queries: QueryClient) {
  let timer: ReturnType<typeof setTimeout> | null = null
  const again = () => {
    if (timer) return
    timer = setTimeout(() => {
      timer = null
      void queries.invalidateQueries({ predicate: (q) => PUSHED.has(String(q.queryKey[0])) })
    }, 250)
  }
  client.onOpen(() => {
    client.call('executions.watch', { limit: 1 }).then(again).catch(() => {})
  })
  client.onNotify((method) => {
    if (method === 'execution.changed' || method === 'confirm.requested' || method === 'confirm.resolved' || method === 'events.lost'
      // A tightening, its undo, and a trust change what `policy.explain` says (42b); `health`'s list follows on its poll.
      || method === 'policy.tightened' || method === 'policy.untightened' || method === 'session.trusted') again()
  })
}

// ---------------------------------------------------------------- pushes

export interface PushEvent { seq: number; at: number; method: string; params: Record<string, unknown> }

let seq = 0
const MAX_EVENTS = 4000

interface PushState {
  /** Every notification since this page loaded, newest last (bounded). */
  events: PushEvent[]
  narrative: NarrativeLine[]
  narrativeOn: boolean | null
}

export const usePush = create<PushState>(() => ({ events: [], narrative: [], narrativeOn: null }))

client.onNotify((method, params) => {
  const ev: PushEvent = { seq: ++seq, at: Date.now(), method, params: (params ?? {}) as Record<string, unknown> }
  usePush.setState((st) => {
    const events = st.events.length >= MAX_EVENTS ? [...st.events.slice(-MAX_EVENTS + 500), ev] : [...st.events, ev]
    if (method === 'narrative.line') {
      const line = params as NarrativeLine
      const narrative = st.narrative.length >= 1000 ? [...st.narrative.slice(-800), line] : [...st.narrative, line]
      return { events, narrative }
    }
    return { events }
  })
})

// The narrative is global: watch it on every (re)connect, and keep its backlog.
client.onOpen(() => {
  client
    .call<NarrativeWatchResult>('narrative.watch')
    .then((r) => usePush.setState({ narrative: r.lines, narrativeOn: true }))
    .catch(() => usePush.setState({ narrativeOn: false }))
})

/** Watch one session's live events while mounted (model.delta, tool.*, turn.*, confirm.*). */
export function useSessionWatch(sessionId: string | undefined) {
  const open = useConn((s) => s.status === 'open')
  useEffect(() => {
    if (!sessionId || !open) return
    client.call('session.watch', { session_id: sessionId }).catch(() => {})
    return () => {
      client.call('session.unwatch', { session_id: sessionId }).catch(() => {})
    }
  }, [sessionId, open])
}

client.connect()
