// The Theseus protocol over a WebSocket: one JSON-RPC 2.0 object per text
// frame. Its types are generated from crates/theseus-protocol (theseus-0g4):
// ./protocol.gen, which a test there writes and the gate holds to the Rust
// types. They are re-exported here, some under the names the apps use; this
// file keeps what is not a type: ProtocolClient and its helpers.

import type { ExternalText, Id, Message, Notification, Request, RpcError } from './protocol.gen'

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
/// query, `web.search "tokio JoinSet documentation"`, anything else by its URL.
export function heldWhat(h: ExternalText): string {
  return h.query != null ? `${h.tool} "${h.query}"` : `${h.tool} ${h.url}`
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
