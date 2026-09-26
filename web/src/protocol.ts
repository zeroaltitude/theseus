// The Theseus protocol over a WebSocket: one JSON-RPC 2.0 object per text
// frame. Mirrors crates/theseus-protocol; keep the two in step.

export type Id = number | string

export interface Request { jsonrpc: '2.0'; id: Id; method: string; params?: unknown }
export interface Notification { jsonrpc: '2.0'; method: string; params?: unknown }
export interface RpcError { code: number; message: string; data?: unknown }
export interface Response { jsonrpc: '2.0'; id: Id; result?: unknown; error?: RpcError }
export type Message = Request | Notification | Response

export interface Usage {
  input_tokens: number
  output_tokens: number
  cache_read_input_tokens: number
  cache_creation_input_tokens: number
}

export interface Health {
  name: string; version: string; protocol: string; uptime_secs: number
  sessions: number; turns: number; model: string; profile: string; provider: string; providers: string[]
  secrets_resolved: string[]
  usage_total: Usage; provider_errors: number; ledger_rows: number
}

export interface SessionInfo {
  session_id: string; kind: 'conversation' | 'task'; label: string | null
  created_at_unix_ms: number; turns: number; usage: Usage
}

export interface ProfileInfo {
  name: string; provider: string; model: string; max_tokens: number; has_system: boolean; live: boolean
}
export interface ProfileList { live: string; live_source: string; profiles: ProfileInfo[] }

export interface Span {
  name: string; kind: string; start_us: number; end_us: number | null
  attrs?: unknown; children?: Span[]
}

export interface TurnResult {
  session_id: string; turn_id: string; loops: number; output: string
  stop_reason: string; provider_stop_reason: string | null; model: string
  provider: string; profile: string
  usage: Usage; elapsed_ms: number; first_token_ms: number | null; request_id: string | null
  trace?: Span | null
}

export interface ProviderErrorData {
  class: string; transient: boolean; usage_unknown: boolean
  turn_id: string | null; session_id: string; elapsed_ms: number
  trace?: Span | null
}

export type NotifyHandler = (method: string, params: unknown) => void

export class ProtocolClient {
  private ws: WebSocket | null = null
  private nextId = 1
  private pending = new Map<Id, { resolve: (v: unknown) => void; reject: (e: RpcError) => void }>()
  private listeners = new Set<NotifyHandler>()
  onStatus: (s: 'connecting' | 'open' | 'closed') => void = () => {}

  connect(url = `${location.protocol === 'https:' ? 'wss' : 'ws'}://${location.host}/ws`): Promise<void> {
    this.onStatus('connecting')
    return new Promise((resolve, reject) => {
      const ws = new WebSocket(url)
      this.ws = ws
      ws.onopen = () => { this.onStatus('open'); resolve() }
      ws.onerror = () => reject(new Error(`cannot connect to ${url}`))
      ws.onclose = () => {
        this.onStatus('closed')
        for (const p of this.pending.values()) p.reject({ code: -1, message: 'connection closed' })
        this.pending.clear()
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
    })
  }

  onNotify(h: NotifyHandler): () => void {
    this.listeners.add(h)
    return () => this.listeners.delete(h)
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

  close() { this.ws?.close() }
}
