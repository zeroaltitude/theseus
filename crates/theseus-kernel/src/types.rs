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
/// The tool name of a budget question: the planned action that asks the
/// operator whether an execution's spend may go back to $0 (theseus-0sg).
pub use theseus_protocol::BUDGET_TOOL;
/// The tool name of a provider call's action, which never asks the operator.
pub use theseus_protocol::PROVIDER_TOOL;

use crate::gate::Proposal;

pub type ExecutionId = String;
pub type SessionId = String;
pub type CorrelationId = String;

/// Schema versions stamped into every record's payload so forward-only
/// migrations can read old rows. 2 (theseus-0sg): budgets and reservations
/// are micro-dollars. Executions stored at 1 carry a unit budget, which
/// `Execution::from_stored` reads; actions stored at 1 carry
/// `reserved_units`, which is never read as dollars.
pub const SCHEMA: u16 = 2;

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
    /// The budget (theseus-0sg): a call did not fit under the spend limit,
    /// and the operator is asked, by the `budget.reset` action named here,
    /// whether the spend may go back to $0. An approval wakes it
    /// (`Kernel::reset_budget`). A decline leaves it waiting; new input wakes
    /// it, and its next call asks again.
    Budget { correlation_id: CorrelationId },
}

/// A wake a conversation set for itself with `wake.at` (DD8, theseus-cff): at
/// `due_at_ms` its session gets a turn whose input is `note`. A one-shot
/// wake is removed by the turn that takes it; a repeating one (37a) is put
/// back, in the same frame, at its next occurrence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingWake {
    /// `wak_…`, from the id of the call that set it (`wake_id`).
    pub id: String,
    pub due_at_ms: u64,
    pub note: String,
    pub set_at_ms: u64,
    /// The `wake.at` call that set it.
    pub by: CorrelationId,
    /// Where its session posted when it was set (`discord:dm:<user>`): its
    /// turn's reply goes there if the session posts nowhere by then (a place
    /// that moved on to a new session).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    /// What makes it repeat (37a, theseus-d4pt); none for a one-shot wake.
    /// Execution schema 3; a schema-2 wake reads as one-shot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repeat: Option<crate::repeat::Repeat>,
    /// Which occurrence of its series is due, from 1 (37a); 0 for a one-shot
    /// wake.
    #[serde(default, skip_serializing_if = "no_occurrence")]
    pub occurrence: u32,
}

fn no_occurrence(n: &u32) -> bool {
    *n == 0
}

/// A `/stop` that landed while a turn held its execution (W1, theseus-lji):
/// that turn plans nothing more, and its end parks the execution on input
/// instead of where the turn would have waited.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Stopped {
    /// Who stopped it: the surface or the person (`Conn::actor`).
    pub by: String,
    pub at_ms: u64,
    /// The turn it stopped.
    pub turn: u64,
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

/// A spend limit in US dollars, with reservations (§3.13, §3.15, Part II
/// M2; dollars since theseus-0sg). Every amount is micro-dollars. A call
/// reserves what it may cost before it runs and settles at what it did cost.
/// A reservation that does not fit is refused, and the execution waits on
/// the operator (`Wake::Budget`) instead of ending; an approved reset is the
/// only way `spent_micros` goes down.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Budget {
    pub limit_micros: Micros,
    /// Settled costs since the execution opened or was last reset.
    pub spent_micros: Micros,
    /// Reserved for in-flight work; released or converted to spent on settle.
    pub reserved_micros: Micros,
    /// Reservations whose real cost is unknown (interrupted provider calls)
    /// stay held here until reconciled; they never silently release, and a
    /// reset leaves them held.
    pub held_unknown_micros: Micros,
    /// Open reservations by id.
    #[serde(default)]
    pub reservations: BTreeMap<String, Micros>,
    /// The budget question the operator has not answered: a planned
    /// `budget.reset` action of this execution. One is open at a time.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub question: Option<CorrelationId>,
    /// What the call waiting on that question would reserve.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub question_needs_micros: Micros,
    /// Approved resets, each a `budget.reset` ledger row.
    #[serde(default)]
    pub resets: u32,
    /// The unit budget a record stored before theseus-0sg carried, as it was.
    /// It is history only: nothing decides from it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub units_before: Option<UnitBudget>,
    /// The limit is the one the opener named, and the config's does not
    /// replace it. Unset, the limit is the config's, and it follows
    /// `spend_limit_micros` when the config changes (theseus-3pj). No caller
    /// in the product names one yet.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub pinned: bool,
}

impl Budget {
    pub fn new(limit_micros: Micros) -> Self {
        Self {
            limit_micros,
            ..Default::default()
        }
    }
    /// What may still be reserved.
    pub fn available(&self) -> Micros {
        self.limit_micros
            .saturating_sub(self.spent_micros)
            .saturating_sub(self.reserved_micros)
            .saturating_sub(self.held_unknown_micros)
    }
    /// What an approved reset leaves to reserve with: the limit, less what
    /// is reserved for calls in flight and what is held for calls whose cost
    /// is unknown, which a reset leaves as they are (theseus-6g6). A call that
    /// needs more than this cannot fit after any reset; theseus-kks's call
    /// over the whole limit is the case where nothing is held.
    pub fn available_after_reset(&self) -> Micros {
        self.limit_micros
            .saturating_sub(self.reserved_micros)
            .saturating_sub(self.held_unknown_micros)
    }
    /// A unit budget read in dollars: the given limit, nothing spent,
    /// reserved, or held (a unit has no price), and the unit figures kept.
    /// Startup then takes the spend from the session's recorded cost.
    pub fn from_units(units: UnitBudget, limit_micros: Micros) -> Self {
        Self {
            limit_micros,
            units_before: Some(units),
            ..Default::default()
        }
    }
}

fn is_zero(m: &Micros) -> bool {
    *m == 0
}

/// A budget as stored before theseus-0sg: abstract units, the tokens of every
/// call at full weight, with a slice kept back as a control reserve.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct UnitBudget {
    pub limit: u64,
    pub spent: u64,
    pub reserved: u64,
    pub held_unknown: u64,
    #[serde(default)]
    pub control_reserve: u64,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub reservations: BTreeMap<String, u64>,
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
    /// The execution that opened this one as a task (DD7, theseus-qn2): its
    /// spend counts against the parent's, and its end is reported there. A
    /// task has no tasks of its own (depth one).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<ExecutionId>,
    /// Channel or parent task this execution reports into (§3.2a).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reports_to: Option<String>,
    /// Tasks of this execution that ended and whose reports its next turn has
    /// not read yet, oldest first (DD7). The frame that ends a task adds it;
    /// `Kernel::take_reports` clears them, in the frame that writes them into
    /// the session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reports: Vec<ExecutionId>,
    /// Wakes this execution set for itself that have not run yet (DD8,
    /// theseus-cff), soonest first. They sit beside `wake`, which says what
    /// the last turn parked on: a conversation waits on its next input and on
    /// its due times at once (`wakes.rs`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wakes: Vec<PendingWake>,
    /// A task whose report starts its parent's next turn (W1, theseus-lji:
    /// `task.create { wake_parent: true }`), unless it was cancelled.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub wake_parent: bool,
    /// The tasks among `reports` whose reports start this execution's next
    /// turn (W1). While any is here and the execution is free, the driver
    /// takes a turn, which reads every report (`take_reports` clears both).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub report_wakes: Vec<ExecutionId>,
    /// A stop that landed while a turn held this execution (W1): that turn
    /// plans nothing more, and its end, or startup after a crash, parks the
    /// execution on input and clears this.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<Stopped>,
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

impl Execution {
    /// Read a stored execution. Schema 2 decodes as it is. Schema 1, a unit
    /// budget from before theseus-0sg, decodes with a dollar budget of
    /// `limit_micros` and its unit figures in `budget.units_before`; its
    /// `schema` stays 1 until startup rewrites it (see `Kernel::startup`).
    pub fn from_stored(payload: &[u8], limit_micros: Micros) -> anyhow::Result<Self> {
        let first = match serde_json::from_slice::<Execution>(payload) {
            Ok(e) => return Ok(e),
            Err(e) => e,
        };
        let mut v: serde_json::Value = serde_json::from_slice(payload)?;
        let stored = v.get("schema").and_then(serde_json::Value::as_u64);
        if stored.is_some_and(|s| s >= SCHEMA as u64) {
            return Err(first.into());
        }
        let units: UnitBudget = serde_json::from_value(v["budget"].take()).map_err(|e| {
            anyhow::anyhow!("execution budget is neither dollars ({first}) nor units ({e})")
        })?;
        v["budget"] = serde_json::to_value(Budget::from_units(units, limit_micros))?;
        Ok(serde_json::from_value(v)?)
    }
}

/// Per-operation declaration of what a repeat would do (§3.16). Every tool
/// declares `SafeToRepeat` or `NonRepeatable`; an outbox post that creates a
/// message is `IdempotentWithKey` (theseus-q4v). `recoverable_by_external_id`
/// waits for a tool that needs it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "class")]
pub enum RetryClass {
    SafeToRepeat,
    NonRepeatable,
    /// A repeat under the same downstream key returns the first result
    /// instead of acting twice, for as long as the downstream honors the
    /// key. `key` names it: `discord.nonce`.
    IdempotentWithKey {
        key: String,
    },
}

impl RetryClass {
    pub fn as_str(&self) -> &'static str {
        match self {
            RetryClass::SafeToRepeat => "safe_to_repeat",
            RetryClass::NonRepeatable => "non_repeatable",
            RetryClass::IdempotentWithKey { .. } => "idempotent_with_key",
        }
    }
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

/// How a cancel knows its call stopped (M4 18a; design §2.3): what a
/// `termination_verified` rests on, by backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerifiedBy {
    /// An L1 job's pid namespace: its init was killed and reaped, and the
    /// kernel kills every process of a namespace before its init's exit ends.
    Pidns,
    /// The job's cgroup: `cgroup.kill`, then `cgroup.events` at `populated 0`.
    Cgroup,
    /// An L0 job's process tree: its wrapper stopped every descendant and
    /// found none left (`scope: descendants`).
    Tree,
    /// A wrapper from before 18a, which dies at the first SIGTERM: its
    /// process group has no live member. A descendant that left the group
    /// (`setsid`) is out of its reach.
    Group,
    /// An async tool's task: aborted, and its handle finished.
    Task,
    /// Nothing verified it: the call cannot be stopped, or its end was not
    /// seen.
    None,
}

impl VerifiedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            VerifiedBy::Pidns => "pidns",
            VerifiedBy::Cgroup => "cgroup",
            VerifiedBy::Tree => "tree",
            VerifiedBy::Group => "group",
            VerifiedBy::Task => "task",
            VerifiedBy::None => "none",
        }
    }
}

/// A stop's verdict (M4 18a): how it knows the call stopped, and what it
/// counted. A job's wrapper writes it to the spool when a cancel stops the
/// job (`Spool::write_stop`), and into its completion's `detail.stop` when its
/// deadline does; the driver makes one for a wrapper from before 18a and for
/// an aborted task.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Verdict {
    /// The means: for a verdict that is not verified, what was tried.
    pub verified_by: VerifiedBy,
    /// The processes the stop ended (a job's).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub killed: Option<u32>,
    /// The processes still alive after the stop's kill and its wait: 0 when
    /// it is verified; absent when nobody could count them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub survivors: Option<u32>,
    /// What the stop could see: `descendants` at L0, the wrapper's tree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// From the stop's signal to its verdict.
    #[serde(default)]
    pub ms: u64,
    /// Why it is not verified, when it is not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why: Option<String>,
}

impl Verdict {
    /// Verified: nothing left, and nothing said otherwise.
    pub fn verified(&self) -> bool {
        self.why.is_none()
            && self.survivors.unwrap_or(0) == 0
            && self.verified_by != VerifiedBy::None
    }

    /// A verified verdict: by `by`, having ended `killed` processes.
    pub fn verified_as(by: VerifiedBy, killed: Option<u32>) -> Self {
        Self {
            verified_by: by,
            killed,
            survivors: Some(0),
            scope: None,
            ms: 0,
            why: None,
        }
    }

    /// A verdict that is not verified, and why.
    pub fn uncertain(tried: VerifiedBy, why: impl Into<String>) -> Self {
        Self {
            verified_by: tried,
            killed: None,
            survivors: None,
            scope: None,
            ms: 0,
            why: Some(why.into()),
        }
    }

    /// In words, as every surface shows it (`theseus_protocol::cancel::words`):
    /// "verified: pid namespace, 4 processes", or "not verified: …".
    pub fn words(&self) -> String {
        theseus_protocol::cancel::words(
            self.verified(),
            self.verified_by.as_str(),
            self.killed,
            self.survivors,
            self.why.as_deref(),
        )
    }
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
    /// (§3.16 references, not payloads) and are not duplicated here, except in
    /// `proposal` below.
    pub args_digest: String,
    /// What an action that waits for the operator asks about: a tool call the
    /// policy stopped for approval, or a budget question. The confirm binds it
    /// and `authorize` re-checks it (theseus-0g4). Every other action is
    /// authorized in the frame after its plan and keeps none. Actions stored
    /// before theseus-0g4 have none either; theirs is on the tool-call node.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposal: Option<Proposal>,
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
    /// The cancel's verdict (M4 18a; ACTION schema 3), set with its last
    /// step: how it knows the backend stopped (`verified_by`), what its stop
    /// ended and left (`killed`, `survivors`), and why it is not verified
    /// when it is not. `verified_by` is `none` for a call that cannot be
    /// stopped. Absent when the cancel reached no running backend (a job
    /// stopped before its launch, a call before its dispatch), or before 18a.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verdict: Option<Verdict>,
    /// Budget reservation held for this action, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reservation_id: Option<String>,
    /// What the reservation holds. Actions stored before theseus-0sg carry
    /// `reserved_units` instead, which is left unread: a unit has no price.
    #[serde(default)]
    pub reserved_micros: Micros,
    /// How an `OutcomeUnknown` was later resolved, if it was.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    /// Number of completions seen (>1 means duplicates were ignored).
    pub completions_seen: u32,
    /// What the completion said, kept on the action: an outbox post's
    /// messages (theseus-q4v). Other actions keep none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<serde_json::Value>,
    /// The action this one belongs to (ACTION schema 4): 18d's credential
    /// request named its job here. Nothing writes it since those requests
    /// went (theseus-w5op), so a request's stored row reads whole, with no
    /// schema bump; absent for every other action.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<CorrelationId>,
}

impl Action {
    /// Waits for the operator's answer (§3.9): planned, with no confirm bound
    /// yet, and not a provider call. This is the one test of a pending confirm
    /// (theseus-0g4), a tool call or a budget question, that every reader
    /// uses through `Kernel::pending_confirms`.
    pub fn awaits_confirm(&self) -> bool {
        self.state == ActionState::Planned && self.confirm.is_none() && self.tool != PROVIDER_TOOL
    }

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

    /// Who stopped this call, when a `/stop` ended it (W1): running, it was
    /// told to stop, or planned or waiting, it was declined, and either way
    /// `Kernel::stop_execution` wrote its resolution as `stopped by <who>`.
    /// The surfaces say so as a stop, never as a failure (theseus-4uw).
    pub fn stopped_by(&self) -> Option<&str> {
        self.resolution.as_deref()?.strip_prefix("stopped by ")
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
    /// The real cost, when the producer knows it: it settles the reservation.
    /// (Completions stored before theseus-0sg said `usage_units`.)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_micros: Option<Micros>,
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
