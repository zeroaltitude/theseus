//! The kernel's durable objects (spec §3.15, §3.16, §3.2a). Every one of
//! these is a WAL record: `Execution` keyed by id, `Action` keyed by
//! correlation id, `Session` keyed by id, `Completion` keyed by correlation
//! id (only quarantined or duplicate ones are stored on their own; a matched
//! completion is folded into the action it settles). State lives here, not
//! in process memory; the process holds locks and caches only.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub type ExecutionId = String;
pub type SessionId = String;
pub type CorrelationId = String;

/// Schema versions stamped into every record's payload so forward-only
/// migrations can read old rows.
pub const SCHEMA: u16 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionKind {
    Conversation,
    Task,
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

impl Authority {
    /// Derive a child authority: same principal, delegation recorded, ceilings
    /// intersected (the child may only carry what the parent had, and only
    /// tighten). A key the parent lacks is dropped; a value the parent has is kept
    /// unless the child asks for the same key, in which case the child's value is
    /// used only if it equals the parent's (there is no partial order on strings
    /// here; anything else is a widening and refused).
    pub fn derive(&self, requested: &BTreeMap<String, String>) -> Result<Authority, String> {
        let mut ceilings = BTreeMap::new();
        for (k, v) in &self.ceilings {
            match requested.get(k) {
                None => {
                    ceilings.insert(k.clone(), v.clone());
                }
                Some(rv) if rv == v => {
                    ceilings.insert(k.clone(), v.clone());
                }
                Some(rv) => {
                    return Err(format!(
                        "ceiling {k}: child requested {rv:?}, parent holds {v:?}"
                    ));
                }
            }
        }
        for k in requested.keys() {
            if !self.ceilings.contains_key(k) {
                return Err(format!(
                    "ceiling {k}: parent holds no such ceiling; refusing to widen"
                ));
            }
        }
        Ok(Authority {
            principal: self.principal.clone(),
            delegated_by: Some(self.principal.clone()),
            ceilings,
        })
    }
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cancel: Option<CancelState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ended_reason: Option<String>,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
}

/// A session is a compiler scope (§3.2a); its record is small and durable by
/// reference (§4.4b). The per-session position table is the store's scope index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub schema: u16,
    pub kind: SessionKind,
    /// Channel id or task id roots, in priority order.
    pub roots: Vec<String>,
    pub execution_id: ExecutionId,
    /// Append tail: records in this session's scope after `tail_from` are the
    /// uncompiled tail (§4.4b). M2 keeps the field; the compiler arrives in M3.
    pub tail_from: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub created_at_ms: u64,
}

/// Per-operation declaration of what a repeat would do (§3.16).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "class")]
pub enum RetryClass {
    SafeToRepeat,
    IdempotentWithKey { key: String },
    RecoverableByExternalId,
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
