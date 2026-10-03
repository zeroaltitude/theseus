// The Theseus protocol over a WebSocket: one JSON-RPC 2.0 object per text
// frame. Its types are generated from crates/theseus-protocol (theseus-0g4):
// ./protocol.gen, which a test there writes and the gate holds to the Rust
// types. They are re-exported here, some under the names the apps use; this
// file keeps what is not a type: ProtocolClient and its helpers.

import type { ExternalText, Id, Message, NodeReachResult, Notification, Request, RpcError } from './protocol.gen'

export type * from './protocol.gen'
export type {
  HealthResult as Health,
  TurnSubmitResult as TurnResult,
  SessionHistoryResult as SessionHistory,
  CatalogListResult as CatalogList,
  ToolListResult as ToolList,
  ProfileListResult as ProfileList,
} from './protocol.gen'

/// What a session read, as the hold's reason names it (theseus-qiy): a search by its
/// query, `web.search "tokio JoinSet documentation"`, a job that connected out of L1 by
/// its hosts, `proc.run's egress to api.github.com:443` (18c), anything else by its URL.
export function heldWhat(h: ExternalText): string {
  if (h.query != null) return `${h.tool} "${h.query}"`
  // A job in L1 that connected out (M4 18c): the hosts it reached.
  return h.via === 'egress' ? `${h.tool}'s egress to ${h.url}` : `${h.tool} ${h.url}`
}

/// A node's reach in words (theseus-n4m, step 12a), as `theseus reach` says it: `seen by 24 contexts in
/// 2 sessions`, with the first and last exposure of the node and its copies (absent when nothing held it),
/// and each generation in a line, for a tooltip.
export function reachWords(r: NodeReachResult): { seen: string; first?: number; last?: number; generations: string[] } {
  const n = (k: number, one: string, many: string) => `${k} ${k === 1 ? one : many}`
  const all = [r.direct, ...r.descendants]
  const firsts = all.flatMap((e) => (e.first_ms != null ? [e.first_ms] : []))
  const lasts = all.flatMap((e) => (e.last_ms != null ? [e.last_ms] : []))
  const held = (e: (typeof all)[number]) =>
    e.compilations.length === 0 && e.loops === 0
      ? 'no context held it'
      : `${n(e.compilations.length, 'compilation', 'compilations')}, ${n(e.loops, 'loop', 'loops')}`
  return {
    seen: `seen by ${n(r.totals.contexts, 'context', 'contexts')} in ${n(r.totals.sessions, 'session', 'sessions')}${r.partial ? ', or more' : ''}`,
    first: firsts.length ? Math.min(...firsts) : undefined,
    last: lasts.length ? Math.max(...lasts) : undefined,
    generations: [
      `generation 0 · ${r.session_id}: ${held(r.direct)}`,
      ...r.descendants.map((d) => `generation ${d.generation} · ${d.session_id} (${d.via} by the ${d.route}): ${held(d)}`),
    ],
  }
}

export type NotifyHandler = (method: string, params: unknown) => void
export type Status = 'connecting' | 'open' | 'closed'

/// A protocol connection that reconnects on its own (backoff up to 10 s).
/// `onOpen` runs after every successful connect, so subscriptions (session.watch)
/// and views can be re-established after a daemon restart.
export class ProtocolClient {
  private ws: WebSocket | null = null
  private nextId = 1
  private pending = new Map<Id, { resolve: (v: unknown) => void; reject: (e: RpcError) => void }>()
  private listeners = new Set<NotifyHandler>()
  private openers = new Set<() => void>()
  private closedByUs = false
  private retryMs = 500
  private timer: ReturnType<typeof setTimeout> | null = null
  onStatus: (s: Status) => void = () => {}
  private url: string

  constructor(url = `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`) {
    this.url = url
  }

  connect() {
    this.closedByUs = false
    this.open()
  }

  private open() {
    this.onStatus('connecting')
    const ws = new WebSocket(this.url)
    this.ws = ws
    ws.onopen = () => {
      this.retryMs = 500
      this.onStatus('open')
      for (const o of this.openers) o()
    }
    ws.onclose = () => {
      if (this.ws !== ws) return
      this.ws = null
      for (const p of this.pending.values()) p.reject({ code: -1, message: 'connection closed' })
      this.pending.clear()
      if (this.closedByUs) { this.onStatus('closed'); return }
      this.onStatus('connecting')
      this.timer = setTimeout(() => this.open(), this.retryMs)
      this.retryMs = Math.min(this.retryMs * 2, 10_000)
    }
    ws.onmessage = (ev) => {
      let msg: Message
      try { msg = JSON.parse(ev.data as string) } catch { return }
      if ('id' in msg && ('result' in msg || 'error' in msg)) {
        const p = this.pending.get(msg.id)
        if (!p) return
        this.pending.delete(msg.id)
        if (msg.error) p.reject(msg.error); else p.resolve(msg.result)
      } else if ('method' in msg) {
        for (const l of this.listeners) l(msg.method, (msg as Notification).params)
      }
    }
  }

  onNotify(h: NotifyHandler): () => void {
    this.listeners.add(h)
    return () => this.listeners.delete(h)
  }

  onOpen(h: () => void): () => void {
    this.openers.add(h)
    return () => this.openers.delete(h)
  }

  call<T = unknown>(method: string, params?: unknown): Promise<T> {
    const ws = this.ws
    if (!ws || ws.readyState !== WebSocket.OPEN) {
      return Promise.reject<T>({ code: -1, message: 'not connected' } as RpcError)
    }
    const id = this.nextId++
    const req: Request = { jsonrpc: '2.0', id, method, params }
    return new Promise<T>((resolve, reject) => {
      this.pending.set(id, { resolve: (v) => resolve(v as T), reject })
      ws.send(JSON.stringify(req))
    })
  }

  close() {
    this.closedByUs = true
    if (this.timer) clearTimeout(this.timer)
    this.ws?.close()
  }
}
