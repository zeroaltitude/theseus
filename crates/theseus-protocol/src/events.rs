//! The notifications, typed (theseus-0g4, finding 12): one struct per shape,
//! and `Event`, every `notify::*` method with its params. Senders build an
//! `Event` and clients match on one, so a field's name lives here only.
//! Field names are the wire's; `tests/wire.rs` holds each one's bytes.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::gate::{GateRecord, Notice};
use crate::{
    notify, ConfirmRequest, EventsLost, ExecutionView, LoopEnded, LoopStarted, Message, ModelDelta,
    NarrativeLine, Notification, ProfileChanged, TightenResult, TrustResult, TurnFailed,
    TurnStarted, TurnSubmitResult,
};

/// `tool.proposed`: the model asked for a call, and what the gate made of it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ToolProposed {
    pub session_id: String,
    pub turn_id: String,
    pub tool_use_id: String,
    pub tool: String,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub input: Value,
    pub gate: GateRecord,
}

/// A context file as the system block carried it (theseus-58a).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ContextFileRef {
    /// The file, `~` expanded.
    pub path: String,
    /// The first 16 hex digits of the SHA-256 of the text included; absent
    /// when the file could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub digest: Option<String>,
    /// Bytes of the file the block carries.
    #[serde(default)]
    pub bytes: u64,
    /// The file was longer than the cap, and the block carries its start.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cut: bool,
    /// Why the file could not be read: `not found`, `permission denied`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub missing: Option<String>,
    /// The persona whose file this is (theseus-c48); absent: the system
    /// level, which every session gets (and every file before theseus-c48).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub persona: Option<String>,
    /// Its entry says `readers = "public"`: a shared place may carry it (the
    /// place rule). Absent: the owner's alone.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub public: bool,
    /// The block carries its header alone, and why: a shared place's request,
    /// and a file not marked public (the place rule).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub withheld: Option<String>,
}

/// Where a request's cache breakpoints went, and their TTLs (theseus-ev1).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct CacheSummary {
    /// The blocks marked, in order: `header`, `context`, `conversation`.
    pub breakpoints: Vec<String>,
    /// The system blocks' TTL: `5m` or `1h`.
    pub ttl: String,
    /// The conversation's TTL, at most `ttl`.
    pub conversation_ttl: String,
}

/// How the compiler sized a request (theseus-f5hf): the provider's own count
/// of what the request repeats, and the rest from its bytes at the catalog's
/// figures.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct EstimateSummary {
    /// What the provider will count: `counted` + `estimated`.
    pub tokens: u64,
    /// `counted` (part of it is the provider's count) or `bytes` (all of it
    /// is from bytes).
    pub method: String,
    /// The provider's count of this compilation's last request (its input,
    /// cache reads, and cache writes), plus its answer's output tokens.
    pub counted: u64,
    /// The rest, from its bytes at the catalog's `bytes_per_token`.
    pub estimated: u64,
    /// What the ring checks: `counted`, plus `estimated` and its margin.
    pub upper: u64,
    /// The request's bytes as JSON, base64 image data left out: the estimate
    /// before theseus-f5hf was a fourth of it.
    pub bytes: u64,
    /// The whole request's bytes by class.
    pub census: CensusSummary,
}

/// A request's bytes by how densely a tokenizer reads them (theseus-f5hf).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct CensusSummary {
    /// Tool schemas, tool inputs, and tool results, in bytes.
    pub json: u64,
    /// The system's text and the messages' text and thinking, in bytes.
    pub text: u64,
    /// Thinking signatures and redacted thinking, in bytes.
    pub opaque: u64,
    /// Messages, and the system's blocks.
    pub messages: u64,
    /// Content blocks.
    pub blocks: u64,
    /// Tool call ids, in `tool_use` and `tool_result` blocks.
    pub ids: u64,
}

/// `context.compiled`: the context a loop's request was compiled from
/// (§3.13). The `context.compiled` ledger row and the trace's compile span
/// carry the same.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ContextCompiled {
    pub session_id: String,
    pub turn_id: String,
    #[serde(rename = "loop")]
    pub loop_index: u32,
    /// `recompile` (a new compilation) or `append`.
    pub decision: String,
    /// What made it recompile (`new_session`, `window`, `manual_fresh`, …);
    /// `null` for an append.
    pub trigger: Option<String>,
    pub compilation_id: String,
    /// The compilation's strategy: `fresh` or `transcript`.
    pub strategy: String,
    pub prefix_nodes: u64,
    pub tail_nodes: u64,
    pub messages: u64,
    pub est_tokens: u64,
    /// How `est_tokens` was reached (theseus-f5hf); absent in rows written
    /// before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub estimate: Option<EstimateSummary>,
    pub digest: String,
    /// The tool calls that had no result, given a synthetic one.
    pub repairs: Vec<String>,
    /// Tools offered.
    pub tools: u64,
    pub nodes_scanned: u64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_files: Vec<ContextFileRef>,
    /// The persona in play (theseus-c48).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub persona: Option<String>,
    pub cache: CacheSummary,
    /// The class of the place the turn speaks in (the place rule,
    /// theseus-nbsh), fixed for the turn; absent from a daemon before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub class: Option<crate::PlaceClass>,
    /// Context files the request carries as their headers alone: a shared
    /// place's that are not marked public.
    #[serde(default, skip_serializing_if = "crate::is_zero")]
    pub withheld: u64,
}

/// `tool.started`: a call runs. A job's says how, and what the broker gave
/// it; a call that is not a job's has none of those fields.
#[cfg_attr(test, derive(ts_rs::TS))]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolStarted {
    pub session_id: String,
    pub turn_id: String,
    pub tool_use_id: String,
    pub tool: String,
    pub correlation_id: String,
    /// `harness`, `inproc`, or `job`.
    pub backend: String,
    /// A job's wrapper's pid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub argv: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub cwd: Option<PathBuf>,
    /// What the secret broker gave a job, by name (`gh got GH_TOKEN`): `null`
    /// for nothing (theseus-dcy).
    #[serde(default, skip_serializing_if = "Option::is_none", with = "nullable")]
    #[cfg_attr(test, ts(type = "string | null"))]
    pub granted: Option<Option<String>>,
    /// What a job was not given (`git got no GIT_TOKEN`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub withheld: Option<Vec<String>>,
    /// A job's class (M4 17b): `l1` when it runs in the sandbox. Absent: L0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub class: Option<String>,
    /// An L1 job's egress list (M4 18c): the hosts its proxy lets it reach,
    /// empty for no network at all. Absent at L0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub egress: Option<Vec<String>>,
}

/// A field that is absent, `null`, or a value: `None`, `Some(None)`, and
/// `Some(Some(v))` (`skip_serializing_if = "Option::is_none"` drops the first).
mod nullable {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<T: Serialize, S: Serializer>(
        v: &Option<Option<T>>,
        s: S,
    ) -> Result<S::Ok, S::Error> {
        v.as_ref().and_then(Option::as_ref).serialize(s)
    }

    pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>>(
        d: D,
    ) -> Result<Option<Option<T>>, D::Error> {
        Option::<T>::deserialize(d).map(Some)
    }
}

/// `tool.ended`: a call's result was written.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ToolEnded {
    pub session_id: String,
    pub turn_id: String,
    pub tool_use_id: String,
    pub tool: String,
    /// The result's status: `ok`, `error`, `declined`, `cancelled`,
    /// `background`, … (`denied` from a daemon before theseus-8az).
    pub status: String,
    pub duration_ms: Option<u64>,
    pub correlation_id: Option<String>,
    /// A background job's real result, after its placeholder.
    pub late: bool,
    pub truncated: bool,
    /// The whole output's size, before any cap.
    pub bytes: u64,
    pub node_id: String,
    /// A job's exit code.
    pub exit_code: Option<i64>,
    /// Who stopped it, when a `/stop` ended it (theseus-4uw).
    pub stopped_by: Option<String>,
    /// The result's first 2,000 characters.
    pub preview: String,
    /// An L1 job's scratch (M4 17b): what it wrote there, which was
    /// discarded (`wrote 3 files, 41 KB, to scratch: target/…; discarded`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub scratch: Option<String>,
    /// The hosts an L1 job reached through its egress (M4 18c):
    /// `reached api.github.com:443 (2 connections)`. Absent when it reached
    /// none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reached: Option<String>,
    /// How a call a cancel or a stop ended is known to have stopped (M4
    /// 18a): "verified: pid namespace, 4 processes", or "not verified: …".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub verified: Option<String>,
}

/// `confirm.resolved`: a question waiting for the operator closed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ConfirmResolved {
    pub session_id: String,
    pub correlation_id: String,
    pub approved: bool,
    /// Who answered or closed it; absent when new input superseded a call's
    /// question.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub by: Option<String>,
    /// An answer to a tool call: whether it trusted the session again
    /// (theseus-9bp).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub trust: Option<bool>,
    /// New input came before an answer.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub superseded: bool,
    /// Its execution was cancelled before an answer (theseus-w98).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub cancelled: bool,
    /// A `/stop` declined it while it waited (W1).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub stopped: bool,
    /// A raised spend limit withdrew a budget question (theseus-3pj).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub withdrawn: bool,
    /// Nobody answered a call's question by the time it said it expires
    /// (`[kernel] confirm_ttl_secs` after it was asked, theseus-830): the
    /// call did not run.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub expired: bool,
}

/// `node.written`: a turn wrote a node.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct NodeWritten {
    pub session_id: String,
    pub node_id: String,
    /// `user_message`, `assistant_message`, `tool_call`, `tool_result`.
    pub kind: String,
}

/// `policy.notified`: a call ran at a `notify` posture, with its notice.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct PolicyNotified {
    pub session_id: String,
    pub turn_id: String,
    pub tool_use_id: String,
    pub correlation_id: String,
    pub tool: String,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub input: Value,
    /// The plan's summary.
    pub summary: String,
    #[serde(flatten)]
    pub notice: Notice,
    /// What the secret broker gives the call, by name; `null` for nothing.
    pub granted: Option<String>,
    /// The task that made the call, by its short id (DD7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub task: Option<String>,
}

/// The process behind an answer, as the process tree says (theseus-6qy).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct Asker {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub pid: Option<u32>,
    /// Its program: the file name of its `argv[0]`, else its `comm`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub argv0: Option<String>,
    /// How long the trace took, in µs.
    pub trace_us: u64,
    /// The job whose wrapper is above it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub job: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub wrapper_pid: Option<u32>,
    /// It is under this daemon with no wrapper between: a job's orphan
    /// (theseus-z4b). The daemon's pid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub under_daemon: Option<u32>,
    /// It is under another serving daemon (theseus-6uo). That daemon's pid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub under_other_daemon: Option<u32>,
    /// Why it could not be traced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub untraceable: Option<String>,
}

/// `approval.refused`: an answer, an undo, or a trust from a Theseus job's
/// process, refused (theseus-6qy). A security event every connection hears.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ApprovalRefused {
    /// The method of the act: `action.confirm`, `policy.tighten`,
    /// `policy.untighten`, or `policy.trust`.
    pub act: String,
    /// The session it was about; `null` for a tool's tightening.
    pub session_id: Option<String>,
    /// An answer's call.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub correlation_id: Option<String>,
    /// The answer's call's tool, or the tightening's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub tool: Option<String>,
    /// An answer's: approve or decline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub approve: Option<bool>,
    /// The connection it came through (`sock#9`), its surface (`cli`), why it
    /// did not count, and the label it gave.
    pub who: String,
    pub via: String,
    pub why: String,
    pub by: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub asker: Option<Asker>,
    /// Always set: the asker was a job's.
    pub from_job: bool,
}

/// Every notification as `(variant, its params, method)`: `Event` and its
/// conversions come from this one table.
macro_rules! events {
    ($($(#[$doc:meta])* $variant:ident($params:ty) = $method:path,)*) => {
        /// A notification, typed: what a sender builds and a client matches on.
        /// An event lives for one send or one render, so its size is no cost.
        #[allow(clippy::large_enum_variant)]
        #[derive(Debug, Clone)]
        pub enum Event {
            $($(#[$doc])* $variant($params),)*
        }

        impl Event {
            /// Every variant's name with its method, from the same table. The
            /// reader rule's registry test (theseus-wjy) checks that each
            /// `notify::*` name has a variant, and looks for each one's sender.
            pub const VARIANTS: &[(&str, &str)] = &[$((stringify!($variant), $method),)*];

            /// Its method, one of `notify::*`.
            pub fn method(&self) -> &'static str {
                match self {
                    $(Event::$variant(_) => $method,)*
                }
            }

            /// The notification a connection writes.
            pub fn notification(&self) -> Notification {
                match self {
                    $(Event::$variant(p) => Notification::new($method, p),)*
                }
            }

            /// A notification as a client reads it: `Ok(None)` for a method
            /// this build does not know (a newer daemon's), an error for a
            /// known one whose params do not decode.
            pub fn from_notification(
                method: &str,
                params: &Value,
            ) -> Result<Option<Event>, serde_json::Error> {
                Ok(Some(match method {
                    $($method => Event::$variant(serde_json::from_value(params.clone())?),)*
                    _ => return Ok(None),
                }))
            }
        }
    };
}

events! {
    TurnStarted(TurnStarted) = notify::TURN_STARTED,
    LoopStarted(LoopStarted) = notify::LOOP_STARTED,
    /// Streamed text.
    ModelDelta(ModelDelta) = notify::MODEL_DELTA,
    /// A thinking summary as it streams: the same shape as a delta.
    ModelThinking(ModelDelta) = notify::MODEL_THINKING,
    ToolProposed(ToolProposed) = notify::TOOL_PROPOSED,
    LoopEnded(LoopEnded) = notify::LOOP_ENDED,
    /// The turn's result, as `turn.submit` returns it.
    TurnEnded(TurnSubmitResult) = notify::TURN_ENDED,
    ProfileChanged(ProfileChanged) = notify::PROFILE_CHANGED,
    ContextCompiled(ContextCompiled) = notify::CONTEXT_COMPILED,
    ToolStarted(ToolStarted) = notify::TOOL_STARTED,
    ToolEnded(ToolEnded) = notify::TOOL_ENDED,
    ConfirmRequested(ConfirmRequest) = notify::CONFIRM_REQUESTED,
    ConfirmResolved(ConfirmResolved) = notify::CONFIRM_RESOLVED,
    NodeWritten(NodeWritten) = notify::NODE_WRITTEN,
    TurnFailed(TurnFailed) = notify::TURN_FAILED,
    PolicyNotified(PolicyNotified) = notify::POLICY_NOTIFIED,
    PolicyTightened(TightenResult) = notify::POLICY_TIGHTENED,
    PolicyUntightened(TightenResult) = notify::POLICY_UNTIGHTENED,
    SessionTrusted(TrustResult) = notify::SESSION_TRUSTED,
    ApprovalRefused(ApprovalRefused) = notify::APPROVAL_REFUSED,
    NarrativeLine(NarrativeLine) = notify::NARRATIVE_LINE,
    /// An execution's view, after a frame changed it (theseus-in3).
    ExecutionChanged(ExecutionView) = notify::EXECUTION_CHANGED,
    /// The connection fell behind and dropped notifications (theseus-in3).
    EventsLost(EventsLost) = notify::EVENTS_LOST,
}

impl Event {
    /// The session it names, if it names one.
    pub fn session_id(&self) -> Option<&str> {
        match self {
            Event::TurnStarted(e) => Some(&e.session_id),
            Event::ToolProposed(e) => Some(&e.session_id),
            Event::TurnEnded(e) => Some(&e.session_id),
            Event::ContextCompiled(e) => Some(&e.session_id),
            Event::ToolStarted(e) => Some(&e.session_id),
            Event::ToolEnded(e) => Some(&e.session_id),
            Event::ConfirmRequested(e) => Some(&e.session_id),
            Event::ConfirmResolved(e) => Some(&e.session_id),
            Event::NodeWritten(e) => Some(&e.session_id),
            Event::TurnFailed(e) => Some(&e.session_id),
            Event::PolicyNotified(e) => Some(&e.session_id),
            Event::SessionTrusted(e) => Some(&e.session_id),
            Event::ApprovalRefused(e) => e.session_id.as_deref(),
            Event::NarrativeLine(e) => e.session_id.as_deref(),
            Event::ExecutionChanged(e) => Some(&e.session_id),
            Event::LoopStarted(_)
            | Event::ModelDelta(_)
            | Event::ModelThinking(_)
            | Event::LoopEnded(_)
            | Event::ProfileChanged(_)
            | Event::PolicyTightened(_)
            | Event::PolicyUntightened(_)
            | Event::EventsLost(_) => None,
        }
    }

    /// The turn it belongs to, if it belongs to one.
    pub fn turn_id(&self) -> Option<&str> {
        match self {
            Event::TurnStarted(e) => Some(&e.turn_id),
            Event::LoopStarted(e) => Some(&e.turn_id),
            Event::ModelDelta(e) | Event::ModelThinking(e) => Some(&e.turn_id),
            Event::ToolProposed(e) => Some(&e.turn_id),
            Event::LoopEnded(e) => Some(&e.turn_id),
            Event::TurnEnded(e) => Some(&e.turn_id),
            Event::ContextCompiled(e) => Some(&e.turn_id),
            Event::ToolStarted(e) => Some(&e.turn_id),
            Event::ToolEnded(e) => Some(&e.turn_id),
            Event::PolicyNotified(e) => Some(&e.turn_id),
            Event::TurnFailed(e) => e.turn_id.as_deref(),
            Event::NarrativeLine(e) => e.turn_id.as_deref(),
            Event::ProfileChanged(_)
            | Event::ConfirmRequested(_)
            | Event::ConfirmResolved(_)
            | Event::NodeWritten(_)
            | Event::PolicyTightened(_)
            | Event::PolicyUntightened(_)
            | Event::SessionTrusted(_)
            | Event::ApprovalRefused(_)
            | Event::ExecutionChanged(_)
            | Event::EventsLost(_) => None,
        }
    }
}

impl From<Event> for Message {
    fn from(e: Event) -> Self {
        Message::Notification(e.notification())
    }
}
