// The Ship's data (theseus-logs): the existing protocol reads and pushes, composed. Nothing here is invented: a
// flare is a node that arrived, a gear is a job dispatched and not settled, a stream is a model.delta.
//
// - The fleet: session.list, execution.list, task.list, and confirm.list, which the push keeps fresh
//   (executions.watch, bound in rpc.ts).
// - The lights: node.list, all sessions at once (the newest 2,000); past that, each session by itself. A session
//   that works is watched (session.watch) while it works, and each node.written reads its nodes again.
// - L1 and jobs: tool.job_started rows (`class: "l1"`), tool.started and tool.ended, and the actions in flight.
// - Reach: node.reach for the selected vessel's nodes, read only while it is selected.
//
// What the pushes say lives in a small store per mounted Ship, replaced (never mutated) on each change, so a render
// reads only state.
import { useEffect, useMemo, useRef, useState } from 'react'
import { useQueryClient } from '@tanstack/react-query'
import { createStore, useStore } from 'zustand'
import type {
  ActionInfo, ConfirmRequest, ExecutionInfo, ExecutionView, Health, LedgerEntry, NodeInfo, NodeReachResult, ProfileListResult,
  SessionInfo, TaskInfo,
} from '@protocol'
import { call, client, useConn, useRpc } from '@/lib/rpc'
import { buildModel, type ReachLink, type ShipModel } from './model'

type D = Record<string, unknown>

export interface ShipData {
  model: ShipModel | null
  /** How much of the graph is read, 0 to 1: the plank strip fills with it. */
  progress: number
  health?: Health
  profiles?: ProfileListResult
  /** Tokens a minute, from the provider.call rows of the last minute. */
  tpm: number | null
  /** Facts that arrived live in the last minute: the plank strip's gold. */
  arrivals: number
  synthetic: boolean
  error?: string
}

const GLOBAL_N = 2000

interface Live {
  nodes: Map<string, NodeInfo>
  /** The node ids of the first read: anything not among them arrived while we watched. */
  known: Set<string> | null
  born: Map<string, number>
  l1: Set<string>
  running: Set<string>
  streaming: Map<string, number>
  failedAt: Map<string, number>
  reports: Map<string, number>
  knownSessions: Set<string> | null
  bornSessions: Map<string, number>
  arrivals: number[]
  reach: ReachLink[]
  progress: number
  error?: string
  /** A slow clock (the gold planks of the last hour, a stream gone quiet). */
  now: number
}

const fresh = (): Live => ({
  nodes: new Map(), known: null, born: new Map(), l1: new Set(), running: new Set(), streaming: new Map(), failedAt: new Map(),
  reports: new Map(), knownSessions: null, bornSessions: new Map(), arrivals: [], reach: [], progress: 0, now: Date.now(),
})

/** Merge nodes into the store; ids the first read did not have arrived live (their flare). */
function mergeNodes(s: Live, list: NodeInfo[]): Partial<Live> | Live {
  let nodes: Map<string, NodeInfo> | null = null
  let born: Map<string, number> | null = null
  const t = Date.now()
  for (const n of list) {
    const o = s.nodes.get(n.node_id)
    if (o && o.text === n.text && JSON.stringify(o.detail) === JSON.stringify(n.detail)) continue
    nodes ??= new Map(s.nodes)
    nodes.set(n.node_id, n)
    if (!o && s.known && !s.known.has(n.node_id)) {
      born ??= new Map(s.born)
      born.set(n.node_id, t)
    }
  }
  if (!nodes) return s
  return born ? { nodes, born, arrivals: [...s.arrivals, t] } : { nodes }
}

const withSet = <T,>(set: Set<T>, v: T, on: boolean): Set<T> => {
  if (set.has(v) === on) return set
  const n = new Set(set)
  if (on) n.add(v)
  else n.delete(v)
  return n
}
const withMap = <K, V>(m: Map<K, V>, k: K, v: V | undefined): Map<K, V> => {
  const n = new Map(m)
  if (v === undefined) n.delete(k)
  else n.set(k, v)
  return n
}

/** The live fleet, from the daemon. `asOf` is the time machine's seam (unused until that round). */
export function useShipLive(selected: string | undefined, asOf?: number): ShipData {
  const queries = useQueryClient()
  const open = useConn((s) => s.status === 'open')
  const [store] = useState(() => createStore<Live>(fresh))
  const st = useStore(store)

  const { data: sl } = useRpc<{ sessions: SessionInfo[] }>('session.list', undefined, 3000)
  const { data: el } = useRpc<{ executions: ExecutionInfo[] }>('execution.list', undefined, 3000)
  const { data: tl } = useRpc<{ tasks: TaskInfo[] }>('task.list', {}, 3000)
  const { data: cl } = useRpc<{ confirms: ConfirmRequest[] }>('confirm.list', undefined, 2000)
  const { data: health } = useRpc<Health>('health', undefined, 2000)
  const { data: profiles } = useRpc<ProfileListResult>('profile.list', undefined, 10_000)
  const { data: jobs } = useRpc<{ rows: LedgerEntry[] }>('ledger.tail', { n: 2000, kind: 'tool.job_started', session_id: null }, 8000)
  const { data: calls } = useRpc<{ rows: LedgerEntry[] }>('ledger.tail', { n: 400, kind: 'provider.call', session_id: null }, 5000)
  const anyWork = (sl?.sessions ?? []).some((s) => s.attention?.level === 'working' || s.execution_state === 'running')
  const { data: al } = useRpc<{ actions: ActionInfo[] }>('action.list', { execution_id: null, n: 500 }, anyWork ? 1000 : 5000)

  // A session's nodes, read again shortly after it says it wrote one (debounced per session).
  const pending = useRef(new Map<string, ReturnType<typeof setTimeout>>())
  const [readSession] = useState(() => (sid: string, delay = 120) => {
    if (pending.current.has(sid)) return
    pending.current.set(sid, setTimeout(() => {
      pending.current.delete(sid)
      call<{ nodes: NodeInfo[] }>('node.list', { session_id: sid, kind: null, n: GLOBAL_N })
        .then((r) => store.setState((s) => mergeNodes(s, r.nodes)))
        .catch(() => {})
    }, delay))
  })

  // The first read: everything at once, then each session it did not cover, four at a time, newest first.
  useEffect(() => {
    if (!open) return
    let gone = false
    ;(async () => {
      try {
        store.setState({ progress: 0.08 })
        const r = await call<{ nodes: NodeInfo[]; total: number }>('node.list', { session_id: null, kind: null, n: GLOBAL_N })
        if (gone) return
        store.setState((s) => mergeNodes(s, r.nodes))
        if (r.nodes.length < r.total) {
          const ss = await call<{ sessions: SessionInfo[] }>('session.list')
          const order = [...ss.sessions].sort((a, b) => b.last_active_ms - a.last_active_ms).map((s) => s.session_id)
          let done = 0
          const next = async (): Promise<void> => {
            const sid = order.shift()
            if (!sid || gone) return
            try {
              const x = await call<{ nodes: NodeInfo[] }>('node.list', { session_id: sid, kind: null, n: GLOBAL_N })
              store.setState((s) => mergeNodes(s, x.nodes))
            } catch { /* a session that went away */ }
            done++
            store.setState({ progress: 0.1 + 0.9 * (done / Math.max(1, ss.sessions.length)) })
            return next()
          }
          await Promise.all([next(), next(), next(), next()])
        }
        store.setState({ progress: 1 })
      } catch (e) {
        store.setState({ error: String((e as { message?: string })?.message ?? e) })
      } finally {
        if (!gone) store.setState((s) => (s.known ? s : { known: new Set(s.nodes.keys()) }))
      }
    })()
    return () => { gone = true }
  }, [open, store])

  // What the pushes say.
  const turnSession = useRef(new Map<string, string>())
  useEffect(() => {
    const arrive = (s: Live) => [...s.arrivals, Date.now()]
    return client.onNotify((method, params) => {
      const p = (params ?? {}) as D
      const sid = typeof p.session_id === 'string' ? p.session_id : undefined
      const cid = typeof p.correlation_id === 'string' ? p.correlation_id : undefined
      switch (method) {
        case 'node.written':
          if (sid) readSession(sid)
          break
        case 'turn.started':
          if (sid && typeof p.turn_id === 'string') turnSession.current.set(p.turn_id, sid)
          store.setState((s) => ({ arrivals: arrive(s) }))
          break
        case 'model.delta': {
          const s0 = typeof p.turn_id === 'string' ? turnSession.current.get(p.turn_id) : undefined
          if (!s0) break
          // At most a few updates a second: the stream's light pulses on its own.
          const was = store.getState().streaming.get(s0) ?? 0
          if (Date.now() - was > 1500) store.setState((s) => ({ streaming: withMap(s.streaming, s0, Date.now()) }))
          break
        }
        case 'tool.started':
          if (cid) {
            store.setState((s) => ({
              l1: p.class === 'l1' ? withSet(s.l1, cid, true) : s.l1,
              running: p.backend === 'job' ? withSet(s.running, cid, true) : s.running,
              arrivals: arrive(s),
            }))
          }
          break
        case 'tool.ended':
          store.setState((s) => ({
            running: cid ? withSet(s.running, cid, false) : s.running,
            streaming: sid && s.streaming.has(sid) ? withMap(s.streaming, sid, undefined) : s.streaming,
          }))
          break
        case 'turn.ended':
          if (sid) store.setState((s) => ({ streaming: s.streaming.has(sid) ? withMap(s.streaming, sid, undefined) : s.streaming, arrivals: arrive(s) }))
          break
        case 'turn.failed':
          if (sid) store.setState((s) => ({ failedAt: withMap(s.failedAt, sid, Date.now()), streaming: withMap(s.streaming, sid, undefined), arrivals: arrive(s) }))
          break
        case 'execution.changed': {
          const v = p as unknown as ExecutionView
          const failed = v.state === 'failed' && !!v.previous && v.previous !== 'failed'
          const reported = v.kind === 'task' && (v.state === 'complete' || v.state === 'succeeded') && !!v.previous && v.previous !== v.state
          if (failed || reported) {
            store.setState((s) => ({
              failedAt: failed ? withMap(s.failedAt, v.session_id, Date.now()) : s.failedAt,
              reports: reported ? withMap(s.reports, v.session_id, Date.now()) : s.reports,
            }))
          }
          if (v.session_id) readSession(v.session_id, 300)
          break
        }
        case 'events.lost':
          void queries.invalidateQueries()
          break
      }
    })
  }, [store, queries, readSession])

  // Watch every session that works or waits for you (and the selected one); let go a little after it stops.
  const working = useMemo(() => {
    const ids = new Set<string>()
    for (const s of sl?.sessions ?? []) {
      if (s.attention?.level === 'working' || s.attention?.level === 'needs_you' || s.execution_state === 'running' || s.execution_state === 'queued') ids.add(s.session_id)
    }
    if (selected) ids.add(selected)
    return [...ids].sort().join(',')
  }, [sl, selected])
  const watched = useRef(new Map<string, ReturnType<typeof setTimeout> | null>())
  useEffect(() => {
    if (!open) return
    const want = new Set(working ? working.split(',') : [])
    for (const id of want) {
      const w = watched.current.get(id)
      if (w === undefined) {
        watched.current.set(id, null)
        client.call('session.watch', { session_id: id }).then(() => readSession(id, 0)).catch(() => {})
      } else if (w) { clearTimeout(w); watched.current.set(id, null) }
    }
    for (const [id, w] of watched.current) {
      if (want.has(id) || w) continue
      watched.current.set(id, setTimeout(() => {
        watched.current.delete(id)
        client.call('session.unwatch', { session_id: id }).catch(() => {})
      }, 15_000))
    }
  }, [working, open, readSession])
  // A reconnect forgets every watch: start again.
  useEffect(() => client.onOpen(() => { watched.current.clear() }), [])

  // Reach: where the selected vessel's nodes went (node.reach), read while it is selected.
  const loaded = st.known !== null
  useEffect(() => {
    store.setState({ reach: [] })
    if (!selected || !open || !loaded) return
    let gone = false
    const mine = [...store.getState().nodes.values()].filter((n) => n.session_id === selected).slice(-160)
    ;(async () => {
      const out: ReachLink[] = []
      const queue = [...mine]
      const worker = async () => {
        for (let n = queue.shift(); n && !gone; n = queue.shift()) {
          try {
            const r = await call<NodeReachResult>('node.reach', { node_id: n.node_id, max_generations: 2 })
            for (const d of r.descendants) {
              if (d.session_id !== selected) out.push({ fromSession: selected, fromNode: n.node_id, toSession: d.session_id, toNode: d.node_id, via: d.via })
            }
          } catch { /* a node with no reach */ }
        }
      }
      await Promise.all([worker(), worker(), worker()])
      if (!gone && out.length) store.setState({ reach: out })
    })()
    return () => { gone = true }
  }, [selected, open, loaded, store])

  // Sessions that open while we watch flare in.
  useEffect(() => {
    if (!sl) return
    store.setState((s) => {
      if (!s.knownSessions) return { knownSessions: new Set(sl.sessions.map((x) => x.session_id)) }
      const fresh = sl.sessions.filter((x) => !s.knownSessions!.has(x.session_id))
      if (!fresh.length) return s
      const known = new Set(s.knownSessions)
      const born = new Map(s.bornSessions)
      for (const x of fresh) { known.add(x.session_id); born.set(x.session_id, Date.now()) }
      return { knownSessions: known, bornSessions: born, arrivals: [...s.arrivals, Date.now()] }
    })
  }, [sl, store])

  // The slow clock: often while something streams, else twice a minute. It also lets old arrivals go.
  const streamingNow = st.streaming.size > 0
  useEffect(() => {
    const t = setInterval(() => {
      const now = Date.now()
      store.setState((s) => ({ now, arrivals: s.arrivals.filter((a) => a > now - 60_000) }))
    }, streamingNow ? 2000 : 30_000)
    return () => clearInterval(t)
  }, [streamingNow, store])

  const model = useMemo(() => {
    if (!sl || !el) return null
    const l1 = new Set(st.l1)
    for (const r of jobs?.rows ?? []) {
      const d = (r.data ?? {}) as D
      if (d.class === 'l1' && typeof d.correlation_id === 'string') l1.add(d.correlation_id)
    }
    // Jobs running: dispatched and not settled (the action list), and those tool.started said began.
    const run = new Set(st.running)
    const reserved = new Map<string, number>()
    for (const a of al?.actions ?? []) {
      if (!a.settled_at_ms && a.dispatched_at_ms && a.tool === 'proc.run') run.add(a.correlation_id)
      if (a.settled_at_ms) run.delete(a.correlation_id)
      if (!a.settled_at_ms && a.reserved_usd > 0) reserved.set(a.execution_id, (reserved.get(a.execution_id) ?? 0) + a.reserved_usd)
    }
    return buildModel({
      sessions: sl.sessions,
      executions: el.executions,
      tasks: tl?.tasks ?? [],
      nodes: [...st.nodes.values()],
      confirms: cl?.confirms ?? [],
      l1,
      jobsRunning: run,
      streaming: st.streaming,
      failedAt: st.failedAt,
      reports: st.reports,
      born: st.born,
      bornSessions: st.bornSessions,
      reserved,
      reach: st.reach,
      now: st.now,
      asOf,
    })
  }, [sl, el, tl, cl, jobs, al, st.nodes, st.l1, st.running, st.streaming, st.failedAt, st.reports, st.born, st.bornSessions, st.reach, st.now, asOf])

  const tpm = useMemo(() => {
    if (!calls) return null
    const cut = st.now - 60_000
    let sum = 0
    for (const r of calls.rows) {
      if (r.at_unix_ms < cut) continue
      const u = ((r.data ?? {}) as D).usage as D | undefined
      if (!u) continue
      sum += Number(u.input_tokens ?? 0) + Number(u.output_tokens ?? 0) + Number(u.cache_read_input_tokens ?? 0) + Number(u.cache_creation_input_tokens ?? 0)
    }
    return sum
  }, [calls, st.now])

  return { model, progress: st.progress, health, profiles, tpm, arrivals: st.arrivals.length, synthetic: false, error: st.error }
}

/** The dev-only synthetic fleet (10,000 nodes in 200 sessions), for measuring. Never in a production build. */
export function useShipSynthetic(): ShipData {
  const [model, setModel] = useState<ShipModel | null>(null)
  useEffect(() => {
    if (!(import.meta.env.DEV || import.meta.env.MODE === 'bench')) return
    let gone = false
    import('./synth').then(({ synthInput }) => {
      if (!gone) setModel(buildModel(synthInput(Date.now())))
    })
    return () => { gone = true }
  }, [])
  return { model, progress: model ? 1 : 0.3, tpm: null, arrivals: 0, synthetic: true }
}
