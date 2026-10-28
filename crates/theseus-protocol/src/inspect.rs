//! `turn.inspect` (theseus-x9swu): one turn, whole, for the cockpit's
//! inspector at `/session/<id>/turn/<turn>`: its prompt, every loop's
//! compile and answer (thinking included), every tool call with its input and
//! result, the turn's ledger rows, and its trace. Assembled from the store's
//! nodes, ledger and `turn.trace` row in one read; it writes nothing and
//! stores nothing of its own.
//!
//! Thinking, a request's text and a tool's input and result are private: a
//! surface that does not read private places gets the shapes, the sizes and
//! the timings, and `withheld` says why.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{ContextCompiled, GateRecord, Span, Usage};

/// The link to one turn in the cockpit: `<base>/session/<session>/turn/<turn>`.
/// A base's trailing slash is dropped. The link carries no key or token.
pub fn turn_url(base: &str, session_id: &str, turn_id: &str) -> String {
    format!(
        "{}/session/{session_id}/turn/{turn_id}",
        base.trim_end_matches('/')
    )
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TurnInspectParams {
    pub session_id: String,
    pub turn_id: String,
}

/// The turn's input.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct InspectPrompt {
    pub node_id: Option<String>,
    pub author: Option<String>,
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
    /// Empty when withheld, or for a continuation (no input).
    pub text: String,
    pub chars: u64,
}

/// The turn's totals.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct InspectTotals {
    pub loops: u32,
    pub model_calls: u32,
    pub tool_calls: u32,
    pub usage: Usage,
    pub cost_usd: f64,
    /// Tool results with `is_error`, provider errors and refusals.
    pub errors: u32,
    #[cfg_attr(test, ts(type = "number"))]
    pub wall_ms: u64,
}

/// One ledger row of the turn, as it was written.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct InspectRow {
    #[cfg_attr(test, ts(type = "number"))]
    pub position: u64,
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
    pub kind: String,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub data: Value,
}

/// One answer of the model, a loop's.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct InspectAnswer {
    pub node_id: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub at_ms: u64,
    pub model: String,
    pub provider: String,
    pub stop_reason: Option<String>,
    pub usage: Usage,
    pub cost_usd: Option<f64>,
    pub request_id: Option<String>,
    pub compilation_id: Option<String>,
    pub request_digest: Option<String>,
    pub effort: Option<String>,
    /// The content blocks as the provider returned them (thinking, text,
    /// tool_use). A private reader gets the thinking in full; any other gets
    /// each block's type and length alone.
    #[cfg_attr(test, ts(type = "Array<unknown>"))]
    pub blocks: Vec<Value>,
    /// The thinking blocks' characters, whether or not shown.
    pub thinking_chars: u64,
    /// The requests this loop sent that carried values the vault resolved
    /// (`request.redacted` rows, theseus-gyjyo R7b): which secret by name, how
    /// many times, and whether the exit substituted it. A retry sends a
    /// request again, so a loop may have more than one. A key the vault never
    /// resolved is not here: it is in the record, found by reading it.
    pub redacted: Vec<InspectRedacted>,
}

/// One request's vault secrets: what `request.redacted` says.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct InspectRedacted {
    /// The secrets' names, sorted. Empty for a surface that reads no private
    /// text (`withheld`): it learns how many and whether substituted.
    pub names: Vec<String>,
    #[cfg_attr(test, ts(type = "number"))]
    pub count: u64,
    /// `true`: replaced by `[redacted:<name>]` before the request left.
    /// `false`: sent as written (`[privacy] redact_outbound = false`).
    pub substituted: bool,
    /// The provider action's correlation id.
    pub correlation_id: Option<String>,
}

/// One tool call of a loop: the call, its gate's record, its result.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct InspectCall {
    pub tool_use_id: String,
    pub tool: String,
    pub call_node: String,
    /// The kernel's id of the call's action: the ledger rows that name it
    /// (`action.*`, `tool.*`) carry it as their `correlation_id`.
    pub correlation_id: Option<String>,
    pub result_node: Option<String>,
    #[cfg_attr(test, ts(type = "number"))]
    pub called_ms: u64,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub resulted_ms: Option<u64>,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub input: Value,
    pub gate: Option<Box<GateRecord>>,
    pub status: Option<String>,
    pub is_error: bool,
    /// The result as the model saw it.
    pub content: String,
    #[cfg_attr(test, ts(type = "number"))]
    pub bytes_total: u64,
    pub truncated: bool,
    /// Where the full output lives when `content` was capped.
    pub full_ref: Option<String>,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub duration_ms: Option<u64>,
    /// A background job's real result, arriving after its placeholder.
    pub late: bool,
}

/// One loop: what was compiled for it, the model's answer, and the calls it made.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct InspectLoop {
    #[serde(rename = "loop")]
    pub loop_index: u32,
    pub compiled: Option<ContextCompiled>,
    pub answer: Option<InspectAnswer>,
    pub calls: Vec<InspectCall>,
    /// The nodes this loop's request added to the one before's: their ids in
    /// order (the diff between consecutive requests), `[]` for the first.
    pub added: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct TurnInspectResult {
    pub session_id: String,
    pub turn_id: String,
    pub prompt: InspectPrompt,
    /// The turn in the cockpit (`[cockpit] public_url`): what `theseus open turn` opens. Empty when
    /// no web UI runs and no `public_url` names one.
    pub url: String,
    /// `ended`, `failed`, `refused` or `running`.
    pub outcome: String,
    pub stop_reason: Option<String>,
    #[cfg_attr(test, ts(type = "number"))]
    pub started_ms: u64,
    #[cfg_attr(test, ts(type = "number | null"))]
    pub ended_ms: Option<u64>,
    pub totals: InspectTotals,
    /// The route decision (`route.decided`), as the ledger has it; with none, the
    /// target the turn started on (profile, provider, model).
    #[cfg_attr(test, ts(type = "unknown"))]
    pub route: Value,
    pub loops: Vec<InspectLoop>,
    /// Every ledger row of the turn, in order (the oldest 2,000).
    pub rows: Vec<InspectRow>,
    pub trace: Option<Span>,
    pub prev_turn: Option<String>,
    pub next_turn: Option<String>,
    /// Why text was left out (a surface that does not read private places).
    pub withheld: Option<String>,
    /// The session's data state in a line (theseus-a1wvq.5: opened, the open
    /// run's ends, the taint), for a private place; absent with none.
    #[serde(skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub data: Option<String>,
    pub ms: f64,
}
