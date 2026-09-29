//! The Theseus wire protocol.
//!
//! JSON-RPC 2.0, one message per line, UTF-8, on any byte stream (stdio, a Unix
//! domain socket, or an in-process channel). This crate is types only: every
//! client, including the in-binary CLI, links this and nothing else from the
//! core, so no client has a privileged path into the kernel.
//!
//! Requests change state and get exactly one response. Notifications report
//! state and are also ledger rows on the server side.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: &str = "0.1";
pub const JSONRPC: &str = "2.0";

/// Method names. Requests (client → server).
pub mod method {
    pub const HEALTH: &str = "health";
    pub const SESSION_OPEN: &str = "session.open";
    pub const SESSION_LIST: &str = "session.list";
    pub const TURN_SUBMIT: &str = "turn.submit";
    pub const HOOKS_LIST: &str = "hooks.list";
    pub const HOOKS_REGISTER: &str = "hooks.register";
    pub const HOOKS_UNREGISTER: &str = "hooks.unregister";
    pub const LEDGER_TAIL: &str = "ledger.tail";
    pub const PROFILE_LIST: &str = "profile.list";
    pub const PROFILE_USE: &str = "profile.use";
    pub const EXECUTION_LIST: &str = "execution.list";
    pub const EXECUTION_CANCEL: &str = "execution.cancel";
    pub const ACTION_LIST: &str = "action.list";
    pub const ACTION_CONFIRM: &str = "action.confirm";
    pub const SESSION_HISTORY: &str = "session.history";
    pub const SESSION_WATCH: &str = "session.watch";
    pub const SESSION_UNWATCH: &str = "session.unwatch";
    pub const SESSION_RECOMPILE: &str = "session.recompile";
    pub const CATALOG_LIST: &str = "catalog.list";
    pub const COMPILATION_LIST: &str = "compilation.list";
    pub const NODE_LIST: &str = "node.list";
    pub const TOOL_LIST: &str = "tool.list";
    pub const SHUTDOWN: &str = "shutdown";
}

/// Notification names (server → client).
pub mod notify {
    pub const TURN_STARTED: &str = "turn.started";
    pub const LOOP_STARTED: &str = "loop.started";
    pub const MODEL_DELTA: &str = "model.delta";
    pub const TOOL_PROPOSED: &str = "tool.proposed";
    pub const LOOP_ENDED: &str = "loop.ended";
    pub const TURN_ENDED: &str = "turn.ended";
    pub const HOOK_EVENT: &str = "hook.event";
    pub const PROFILE_CHANGED: &str = "profile.changed";
    /// Thinking summaries / progress updates as they stream.
    pub const MODEL_THINKING: &str = "model.thinking";
    /// The context for a loop was compiled (append or recompile, sizes, digest).
    pub const CONTEXT_COMPILED: &str = "context.compiled";
    /// A tool call started (after the gate) and ended (with its result).
    pub const TOOL_STARTED: &str = "tool.started";
    pub const TOOL_ENDED: &str = "tool.ended";
    /// A tool call needs the operator's confirmation; the turn has parked.
    pub const CONFIRM_REQUESTED: &str = "confirm.requested";
    pub const CONFIRM_RESOLVED: &str = "confirm.resolved";
    /// A node was written to a watched session (history stays live).
    pub const NODE_WRITTEN: &str = "node.written";
    /// A turn failed after it was admitted (provider error, store error). The
    /// requester also gets the error response; watchers only get this.
    pub const TURN_FAILED: &str = "turn.failed";
    /// A call ran under a `notify` posture (`[policy].enforcement`, or a
    /// `[policy.tools]` / `[policy.mcp]` line), and the operator is told.
    pub const POLICY_NOTIFIED: &str = "policy.notified";
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Id {
    Num(u64),
    Str(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Request {
    pub jsonrpc: String,
    pub id: Id,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Response {
    pub jsonrpc: String,
    pub id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

/// Any line on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Message {
    Request(Request),
    Response(Response),
    Notification(Notification),
}

pub mod error_code {
    pub const PARSE: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    /// Theseus-specific: blocked (fail closed).
    pub const BLOCKED: i64 = -32001;
    pub const NOT_FOUND: i64 = -32002;
    pub const PROVIDER: i64 = -32003;
}

impl Request {
    pub fn new(id: Id, method: &str, params: impl Serialize) -> Self {
        Self {
            jsonrpc: JSONRPC.into(),
            id,
            method: method.into(),
            params: serde_json::to_value(params).unwrap_or(Value::Null),
        }
    }
}

impl Notification {
    pub fn new(method: &str, params: impl Serialize) -> Self {
        Self {
            jsonrpc: JSONRPC.into(),
            method: method.into(),
            params: serde_json::to_value(params).unwrap_or(Value::Null),
        }
    }
}

impl Response {
    pub fn ok(id: Id, result: impl Serialize) -> Self {
        Self {
            jsonrpc: JSONRPC.into(),
            id,
            result: Some(serde_json::to_value(result).unwrap_or(Value::Null)),
            error: None,
        }
    }
    pub fn err(id: Id, code: i64, message: impl Into<String>) -> Self {
        Self::err_with(id, code, message, Value::Null)
    }
    pub fn err_with(id: Id, code: i64, message: impl Into<String>, data: Value) -> Self {
        Self {
            jsonrpc: JSONRPC.into(),
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data,
            }),
        }
    }
}

// ---------------------------------------------------------------- payloads

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthResult {
    pub name: String,
    pub version: String,
    pub protocol: String,
    pub uptime_secs: u64,
    pub sessions: u64,
    pub turns: u64,
    pub model: String,
    /// The live profile name.
    #[serde(default)]
    pub profile: String,
    /// Default provider name and every configured provider.
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub providers: Vec<String>,
    pub secrets_resolved: Vec<String>,
    /// Tokens across every session, summed from session records.
    pub usage_total: Usage,
    pub provider_errors: u64,
    pub ledger_rows: u64,
    #[serde(default)]
    pub telemetry: TelemetryStatus,
    /// The durable kernel (M2): executions, actions, admission.
    #[serde(default)]
    pub kernel: KernelStatus,
    /// Dollars across every session, from the model catalog.
    #[serde(default)]
    pub cost_usd_total: f64,
    #[serde(default)]
    pub catalog_version: String,
    /// Channel bindings (M3: Discord) and what each is doing.
    #[serde(default)]
    pub bindings: Vec<BindingStatus>,
}

/// One channel binding as health reports it (spec P5: bindings as a file).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BindingStatus {
    /// "discord".
    pub kind: String,
    /// unconfigured | disabled | connecting | ready | resuming | disconnected | failed
    pub state: String,
    /// Why it is in that state, when that is not obvious (a missing file, a close code).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bot_user: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub guild_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bindings_file: Option<String>,
    /// First 12 hex of the bindings file's SHA-256: the binding revision.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub revision: Option<String>,
    #[serde(default)]
    pub places: Vec<PlaceStatus>,
    #[serde(default)]
    pub connected_at_ms: u64,
    /// Gateway heartbeat round trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latency_ms: Option<u64>,
    #[serde(default)]
    pub messages_in: u64,
    #[serde(default)]
    pub messages_out: u64,
    #[serde(default)]
    pub edits: u64,
    #[serde(default)]
    pub interactions: u64,
    #[serde(default)]
    pub ignored: u64,
    #[serde(default)]
    pub errors: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

/// A place Theseus lives in: a text channel or a DM, and the session behind it.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PlaceStatus {
    /// "channel" | "dm".
    pub kind: String,
    pub label: String,
    /// The Discord channel id (for a DM, known after the DM channel opens).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub channel_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Discord user ids that may drive turns here.
    #[serde(default)]
    pub users: Vec<String>,
    /// Only messages that @mention the bot or reply to it start turns.
    #[serde(default)]
    pub mention_only: bool,
    #[serde(default)]
    pub last_activity_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TelemetryStatus {
    pub enabled: bool,
    pub otlp_endpoint: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KernelStatus {
    /// Startup finished (five steps) and events are accepted.
    pub accepting: bool,
    pub admission_ceiling: u32,
    /// Executions holding a turn right now.
    pub turns_held: u32,
    /// Counts by state: queued, running, waiting, blocked, cancelled, failed, budget_exhausted, complete.
    #[serde(default)]
    pub executions_by_state: std::collections::BTreeMap<String, u64>,
    /// Counts by state: planned, authorized, dispatched, succeeded, failed, outcome_unknown, cancelled.
    #[serde(default)]
    pub actions_by_state: std::collections::BTreeMap<String, u64>,
    /// Completions that matched no action (never inferred into anything).
    pub quarantined_completions: u64,
    /// The last startup: step timings in µs and what it recovered.
    #[serde(default)]
    pub startup: Value,
}

/// One execution as the protocol shows it (spec §3.15).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionInfo {
    pub execution_id: String,
    pub session_id: String,
    pub kind: String,
    pub state: String,
    pub turns: u64,
    /// Times a crash interrupted a running turn (requeued at startup).
    pub interrupted: u32,
    /// Dispatched actions not yet settled.
    pub outstanding: u32,
    /// Settled results the next turn will consume.
    pub queued_results: u32,
    pub budget: BudgetInfo,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub wake: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reports_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_reason: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BudgetInfo {
    pub limit: u64,
    pub spent: u64,
    pub reserved: u64,
    pub held_unknown: u64,
    pub available: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionListResult {
    pub executions: Vec<ExecutionInfo>,
}

/// One action (a tool or provider call with a correlation id, spec §3.16).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionInfo {
    pub correlation_id: String,
    pub execution_id: String,
    pub session_id: String,
    pub tool: String,
    pub state: String,
    pub retry_class: String,
    pub planned_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorized_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatched_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled_at_ms: Option<u64>,
    pub deadline_at_ms: u64,
    pub reserved_units: u64,
    pub confirmed: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_op_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    pub completions_seen: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionListParams {
    /// Only this execution's actions.
    #[serde(default)]
    pub execution_id: Option<String>,
    /// Newest `n` (default 200).
    #[serde(default)]
    pub n: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionListResult {
    pub actions: Vec<ActionInfo>,
    pub total: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionCancelParams {
    pub execution_id: String,
    /// Who asked, as a label in the ledger (e.g. `discord:eddie`). Default: the connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionCancelResult {
    pub execution: ExecutionInfo,
    /// Dispatched actions whose backends were asked to stop.
    pub cancelled_actions: Vec<String>,
}

/// Also the kernel's: an execution stores it (`theseus_kernel::SessionKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Conversation,
    Task,
}

impl SessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SessionKind::Conversation => "conversation",
            SessionKind::Task => "task",
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionOpenParams {
    #[serde(default)]
    pub kind: Option<SessionKind>,
    #[serde(default)]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    pub session_id: String,
    pub kind: SessionKind,
    pub label: Option<String>,
    pub created_at_unix_ms: u64,
    pub turns: u64,
    /// Cumulative tokens over every turn in this session.
    #[serde(default)]
    pub usage: Usage,
    /// The session's one execution (spec §3.2a) and its current state.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_state: Option<String>,
    #[serde(default)]
    pub last_active_ms: u64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub tool_calls: u64,
    /// Profile/provider/model of the last turn (continuations reuse it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compilation_id: Option<String>,
    /// First words of the first prompt, for pickers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Tool calls in this session waiting for the operator's confirmation.
    #[serde(default)]
    pub pending_confirms: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionListResult {
    pub sessions: Vec<SessionInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnSubmitParams {
    /// Omit to open a fresh conversation session for this turn.
    #[serde(default)]
    pub session_id: Option<String>,
    pub input: String,
    /// Profile for this turn (a configured profile name); default is the live profile.
    #[serde(default)]
    pub profile: Option<String>,
    /// Raw override of the profile's provider for this turn.
    #[serde(default)]
    pub provider: Option<String>,
    /// Raw override of the profile's model for this turn.
    #[serde(default)]
    pub model: Option<String>,
    /// Who wrote the input, as a label on the message node (e.g. `discord:eddie`).
    /// Default: the connection's own label. A label, not an authority: every
    /// local protocol client acts as the operator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileInfo {
    pub name: String,
    pub provider: String,
    pub model: String,
    /// Output cap per call (the API's `max_tokens`), not an input limit.
    #[serde(alias = "max_tokens")]
    pub max_output_tokens: u32,
    pub has_system: bool,
    pub live: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileListResult {
    pub live: String,
    /// Where the live choice came from: "config" or "runtime" (persisted switch).
    pub live_source: String,
    pub profiles: Vec<ProfileInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileUseParams {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileChanged {
    pub previous: String,
    pub live: String,
    pub by: String,
}

/// One timed span of a turn trace. Times are microseconds from the turn's
/// start; a mark has `end_us == start_us`. Children are in start order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Span {
    pub name: String,
    /// turn | loop | hook | provider | mark | advancer | store | lock | compile
    pub kind: String,
    pub start_us: u64,
    #[serde(default)]
    pub end_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub attrs: Value,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Span>,
}

impl Span {
    pub fn duration_us(&self) -> u64 {
        self.end_us
            .unwrap_or(self.start_us)
            .saturating_sub(self.start_us)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TurnSubmitResult {
    pub session_id: String,
    pub turn_id: String,
    pub loops: u32,
    pub output: String,
    /// Why the Advancer ended the turn.
    pub stop_reason: String,
    /// The provider's own stop reason for the last loop.
    pub provider_stop_reason: Option<String>,
    pub model: String,
    #[serde(default)]
    pub provider: String,
    /// Profile the turn ran under ("" when raw overrides bypassed profiles entirely).
    #[serde(default)]
    pub profile: String,
    pub usage: Usage,
    pub elapsed_ms: u64,
    /// Time to the first streamed token of the last loop.
    #[serde(default)]
    pub first_token_ms: Option<u64>,
    /// Provider request id of the last loop, for support tickets.
    #[serde(default)]
    pub request_id: Option<String>,
    /// Every timed thing in the turn, nested: turn > loops > hooks/provider/advancer.
    #[serde(default)]
    pub trace: Option<Span>,
    #[serde(default)]
    pub execution_id: Option<String>,
    /// Dollars for this turn's provider calls, from the model catalog (None: model not in catalog).
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub tool_calls: u32,
    /// Set when the turn parked waiting for the operator to confirm this action.
    #[serde(default)]
    pub awaiting_confirm: Option<String>,
    /// The provider's `stop_details` (a refusal's category).
    #[serde(default)]
    pub stop_details: Option<Value>,
    /// The turn ran without new input (a continuation: late results, a confirm answer, a restart).
    #[serde(default)]
    pub continuation: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct LedgerTailParams {
    #[serde(default)]
    pub n: Option<usize>,
    /// Only rows of this kind (e.g. "turn.ended", "provider.error"). A renamed
    /// kind also reads the rows stored under its old name.
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub position: u64,
    pub at_unix_ms: u64,
    pub kind: String,
    pub session_id: Option<String>,
    pub turn_id: Option<String>,
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerTailResult {
    pub rows: Vec<LedgerEntry>,
    pub total: u64,
}

// ---------------------------------------------------------------- M3: content

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionRef {
    pub session_id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionHistoryParams {
    pub session_id: String,
    /// Newest `n` nodes (default all).
    #[serde(default)]
    pub n: Option<usize>,
}

/// One node as clients render it (a message, a tool call, a tool result).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub node_id: String,
    /// `user_message`, `assistant_message`, `tool_call`, `tool_result`.
    pub kind: String,
    pub session_id: String,
    pub position: u64,
    pub at_unix_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub loop_index: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    /// Text for display (user text; assistant text blocks; tool result content).
    #[serde(default)]
    pub text: String,
    /// Thinking summaries (assistant), when the provider returned them.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub thinking: String,
    /// Kind-specific fields: model/usage/cost (assistant), tool/input/gate
    /// (tool call), tool/status/is_error/bytes (tool result).
    #[serde(default)]
    pub detail: Value,
    #[serde(default)]
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionHistoryResult {
    pub session: SessionInfo,
    pub nodes: Vec<NodeInfo>,
    /// Actions waiting for the operator's confirmation in this session.
    #[serde(default)]
    pub pending_confirms: Vec<ConfirmRequest>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionRecompileParams {
    pub session_id: String,
    /// `fresh` (start over) or `transcript` (keep everything, thinking stripped).
    pub strategy: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodeListParams {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub kind: Option<String>,
    #[serde(default)]
    pub n: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeListResult {
    pub nodes: Vec<NodeInfo>,
    pub total: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompilationListParams {
    #[serde(default)]
    pub session_id: Option<String>,
    #[serde(default)]
    pub n: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationInfo {
    pub compilation_id: String,
    pub session_id: String,
    pub created_at_ms: u64,
    pub trigger: String,
    pub strategy: String,
    pub as_of: u64,
    pub includes: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from: Option<String>,
    /// The manifest as stored (model, provider, digests, catalog version, strip_thinking).
    pub manifest: Value,
    /// This is the session's current compilation.
    #[serde(default)]
    pub current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompilationListResult {
    pub compilations: Vec<CompilationInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogModel {
    pub model: String,
    /// The catalog row as configured (provider, window, prices, capabilities, source).
    pub entry: Value,
    /// Profiles that use this model.
    #[serde(default)]
    pub profiles: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CatalogListResult {
    pub version: String,
    pub models: Vec<CatalogModel>,
}

/// A tool call waiting for the operator (spec §3.9: confirmation is bound to
/// the exact tool, arguments, resource, and an expiry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmRequest {
    pub correlation_id: String,
    pub session_id: String,
    pub execution_id: String,
    pub tool: String,
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    /// Why policy asks (e.g. "write under /home/x/projects", "run cargo test").
    pub reason: String,
    pub by: String,
    pub requested_at_ms: u64,
    pub expires_at_ms: u64,
    /// The floor asks: the call touches Theseus's own binary or state, or the
    /// 1Password CLI or token. It asks at every posture.
    #[serde(default)]
    pub floor: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ActionConfirmParams {
    pub correlation_id: String,
    pub approve: bool,
    #[serde(default)]
    pub note: Option<String>,
    /// Subscribe this connection to the session's events before the answer
    /// wakes the execution, so the continuation turn is seen from its start.
    #[serde(default)]
    pub watch: bool,
    /// Who answered, as a label (e.g. `discord:eddie`). Default: the connection.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionConfirmResult {
    pub correlation_id: String,
    pub approved: bool,
    pub session_id: String,
    pub execution_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Canonical name (`fs.read`).
    pub name: String,
    /// Name on the wire (`fs_read`).
    pub wire_name: String,
    pub family: String,
    pub description: String,
    /// `read`, `write`, or `run`.
    pub class: String,
    /// `inproc` or `job`.
    pub backend: String,
    /// Its posture today: `open`, `notify`, or `approve`.
    pub policy: String,
    pub input_schema: Value,
    /// Calls since the daemon started.
    #[serde(default)]
    pub calls: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolListResult {
    pub tools: Vec<ToolInfo>,
    /// Workspace roots tools may touch.
    pub roots: Vec<String>,
    /// `proc.run` calls over all tool calls (spec §3.23 shell-fallback ratio).
    pub shell_fallback_ratio: f64,
    /// Calls since the daemon started.
    pub calls_total: u64,
}

/// `error.data` on a provider failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderErrorData {
    pub class: String,
    pub transient: bool,
    pub usage_unknown: bool,
    pub turn_id: Option<String>,
    pub session_id: String,
    pub elapsed_ms: u64,
    /// The trace up to the failure.
    #[serde(default)]
    pub trace: Option<Span>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnStarted {
    pub session_id: String,
    pub turn_id: String,
    #[serde(default)]
    pub execution_id: Option<String>,
    #[serde(default)]
    pub continuation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnFailed {
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub execution_id: Option<String>,
    #[serde(default)]
    pub continuation: bool,
    /// Error class when known (rate_limited, overloaded, auth, ...).
    #[serde(default)]
    pub class: Option<String>,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopStarted {
    pub turn_id: String,
    pub loop_index: u32,
    pub model: String,
    pub tools_offered: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelDelta {
    pub turn_id: String,
    pub loop_index: u32,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoopEnded {
    pub turn_id: String,
    pub loop_index: u32,
    pub provider_stop_reason: Option<String>,
    pub tool_calls: u32,
    pub advancer: String,
    pub decision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookInfo {
    pub event: String,
    pub kind: String,
    pub handlers: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandlerInfo {
    pub event: String,
    pub handler_id: String,
    pub client: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HooksListResult {
    pub events: Vec<HookInfo>,
    pub handlers: Vec<HandlerInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HooksRegisterParams {
    pub event: String,
    pub handler_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HooksRegisterResult {
    pub event: String,
    pub kind: String,
    pub handler_id: String,
}

/// Delivered to a remote handler registered for an Observe-kind hook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEventNotification {
    pub event: String,
    pub handler_id: String,
    pub turn_id: Option<String>,
    pub session_id: Option<String>,
    pub payload: Value,
}

pub fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
