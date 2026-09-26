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
    /// Theseus-specific: the gate refused (fail closed).
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
        Self {
            jsonrpc: JSONRPC.into(),
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: Value::Null,
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
    pub secrets_resolved: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Conversation,
    Task,
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
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub usage: Usage,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnStarted {
    pub session_id: String,
    pub turn_id: String,
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
