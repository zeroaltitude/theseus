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

export interface KernelStatus {
  accepting: boolean; admission_ceiling: number; turns_held: number
  executions_by_state: Record<string, number>; actions_by_state: Record<string, number>
  quarantined_completions: number; startup: unknown
}

export interface Health {
  name: string; version: string; protocol: string; uptime_secs: number
  sessions: number; turns: number; model: string; profile: string; provider: string; providers: string[]
  secrets_resolved: string[]
  usage_total: Usage; provider_errors: number; ledger_rows: number
  kernel: KernelStatus
  cost_usd_total?: number; catalog_version?: string
  bindings?: BindingStatus[]
  /// `narrative = true` in the config: The Narrative tab shows.
  narrative?: boolean
}

export interface PlaceStatus {
  kind: 'channel' | 'dm'; label: string; channel_id?: string; session_id?: string
  users: string[]; mention_only?: boolean; last_activity_ms: number
}

export interface BindingStatus {
  kind: string; state: string; detail?: string; bot_user?: string; guild_id?: string
  bindings_file?: string; revision?: string; places: PlaceStatus[]
  connected_at_ms: number; latency_ms?: number
  messages_in: number; messages_out: number; edits: number; interactions: number
  ignored: number; errors: number; last_error?: string
}

export interface SessionInfo {
  session_id: string; kind: 'conversation' | 'task'; label: string | null
  created_at_unix_ms: number; turns: number; usage: Usage
  execution_id?: string | null; execution_state?: string | null
  last_active_ms?: number; cost_usd?: number; tool_calls?: number
  profile?: string | null; model?: string | null; compilation_id?: string | null; title?: string | null
  pending_confirms?: number
}

export interface BudgetInfo { limit: number; spent: number; reserved: number; held_unknown: number; available: number }

export interface ExecutionInfo {
  execution_id: string; session_id: string; kind: string; state: string
  turns: number; interrupted: number; outstanding: number; queued_results: number
  budget: BudgetInfo; wake?: unknown; reports_to?: string | null; ended_reason?: string | null
  created_at_ms: number; updated_at_ms: number
}

export interface ActionInfo {
  correlation_id: string; execution_id: string; session_id: string; tool: string; state: string
  retry_class: string; planned_at_ms: number; authorized_at_ms?: number | null; dispatched_at_ms?: number | null
  settled_at_ms?: number | null; deadline_at_ms: number; reserved_units: number; confirmed: boolean
  cancel?: string | null; external_op_id?: string | null; result_ref?: string | null; resolution?: string | null
  completions_seen: number
}

export interface LedgerEntry {
  position: number; at_unix_ms: number; kind: string
  session_id: string | null; turn_id: string | null; data: unknown
}

export interface ProfileInfo {
  name: string; provider: string; model: string; max_output_tokens: number; has_system: boolean; live: boolean
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
  execution_id?: string | null; cost_usd?: number | null; tool_calls?: number
  awaiting_confirm?: string | null; stop_details?: unknown; continuation?: boolean
}

export interface ProviderErrorData {
  class: string; transient: boolean; usage_unknown: boolean
  turn_id: string | null; session_id: string; elapsed_ms: number
  trace?: Span | null
}

// ---- M3: content

/// One node of a session's graph as clients render it.
export interface NodeInfo {
  node_id: string
  kind: 'user_message' | 'assistant_message' | 'tool_call' | 'tool_result' | string
  session_id: string; position: number; at_unix_ms: number
  turn_id?: string | null; loop_index?: number | null; author?: string | null
  text: string; thinking?: string
  /// Kind-specific: model/usage/cost/tool_calls (assistant), tool/input/decision/result/plan
  /// (tool call), tool/status/is_error/duration_ms/late/meta (tool result).
  detail: Record<string, unknown> | null
  bytes: number
}

export interface ConfirmRequest {
  correlation_id: string; session_id: string; execution_id: string
  tool: string; input: unknown; resource?: string | null; reason: string; by: string
  requested_at_ms: number; expires_at_ms: number; floor?: boolean
}

export interface SessionHistory { session: SessionInfo; nodes: NodeInfo[]; pending_confirms: ConfirmRequest[] }

export interface CompilationInfo {
  compilation_id: string; session_id: string; created_at_ms: number
  trigger: string; strategy: string; as_of: number; includes: number
  derived_from?: string | null; manifest: Record<string, unknown>; current: boolean
}

export interface CatalogModel { model: string; entry: Record<string, unknown>; profiles: string[] }
export interface CatalogList { version: string; models: CatalogModel[] }

export interface ToolInfo {
  name: string; wire_name: string; family: string; description: string
  class: string; backend: string; policy: string; input_schema: unknown; calls: number
}
export interface ToolList { tools: ToolInfo[]; roots: string[]; shell_fallback_ratio: number; calls_total: number }

// ---- the narrative (theseus-5fy)

export type NarrativePart = 'session' | 'turn' | 'loop' | 'context' | 'model' | 'tool' | 'approval' | 'job'

/// One templated sentence about one architectural step; kept only in the daemon's memory.
export interface NarrativeLine {
  seq: number; at_unix_ms: number; part: NarrativePart
  session_id?: string | null; turn_id?: string | null; text: string
}
export interface NarrativeWatchResult { lines: NarrativeLine[]; capacity: number }

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
