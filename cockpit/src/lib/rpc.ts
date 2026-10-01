// The cockpit's data layer (theseus-45n5): one protocol connection, polled reads through TanStack Query, and
// the pushes (the narrative, and the sessions being watched) collected in small stores.
//
// Until the all-sessions push lands (the spine's 9b, executions.watch), the fleet is polled. Polling is cheap:
// every read is answered from the daemon's memory or its index.
import { useEffect } from 'react'
import { useQuery, type UseQueryOptions } from '@tanstack/react-query'
import { create } from 'zustand'
import { ProtocolClient, type NarrativeLine, type NarrativeWatchResult, type Status } from '@protocol'

export const client = new ProtocolClient()

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

/** One call, timed: its round trip feeds the link-latency readout. */
export async function call<T = unknown>(method: string, params?: unknown): Promise<T> {
  const t0 = performance.now()
  const out = await client.call<T>(method, params)
  const dt = performance.now() - t0
  useConn.setState((st) => ({ rtts: [...st.rtts.slice(-59), dt] }))
  return out
}

/** A polled read. `interval` in ms; 0 reads once. Disabled while the link is down. */
export function useRpc<T>(
  method: string,
  params?: unknown,
  interval = 2000,
  opts?: Partial<UseQueryOptions<T>>,
) {
  const open = useConn((s) => s.status === 'open')
  return useQuery<T>({
    queryKey: [method, params ?? null],
    queryFn: () => call<T>(method, params),
    refetchInterval: interval > 0 ? interval : false,
    enabled: open && (opts?.enabled ?? true),
    staleTime: interval > 0 ? interval / 2 : 30_000,
    ...opts,
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
