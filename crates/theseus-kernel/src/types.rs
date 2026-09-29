//! The kernel's durable objects (spec §3.15, §3.16, §3.2a). Every one of
//! these is a WAL record: `Execution` keyed by id, `Action` keyed by
//! correlation id, `Completion` keyed by correlation id (only quarantined or
//! duplicate ones are stored on their own; a matched completion is folded
//! into the action it settles). A session's own record is the core's; the
//! kernel knows a session only by the id its execution carries. State lives
//! here, not in process memory; the process holds locks and caches only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
/// One enum on the wire and in the store: stored executions carry
/// `"conversation"` or `"task"` either way.
pub use theseus_protocol::SessionKind;

pub type ExecutionId = String;
pub type SessionId = String;
pub type CorrelationId = String;

/// Schema versions stamped into every record's payload so forward-only
/// migrations can read old rows.
pub const SCHEMA: u16 = 1;

/// US dollars in millionths ($1 is 1,000,000). Budgets count in these, so
/// every sum and every comparison against a limit is exact (theseus-0sg).
pub type Micros = u64;
pub const MICROS_PER_USD: u64 = 1_000_000;

/// Dollars to micro-dollars, to the nearest one; negative and NaN are zero.
pub fn usd_to_micros(usd: f64) -> Micros {
    if usd.is_finite() && usd > 0.0 {
        (usd * MICROS_PER_USD as f64).round() as u64
    } else {
        0
    }
}

pub fn micros_to_usd(m: Micros) -> f64 {
    m as f64 / MICROS_PER_USD as f64
}

/// A dollar amount as a sentence says it: `$100`, `$0.45`, `$12.30`,
/// `$0.002`, `$0.004521`. Exact to the micro-dollar, with no trailing zeros
/// past the cents.
pub fn usd(m: Micros) -> String {
    let (whole, frac) = (m / MICROS_PER_USD, m % MICROS_PER_USD);
    if frac == 0 {
        return format!("${whole}");
    }
    let mut digits = format!("{frac:06}");
    while digits.len() > 2 && digits.ends_with('0') {
        digits.pop();
    }
    format!("${whole}.{digits}")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecState {
    /// Has runnable work; wants a turn. The admission scheduler picks from here.
    Queued,
    /// Holds a turn (the turn lock) right now.
    Running,
    /// No runnable work; a wake condition exists (§3.15).
    Waiting,
    /// Cannot proceed without a human or an external change; reason recorded.
    Blocked,
    Cancelled,
    Failed,
    BudgetExhausted,
    Complete,
}

impl ExecState {
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            ExecState::Cancelled
                | ExecState::Failed
                | ExecState::BudgetExhausted
                | ExecState::Complete
        )
    }
    pub fn as_str(self) -> &'static str {
        match self {
            ExecState::Queued => "queued",
            ExecState::Running => "running",
            ExecState::Waiting => "waiting",
            ExecState::Blocked => "blocked",
            ExecState::Cancelled => "cancelled",
            ExecState::Failed => "failed",
            ExecState::BudgetExhausted => "budget_exhausted",
            ExecState::Complete => "complete",
        }
    }
}

/// Why an execution is `Waiting`, and what wakes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "on")]
pub enum Wake {
    /// A due time (a scheduled wake, a retry backoff, a task sleeping a week).
    DueAt { at_ms: u64 },
    /// One or more dispatched actions; any completion wakes it.
    Actions { correlation_ids: Vec<CorrelationId> },
    /// Another execution reaching a terminal state (a child task).
    Execution { execution_id: ExecutionId },
    /// A confirmation answer (§3.9); the confirm id binds the pending action.
    Confirm { confirm_id: String },
    /// Human input on the session (a conversation between exchanges).
    Input,
}

/// The authority an execution acts under (§3.9), inherited never widened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Authority {
    pub principal: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delegated_by: Option<String>,
    /// Ceilings by name (e.g. `shell: l1`, `tools: fs,text`), intersected on fork.
    #[serde(default)]
    pub ceilings: BTreeMap<String, String>,
}

/// Budget as a hard limit with reservations (§3.15, Part II M2). Units are
/// abstract "units" (tokens today; dollars once the catalog lands, M2+).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Budget {
    pub limit: u64,
    pub spent: u64,
    /// Reserved for in-flight work; released or converted to spent on settle.
    pub reserved: u64,
    /// Reservations whose real usage is unknown (interrupted provider calls)
    /// stay held here until reconciled; they never silently release.
    pub held_unknown: u64,
    /// Kept back for control and cleanup (cancel, final report) so a runaway
    /// never leaves the execution unable to end cleanly.
    pub control_reserve: u64,
    /// Open reservations by id.
    #[serde(default)]
    pub reservations: BTreeMap<String, u64>,
}

impl Budget {
    pub fn new(limit: u64, control_reserve: u64) -> Self {
        Self {
            limit,
            control_reserve: control_reserve.min(limit),
            ..Default::default()
        }
    }
    /// What may still be reserved for ordinary work.
    pub fn available(&self) -> u64 {
        self.limit
            .saturating_sub(self.spent)
            .saturating_sub(self.reserved)
            .saturating_sub(self.held_unknown)
            .saturating_sub(self.control_reserve)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Execution {
    pub id: ExecutionId,
    pub schema: u16,
    pub session_id: SessionId,
    pub kind: SessionKind,
    pub state: ExecState,
    pub authority: Authority,
    pub budget: Budget,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake: Option<Wake>,
    /// Dispatched actions not yet settled.
    #[serde(default)]
    pub outstanding: Vec<CorrelationId>,
    /// Settled actions whose results the next turn must consume, in order.
    #[serde(default)]
    pub queued_results: Vec<CorrelationId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ExecutionId>,
    /// Channel or parent task this execution reports into (§3.2a).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reports_to: Option<String>,
    /// Turns taken (lock acquisitions).
    pub turns: u64,
    /// Recovery counter: how many times a crash interrupted a running turn.
    pub interrupted: u32,
    /// Set when the execution became runnable without human input (requeued
    /// after a crash, a confirm answered): the harness driver takes the turn.
    /// Cleared on admission.
    #[serde(default)]
    pub resume_pending: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<CancelState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_reason: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// Per-operation declaration of what a repeat would do (§3.16). Every tool
/// declares one of these two; the others §3.16 names wait for a tool that
/// needs them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "class")]
pub enum RetryClass {
    SafeToRepeat,
    NonRepeatable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Planned,
    Authorized,
    Dispatched,
    Succeeded,
    Failed,
    OutcomeUnknown,
    Cancelled,
}

impl ActionState {
    pub fn is_settled(self) -> bool {
        matches!(
            self,
            ActionState::Succeeded | ActionState::Failed | ActionState::Cancelled
        )
    }
    pub fn as_str(self) -> &'static str {
        match self {
            ActionState::Planned => "planned",
            ActionState::Authorized => "authorized",
            ActionState::Dispatched => "dispatched",
            ActionState::Succeeded => "succeeded",
            ActionState::Failed => "failed",
            ActionState::OutcomeUnknown => "outcome_unknown",
            ActionState::Cancelled => "cancelled",
        }
    }
}

/// Cancellation is a lifecycle, not a flag (§3.16).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelState {
    Requested,
    Acknowledged,
    TerminationVerified,
    Unsupported,
    OutcomeUncertain,
}

/// A confirmation bound to the final action (§3.9, §3.17): the digest covers
/// tool, arguments, resource, and policy context; any later change to those
/// invalidates it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfirmBinding {
    pub confirm_id: String,
    pub by: String,
    pub bound_digest: String,
    pub expires_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Action {
    pub correlation_id: CorrelationId,
    pub schema: u16,
    pub execution_id: ExecutionId,
    pub session_id: SessionId,
    pub tool: String,
    /// sha256 of the canonical arguments; the arguments themselves are a node
    /// (§3.16 references, not payloads) and are not duplicated here.
    pub args_digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resource: Option<String>,
    pub retry_class: RetryClass,
    pub state: ActionState,
    /// The deadline the wrapper enforces locally and the reconciler polls past.
    pub deadline_at_ms: u64,
    pub planned_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub authorized_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dispatched_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub settled_at_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_op_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confirm: Option<ConfirmBinding>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<CancelState>,
    /// Budget reservation held for this action, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reservation_id: Option<String>,
    #[serde(default)]
    pub reserved_units: u64,
    /// How an `OutcomeUnknown` was later resolved, if it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    /// Number of completions seen (>1 means duplicates were ignored).
    pub completions_seen: u32,
}

impl Action {
    /// The note from a decline, when `Kernel::decline_action` settled this
    /// action: its resolution reads `declined by <who>: <note>`, or `denied by
    /// <who>: <note>` in rows written before theseus-8az.
    pub fn declined_note(&self) -> Option<&str> {
        let r = self.resolution.as_deref()?;
        let rest = r
            .strip_prefix("declined by ")
            .or_else(|| r.strip_prefix("denied by "))?;
        Some(rest.split_once(": ").map_or(rest, |(_, note)| note))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    Failed,
    Unknown,
}

/// One envelope across all sources (§3.16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Completion {
    pub correlation_id: CorrelationId,
    pub outcome: Outcome,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub external_op_id: Option<String>,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    /// Who produced it: `wrapper:<pid>`, `inproc`, `reconciler`, `sim`.
    pub producer: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
    /// Real usage if the producer knows it (converts the reservation).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage_units: Option<u64>,
    /// Producer-specific facts: exit code, signal, bytes, truncation, duration.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
}

/// A ledger row. Same JSON shape as `theseus-core`'s so `theseus ledger`
/// reads both; `execution_id` and `correlation_id` ride in `data`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LedgerRow {
    pub at_unix_ms: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_id: Option<String>,
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub data: serde_json::Value,
}

pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::now_v7().simple())
}
