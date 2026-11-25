//! The Theseus wire protocol.
//!
//! JSON-RPC 2.0, one message per line, UTF-8, on any byte stream (stdio, a Unix
//! domain socket, or an in-process channel). This crate is types only: every
//! client, including the in-binary CLI, links this and nothing else from the
//! core, so no client has a privileged path into the kernel.
//!
//! Requests change state and get exactly one response. Notifications report
//! state and are also ledger rows on the server side.

mod actions;
pub mod answer;
pub mod arrangement;
mod aws;
pub mod bench;
pub mod books;
mod budgets;
pub mod calendar;
pub mod cancel;
pub mod card;
pub mod census;
pub mod check;
pub mod cite;
pub mod content;
pub use content::{
    ContentUnlabelParams, ItemFlag, ItemLabelsView, ItemQuestion, LabelLink, LabelRaised,
    LabelView, UnlabelResult,
};
pub mod context;
mod cost;
pub mod exchange;
pub mod own_command;
pub mod own_file;
pub mod subagent;
pub use subagent::SubagentAnswer;
pub mod cred;
pub mod defer;
pub mod diff;
pub mod digest_google;
mod events;
mod exec_list;
mod explain;
pub mod extend;
pub mod features;
pub mod files_left;
pub mod fill;
pub mod flow;
pub mod forecast;
pub mod fork;
mod gate;
pub mod google;
pub mod grants;
mod hands;
mod health;
mod history;
pub mod human_time;
pub mod import;
pub mod import_sync;
pub mod index;
pub mod initiative;
pub mod plain;
pub use plain::PlainWords;
pub mod config_history;
pub mod config_whole;
pub mod inspect;
pub mod job_follow;
pub mod jsonrpc;
pub mod judge;
pub mod judge_runs;
pub mod later;
pub mod learning;
mod ledger;
mod ledger_verify;
pub mod lsp;
pub mod mcp;
pub mod mcp_server;
pub mod memory;
pub mod modes;
pub mod money;
mod names;
pub mod narrative_watch;
mod nodes;
pub mod notice;
pub mod notices;
pub mod nudge;
mod ontology;
pub mod ops;
pub mod outbox;
pub mod own_bench;
pub mod owner_proof;
pub mod packs;
pub mod patterns;
pub mod persona;
pub mod persona_import;
pub mod persona_write;
mod places;
pub mod projects;
mod push;
pub mod resident;
pub mod retract;
mod retry;
pub mod rewind;
pub mod route;
pub mod routines;
pub mod rsi;
pub mod sandbox;
pub mod sessions;
pub mod signals;
pub mod standing;
pub mod standing_proposals;
mod status;
mod submit;
pub mod surface;
pub mod task_not_this;
pub mod task_pause;
pub mod task_start;
pub mod task_undo;
pub mod tasks;
pub mod term;
#[cfg(test)]
mod tests_task_info;
pub mod thread_cache;
pub mod thread_route;
pub mod threads;
mod tighten;
pub mod time;
mod toolinfo;
#[cfg(test)]
mod ts;
pub mod vault_keep;
pub mod view;
pub mod voice;
pub mod voice_pick;
pub mod wakes;
pub mod warm;
pub mod watches;
pub mod why;
mod window;
pub mod work;
mod work_digest;
pub mod work_system;
mod work_watch;

pub use actions::{ActionInfo, ActionListParams, ActionListResult};
pub use arrangement::{ArrangementPiece, TaskArrangement};
pub use aws::*;
pub use budgets::*;
pub use cancel::{
    CancelCount, CancelVerdict, ExecutionCancelParams, ExecutionCancelResult, ExecutionStopParams,
    ExecutionStopResult, JobKept, StopAgent,
};
pub use card::*;
pub use check::{CheckOverlap, CheckPiece, TaskCheck};
pub use config_history::{ConfigHistoryParams, ConfigHistoryResult};
pub use config_whole::{has_whole_text, ConfigWholeParams, ConfigWholeResult, CONFIG_ACK};
pub use diff::{ConfirmPreview, ToolDiff};
pub use events::*;
pub use exec_list::{ExecutionListParams, ExecutionListResult};
pub use explain::*;
pub use fork::{ForkOrigin, Inherits, ReachFork, SessionForkParams, SessionForkResult};
pub use gate::*;
pub use hands::*;
pub use health::*;
pub use history::*;
pub use index::TenderStatus;
pub use initiative::{Initiative, Proactive};
pub use job_follow::*;
pub use ledger::*;
pub use ledger_verify::*;
pub use names::{method, notify};
pub use nodes::*;
pub use ontology::*;
pub use outbox::OutboxStatus;
pub use persona::{
    PersonaFile, PersonaInfo, PersonaListParams, PersonaListResult, PersonaPlace, PersonaTally,
};
pub use persona_import::*;
pub use persona_write::*;
pub use places::*;
pub use push::*;
pub use retry::*;
pub use rewind::{
    FileChange, FilesChangesParams, FilesChangesResult, FilesRestoreParams, FilesRestoreResult,
    RestoredFile, RewindPoint,
};
pub use sessions::{RetiredReason, SessionLink, SessionRetired, SessionState};
pub use standing::*;
pub use standing_proposals::*;
pub use status::*;
pub use submit::{SessionOpenParams, TurnSubmitParams};
pub use tasks::{TaskDetail, TaskListParams, TaskListResult};
pub use term::{TerminalInfo, TerminalLeft};
pub use tighten::*;
pub use time::now_unix_ms;
pub use toolinfo::{ToolInfo, ToolListResult};
pub use wakes::*;
pub use watches::{
    WatchCheck, WatchCreateParams, WatchCreateResult, WatchInfo, WatchListParams, WatchListResult,
    WatchThen,
};
pub use window::*;
pub use work_digest::*;
pub use work_watch::*;

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const VERSION: &str = "0.1";
pub const JSONRPC: &str = "2.0";

/// The largest image an attachment or `fs.read` passes to a model
/// (theseus-9g2): 5 MiB of raw bytes, which every route takes (Anthropic's
/// direct API allows 10 MB of base64; Bedrock and Vertex allow 5 MB).
pub const MAX_IMAGE_BYTES: u64 = 5 * 1024 * 1024;

/// The `tool` of a budget question (theseus-0sg): a session reached its spend
/// limit, and a `confirm.requested` with this tool asks the operator whether
/// its spend may go back to $0. Answered with `action.confirm` like any other.
pub const BUDGET_TOOL: &str = "budget.reset";

/// The `tool` of a provider call's action. It is authorized in the frame after its plan and never asks the operator.
pub const PROVIDER_TOOL: &str = "provider.messages";

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(untagged)]
pub enum Id {
    Num(u64),
    Str(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Request {
    pub jsonrpc: String,
    pub id: Id,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub params: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Notification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub params: Value,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct RpcError {
    pub code: i64,
    #[serde(default)]
    pub message: String,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub data: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Response {
    pub jsonrpc: String,
    pub id: Id,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub error: Option<RpcError>,
}

/// Any line on the wire.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(untagged)]
pub enum Message {
    Request(Request),
    Response(Response),
    Notification(Notification),
}

pub mod error_code {
    pub const PARSE: i64 = -32700;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL: i64 = -32603;
    // Theseus-specific codes. -32001 was `BLOCKED`, never sent.
    pub const NOT_FOUND: i64 = -32002;
    pub const PROVIDER: i64 = -32003;
    /// The config turns this feature off (`narrative.watch` without `narrative = true`).
    pub const DISABLED: i64 = -32004;
    /// An answer to a waiting call that does not count (theseus-sgh, spec
    /// §3.9 "Approval"): not from a trusted user, or not through a trusted
    /// channel. The message says why; the call keeps waiting.
    pub const REFUSED: i64 = -32005;
    // -32006 was `CONFIG_UNCONFIRMED`, the act-gate's refusal (theseus-2fo),
    // retired when the daemon began acting on its config copy (theseus-zmgb).
    /// A bound was reached (theseus-in3): what one connection may hold, 64
    /// parked `session.wait`s, or the daemon's connections (theseus-7vtp).
    pub const LIMIT: i64 = -32007;
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

/// One execution as the protocol shows it (spec §3.15).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    #[cfg_attr(test, ts(type = "unknown"))]
    pub wake: Value,
    /// `wake`, typed (theseus-in3): what it waits on, while it waits.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub waiting_on: Option<WaitingOn>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub reports_to: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ended_reason: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// What it needs from people, by `attention()` (theseus-in3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub attention: Option<Attention>,
}

/// An execution's budget in US dollars (theseus-0sg).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetInfo {
    pub limit_usd: f64,
    /// Settled costs since the execution opened or was last reset. The
    /// session's `cost_usd` is its lifetime total, which a reset never lowers.
    pub spent_usd: f64,
    pub reserved_usd: f64,
    pub held_unknown_usd: f64,
    pub available_usd: f64,
    /// Approved resets of the spend to $0.
    #[serde(default)]
    pub resets: u32,
    /// The budget question waiting for the operator (a correlation id).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub question: Option<String>,
    /// The unit budget a record stored before dollar budgets carried, as it
    /// was: `limit`, `spent`, `reserved`, `held_unknown`.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[cfg_attr(
        test,
        ts(
            type = "{ limit: number; spent: number; reserved: number; held_unknown: number } | null"
        )
    )]
    pub units_before: Value,
}

/// A task (DD7, theseus-qn2): a child session a conversation opened with
/// `task.create`, which works on its own and reports back to the place.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskInfo {
    /// Its session's id (`ses_…`).
    pub task_id: String,
    /// The last six characters of its id, which is how people name it
    /// (`/cancel a1b2c3`).
    pub short: String,
    pub execution_id: String,
    pub parent_session_id: String,
    pub parent_execution_id: String,
    /// The brief's first line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    /// Its execution's state: `queued`, `running`, `waiting`, `complete`, …
    pub state: String,
    /// What it waits on, while it waits: `actions` (a job), `confirm`,
    /// `budget`, or `input`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub waiting_on: Option<String>,
    /// Its spend under its carved limit (since any reset), and that limit.
    pub spent_usd: f64,
    pub limit_usd: f64,
    /// What its session has cost in all, resets included.
    pub cost_usd: f64,
    pub turns: u64,
    /// Questions it asks the operator now.
    #[serde(default)]
    pub pending_confirms: u32,
    /// Where its cards and report go (`discord:dm:<user>`), if anywhere.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub ended_reason: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// Its report starts its parent's next turn (W1, `wake_parent`).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wake_parent: bool,
    /// When a deferred task starts (theseus-rcgph): it is asleep until
    /// then, with no turn taken. Absent for a task that has started, and
    /// from a daemon before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub starts_at_ms: Option<u64>,
    /// What it needs from people, by `attention()` (theseus-in3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub attention: Option<Attention>,
    /// The messages its parent quoted to start it (M5 27).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub arrangement: Option<TaskArrangement>,
    /// A check task's basis (M5 28a): what it checks, and why it is
    /// independent of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub check: Option<TaskCheck>,
    /// Task levels above it and including it: 1 for a conversation's task
    /// (theseus-tecj5). Absent (0) from a daemon before it.
    #[serde(default, skip_serializing_if = "is_zero_depth")]
    pub depth: u32,
    /// The fan-out call that opened this task as one of its helpers
    /// (theseus-z9mze.6): the call's correlation id, which groups a call's
    /// helpers. Absent for any other task, and from a daemon before it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub fan_out: Option<String>,
}

fn is_zero_depth(d: &u32) -> bool {
    *d == 0
}

/// `task.cancel`: stop a task and its jobs, as `execution.cancel` does; the
/// place hears that it was cancelled, once.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskCancelParams {
    /// Its id, its execution's id, or the end of either (`a1b2c3`), as long
    /// as one task matches.
    pub task: String,
    /// Who asked, as the ledger names them. Default: the surface (`the CLI`,
    /// `the web UI`; DD8), or the connection's label on a surface no
    /// listener named.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskCancelResult {
    pub task: TaskInfo,
    /// Dispatched actions whose backends were asked to stop.
    pub cancelled_actions: Vec<String>,
    /// How each of them stopped, and how that is known (M4 18a).
    #[serde(default)]
    pub verdicts: Vec<CancelVerdict>,
}

/// `wake.list`: every pending wake, soonest first, or only one session's, or
/// only those whose turns post to one place.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeListParams {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    /// Only the wakes whose turns post to this place (`discord:dm:<user>`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub target: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeListResult {
    pub wakes: Vec<WakeInfo>,
}

/// `wake.cancel`: cancel a pending wake, so nothing fires.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeCancelParams {
    /// Its id, or the end of it (`a1b2c3`), as long as one wake matches.
    pub wake: String,
    /// Who asked, as the ledger names them. Default: the surface (`the CLI`,
    /// `the web UI`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct WakeCancelResult {
    /// The wake as it was before the cancel.
    pub wake: WakeInfo,
}

/// A question's task, when a task asks it (DD7): the card names it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct TaskRef {
    pub task_id: String,
    pub short: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
}

/// Also the kernel's: an execution stores it (`theseus_kernel::SessionKind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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

/// Every job's variable, L0 and L1, naming its session (theseus-b5cl): the
/// CLI sends it as `opened_from`. A job can strip it, so it is a light guard
/// under default trust, not a boundary.
pub const JOB_SESSION_ENV: &str = "THESEUS_SESSION";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    #[cfg_attr(test, ts(optional))]
    pub execution_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub execution_state: Option<String>,
    #[serde(default)]
    pub last_active_ms: u64,
    #[serde(default)]
    pub cost_usd: f64,
    #[serde(default)]
    pub tool_calls: u64,
    /// Profile/provider/model of the last turn (continuations reuse it).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub profile: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub compilation_id: Option<String>,
    /// First words of the first prompt, for pickers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title: Option<String>,
    /// Tool calls in this session waiting for the operator's confirmation.
    #[serde(default)]
    pub pending_confirms: u32,
    /// A task session's parent (DD7): the session that started it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub parent_session_id: Option<String>,
    /// A task's carved limit, in US dollars.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub limit_usd: Option<f64>,
    /// What its execution needs from people, by `attention()` (theseus-in3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub attention: Option<Attention>,
    /// Live, quiet or retired, as read (theseus-emqx, `sessions::derive`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub state: Option<SessionState>,
    /// Why and when it was retired; a busy session that reads live still
    /// carries a stored one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub retired: Option<SessionRetired>,
    /// The session its place moved to, and the one it replaced.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub superseded_by: Option<SessionLink>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub supersedes: Option<SessionLink>,
    /// Its earlier titles, oldest first, once it was re-titled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub title_was: Option<Vec<String>>,
    /// The directory it works in, as its client sent it (theseus-aab7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub dir: Option<String>,
    /// Where it started, as `session.list` reads it (theseus-6yka):
    /// `discord` when it speaks to a place there, `cli` when a terminal
    /// client gave it a directory, else absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub started_on: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub data: Option<ops::DataMark>,
    /// The content labels it carries (theseus-9ymz0), in the flags' order. Empty
    /// when it carries none, and from a daemon before them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<content::LabelView>,
    /// With `session.list`'s `with_text`: the words of its first user
    /// message and of its last message, cut short; and `prompts`, the head
    /// and the tail of each of its newest user messages, for a search.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub first_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub last_text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub prompts: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionListResult {
    pub sessions: Vec<SessionInfo>,
    /// With `n`: the `before` for the next page back while older sessions
    /// remain; absent at the first one, and without `n`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub older: Option<u64>,
    /// The window and the empty grace each `state` was derived with
    /// (theseus-emqx), so a client derives the same for a past moment.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub live_window_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub empty_grace_ms: Option<u64>,
}

/// A file sent with a turn's input (theseus-9g2): a Discord attachment, or
/// `theseus ask --attach`. Text travels as `text` and an image as base64
/// `data`. A file the sender did not read carries `not_read` instead, with
/// the reason, and the model is told it exists.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Attachment {
    pub name: String,
    /// As the sender knows it (`text/plain`, `image/png`); may be empty.
    #[serde(default)]
    pub media_type: String,
    /// Bytes of the whole file.
    #[serde(default)]
    pub size: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub text: Option<String>,
    /// The file's bytes, base64 (standard alphabet, padded).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub data: Option<String>,
    /// Why it was not read: too large, a type that is not read, a failed download.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub not_read: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProfileInfo {
    pub name: String,
    pub provider: String,
    pub model: String,
    /// Output cap per call (the API's `max_tokens`), not an input limit.
    #[serde(alias = "max_tokens")]
    pub max_output_tokens: u32,
    pub has_system: bool,
    pub live: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub origin: Option<String>,
    /// A compatibility output, always `true` from this daemon: the restricted-origin rule is gone
    /// (2026-11-21) and no label keeps a session off a profile. A cockpit page loaded from a build
    /// before that (its `ops/floor.ts`) greys every profile of a private or sensitive session whose
    /// `cleared` is not `true`, US ones included, until it is reloaded; this keeps such a page
    /// working. The current cockpit does not read it, so it is not in the generated types.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(skip))]
    pub cleared: Option<bool>,
    /// The effort the profile is configured at (`low` to `max`); absent when it sets none and the model's
    /// own default stands. The cockpit groups the profiles of one model by it (theseus-yxkww).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub effort: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProfileListResult {
    pub live: String,
    /// Where the live choice came from: "config" or "runtime" (persisted switch).
    pub live_source: String,
    pub profiles: Vec<ProfileInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProfileUseParams {
    pub name: String,
}

/// `profile.release`'s answer (theseus-ervz9): what the live profile was and is, and whether a runtime
/// pin was actually cleared. `previous == live` says nothing about that (the pin and the config's
/// `[model] live` are often the same name), so the fact is carried, not inferred.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProfileReleaseResult {
    /// A runtime pin (`profile.use`) was cleared; false when the live profile was already the config's.
    pub released: bool,
    pub previous: String,
    pub live: String,
    pub by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ProfileChanged {
    pub previous: String,
    pub live: String,
    pub by: String,
}

/// One timed span of a turn trace. Times are microseconds from the turn's
/// start; a mark has `end_us == start_us`. Children are in start order.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Span {
    pub name: String,
    /// turn | loop | provider | tool | mark | advancer | store | lock | compile.
    /// Traces stored before theseus-hco also hold `hook` spans.
    pub kind: String,
    pub start_us: u64,
    #[serde(default)]
    pub end_us: Option<u64>,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    #[cfg_attr(test, ts(type = "unknown"))]
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

fn is_zero(n: &u64) -> bool {
    *n == 0
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    #[serde(default)]
    pub cache_read_input_tokens: u64,
    /// Every cache write, whatever its TTL.
    #[serde(default)]
    pub cache_creation_input_tokens: u64,
    /// Of `cache_creation_input_tokens`, those written with the 1-hour TTL
    /// (Anthropic's `usage.cache_creation.ephemeral_1h_input_tokens`), which
    /// cost 2 × input against 1.25 × for the rest (theseus-ev1). Absent when
    /// 0, so a row or record from before it reads as none.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub cache_creation_1h_input_tokens: u64,
}

#[derive(Default, Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    /// Every timed thing in the turn, nested: turn > loops > provider/tools/advancer.
    #[serde(default)]
    pub trace: Option<Span>,
    #[serde(default)]
    pub execution_id: Option<String>,
    /// Dollars for this turn's provider calls, from the model catalog (None: model not in catalog).
    #[serde(default)]
    pub cost_usd: Option<f64>,
    /// What the session has cost in all, this turn included (theseus-c0bb).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_cost_usd: Option<f64>,
    /// The last loop's request, in tokens as the compiler estimated it, and the
    /// window it compiled against (theseus-wy6v.2): how full the context is.
    /// Absent from a daemon before it, and from a turn that made no request.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub context_tokens: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "number"))]
    pub context_window: Option<u64>,
    #[serde(default)]
    pub tool_calls: u32,
    /// Set when the turn parked waiting for the operator to confirm this action.
    #[serde(default)]
    pub awaiting_confirm: Option<String>,
    /// The provider's `stop_details` (a refusal's category).
    #[serde(default)]
    #[cfg_attr(test, ts(type = "unknown"))]
    pub stop_details: Option<Value>,
    /// The turn ran without new input (a continuation: late results, a confirm answer, a restart).
    #[serde(default)]
    pub continuation: bool,
    /// The notes recall put in front of the model this turn (M6 30b: canary and live; never shadow).
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub recalled: u32,
    /// How routing placed the turn (M5 25e); absent when it was not asked.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub route: Option<route::TurnRoute>,
    /// A refusal moved the turn to its model's fallback (theseus-7gir.18); absent when none did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub fallback: Option<route::TurnFallback>,
    /// What the turn left for later, on a daemon spawned for one run (theseus-mqxk); never a socket daemon's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub later: Option<later::Later>,
    /// The roots, when this turn put its session in a directory outside every one (theseus-aab7): the CLI says so once.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub outside_roots: Option<Vec<String>>,
    /// The harness nudged the turn once after an announced stop (theseus-fh0u); absent when it did not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub nudged: Option<nudge::TurnNudge>,
    /// What the turn's file tools changed and its commands left (theseus-53ujg.1); none in a shared place.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub files: Option<files_left::FilesLeft>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    /// The deferred-tool families this turn's first request loaded past its place's budget, each with its tokens and
    /// who admitted it (`named` or `jev`), under `[tools.defer] admit_ceiling` (theseus-xlri5); none when nothing passed.
    pub raised: Option<route::ToolsRaised>,
    /// The sources of the handles the reply cites, resolved against this
    /// turn's recall (theseus-ejdk); none when it cited none, and none a
    /// shared place may not show.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cites: Vec<cite::Citation>,
    /// Where the turn ran when its place had threads to choose between (theseus-rha2): its tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub thread: Option<thread_route::ThreadTag>,
    /// Where Jev filed the session (theseus-xzxqv): `topic › theseus › ontology (new)`, a reply's footer line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub filed: Option<String>,
    /// The turn in the cockpit (`[cockpit] public_url`, theseus-x9swu): the link a reply's footer carries.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub inspect_url: Option<String>,
    /// The content labels the session carries now (theseus-emqbm), in the
    /// flags' order, each with the address that removes it. A reply's footer
    /// lists them, so the operator can take one off at any time, not only from
    /// the one message that raised it. Empty when the session carries none,
    /// and from a daemon before them: a footer with nothing to list is
    /// byte-identical to the footer before this field.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub labels: Vec<content::LabelLink>,
}

fn is_zero_u32(n: &u32) -> bool {
    *n == 0
}

// ---------------------------------------------------------------- M3: content

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct SessionRef {
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CatalogModel {
    pub model: String,
    /// The catalog row as configured (provider, window, prices, capabilities, source).
    #[cfg_attr(test, ts(type = "Record<string, unknown>"))]
    pub entry: Value,
    /// Profiles that use this model.
    #[serde(default)]
    pub profiles: Vec<String>,
    /// The config's `[catalog."<model>"]` table, as written, when it has one
    /// (theseus-vwar): `entry` holds what it changes, and one that copies the
    /// code's row changes nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "Record<string, unknown>"))]
    pub config: Option<Value>,
    /// The code's own row for the model, when the config's table is over a
    /// model the code has: what the table changes is where the two differ.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional, type = "Record<string, unknown>"))]
    pub code: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct CatalogListResult {
    pub version: String,
    pub models: Vec<CatalogModel>,
}

/// A tool call waiting for the operator (spec §3.9: confirmation is bound to
/// the exact tool, arguments, resource, and an expiry).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfirmRequest {
    pub correlation_id: String,
    pub session_id: String,
    pub execution_id: String,
    /// The turn whose call asks, when a turn made the call: an execution holds many turns of its
    /// session, so a chat that follows a thread's session shows only the questions of its own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_id: Option<String>,
    pub tool: String,
    #[cfg_attr(test, ts(type = "unknown"))]
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub resource: Option<String>,
    /// Why policy asks (e.g. "write under /home/x/projects", "run cargo test").
    pub reason: String,
    pub by: String,
    pub requested_at_ms: u64,
    /// When the question stops holding; 0 for a budget question, which
    /// holds until it is answered or a newer one replaces it.
    pub expires_at_ms: u64,
    /// The floor asks: the call touches Theseus's own binary or state, or the
    /// 1Password CLI or token. It asks at every posture.
    #[serde(default)]
    pub floor: bool,
    /// Set on a budget question (`tool` is `budget.reset`): the figures it
    /// asks about. `reason` is the question in words.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub budget: Option<BudgetAsk>,
    /// The task that asks, when a task does (DD7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub task: Option<TaskRef>,
    /// The layer-1 task change the call asks for, when it is one (39b).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub change: Option<tasks::TaskChange>,
    /// Its answers, worded once (`work::answer_options`): what `work.answer`
    /// takes. Absent from a daemon that does not say.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<work::AnswerOption>,
    /// This connection may answer it (the place rule). Set where a read is
    /// for one connection (`confirm.list`); absent on a push, which every
    /// connection hears alike. A daemon's refusal still says where to answer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub may_answer: Option<bool>,
    /// What the call would do, as the question shows it (theseus-z6au).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub preview: Option<ConfirmPreview>,
    /// A plan question's other yes: approve, and move the conversation to
    /// edits mode (theseus-lyxy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub grant_offer: Option<grants::GrantOffer>,
    /// The question in plain words, worded once for every surface
    /// (theseus-cudar): who asks, what, why, the risk, and what Yes and No do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub plain: Option<plain::PlainWords>,
    /// The asker marked this question as needing the owner's second proof, a
    /// code from the authenticator app (issue 22): a yes without one changes
    /// nothing.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub needs_proof: bool,
    /// And a proof given earlier never covers it: a fresh code every time.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub needs_fresh_proof: bool,
    /// What an answer from the asking connection's place would be asked for,
    /// in words (`confirm.list` sets it; a push, heard alike by every
    /// connection, does not).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub proof: Option<owner_proof::ProofStanding>,
}

/// What a budget question asks about (theseus-0sg): the session reached its
/// spend limit. Approving resets `spent_usd` to $0 and the waiting call
/// proceeds; the session's lifetime cost keeps counting.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct BudgetAsk {
    pub spent_usd: f64,
    pub limit_usd: f64,
    /// What the waiting call reserves.
    pub needed_usd: f64,
    /// The session's lifetime cost, resets included.
    pub lifetime_usd: f64,
}

/// `confirm.list`: the questions of every session parked on one, the most
/// recently active session first.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ConfirmListResult {
    pub confirms: Vec<ConfirmRequest>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionConfirmParams {
    pub correlation_id: String,
    pub approve: bool,
    #[serde(default)]
    pub note: Option<String>,
    /// Subscribe this connection to the session's events before the answer
    /// wakes the execution, so the continuation turn is seen from its start.
    #[serde(default)]
    pub watch: bool,
    /// Who answered, as a label (e.g. `discord:zeroaltitude`). Default: the connection.
    /// A label names; it proves nothing. With `[approval]`, the connection's
    /// surface and `discord` decide whether the answer counts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub author: Option<String>,
    /// Set by the Discord binding: the channel and user the answer came from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub discord: Option<DiscordOrigin>,
    /// Approve, and take the question's offer (a plan's edits mode).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub grant: Option<grants::GrantAnswer>,
    /// The owner's second proof, for a question that needs one (issue 22):
    /// a code from the authenticator app, typed into the surface's own form.
    /// Checked by the daemon; never stored or echoed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub code: Option<owner_proof::ProofCode>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct ActionConfirmResult {
    pub correlation_id: String,
    pub approved: bool,
    pub session_id: String,
    pub execution_id: String,
    /// The answer woke the execution, so a continuation turn follows. False
    /// for a declined budget question: the session keeps waiting, and its
    /// next message asks again.
    #[serde(default = "yes")]
    pub resumes: bool,
}

fn yes() -> bool {
    true
}

/// `error.data` on a provider failure.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
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
    /// What the turn's finished loops spent before it failed.
    #[serde(default)]
    pub usage: Usage,
    #[serde(default)]
    pub cost_usd: Option<f64>,
    #[serde(default)]
    pub tool_calls: u32,
}

/// A notification's params: a field it lacks reads as its default, as the
/// renderers always read them (theseus-0g4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct TurnStarted {
    pub session_id: String,
    pub turn_id: String,
    #[serde(default)]
    pub execution_id: Option<String>,
    #[serde(default)]
    pub continuation: bool,
}

/// A notification's params: a field it lacks reads as its default, as the
/// renderers always read them (theseus-0g4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct TurnFailed {
    pub session_id: String,
    #[serde(default)]
    pub turn_id: Option<String>,
    #[serde(default)]
    pub execution_id: Option<String>,
    #[serde(default)]
    pub continuation: bool,
    /// Error class when known (rate_limited, overloaded, auth, ...).
    /// `stopping`: the call was not sent because the daemon's stop had begun,
    /// and its next start retries it (theseus-36re); no provider's error.
    #[serde(default)]
    pub class: Option<String>,
    pub error: String,
    /// What the execution does next (theseus-ljr): `backoff` (the driver
    /// retries while the class lasts), `retry` (once), or `park` (it waits
    /// on input, and the next message retries). Absent for a task's turn and
    /// a stopped one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub then: Option<String>,
}

/// A notification's params: a field it lacks reads as its default, as the
/// renderers always read them (theseus-0g4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct LoopStarted {
    pub turn_id: String,
    pub loop_index: u32,
    pub model: String,
    pub tools_offered: u32,
}

/// A notification's params: a field it lacks reads as its default, as the
/// renderers always read them (theseus-0g4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct ModelDelta {
    pub turn_id: String,
    pub loop_index: u32,
    pub text: String,
}

/// A notification's params: a field it lacks reads as its default, as the
/// renderers always read them (theseus-0g4).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(default)]
pub struct LoopEnded {
    pub turn_id: String,
    pub loop_index: u32,
    pub provider_stop_reason: Option<String>,
    pub tool_calls: u32,
    pub advancer: String,
    pub decision: String,
}

/// Which architectural part of the session/turn/loop structure a narrative
/// line comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
#[serde(rename_all = "lowercase")]
pub enum NarrativePart {
    Session,
    Turn,
    Loop,
    Context,
    Model,
    Tool,
    Approval,
    Job,
    /// The daemon's config: the vault's read of the copy a start served
    /// from, what it could not settle, or a restart onto a changed note
    /// (theseus-2fo).
    Config,
}

impl NarrativePart {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Session => "session",
            Self::Turn => "turn",
            Self::Loop => "loop",
            Self::Context => "context",
            Self::Model => "model",
            Self::Tool => "tool",
            Self::Approval => "approval",
            Self::Job => "job",
            Self::Config => "config",
        }
    }
}

/// One sentence of the narrative, filled into a fixed template from the
/// structure itself (never written by a model). Kept only in memory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NarrativeLine {
    /// Counts from 1 since the daemon started; a client merges the tail and
    /// the live lines on it.
    pub seq: u64,
    pub at_unix_ms: u64,
    pub part: NarrativePart,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(test, ts(optional))]
    pub turn_id: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(test, derive(ts_rs::TS))]
pub struct NarrativeWatchResult {
    /// The most recent lines, oldest first; at most `capacity`.
    pub lines: Vec<NarrativeLine>,
    pub capacity: u32,
}
