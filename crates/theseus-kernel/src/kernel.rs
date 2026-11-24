//! The kernel: every state transition of executions and actions, written as
//! WAL frames through the `Store` contract. Synchronous and deterministic:
//! time comes from an injected `Clock`, randomness from nowhere, and the only
//! process state is the set of turn locks currently held and the executions
//! being written right now. Drop the `Kernel` and reopen the store and nothing
//! is lost but the locks, which is the point.
//!
//! Every mutating method writes exactly one frame: the records that change
//! plus the ledger rows that describe the change. A crash between two frames
//! leaves the store in a state some earlier method call produced, never in a
//! state no method produces. A method that reads an execution or one of its
//! actions and writes it back holds the execution's lock from the read until
//! its frame is indexed, so no two of them lose each other's update
//! (theseus-id9, `locks.rs`). Several transitions share one frame through a
//! transaction (`Kernel::frame`, `tx.rs`, theseus-0owd): the combined
//! transitions (`admit_input`, `plan_and_dispatch`, the `_with` family) are
//! compositions of the ordinary ones.

use std::collections::{BTreeMap, HashSet};
use std::sync::atomic::{AtomicU32, AtomicU64};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_protocol::LedgerKind;
use theseus_store::{kinds, NewRecord, Record, Store};

use crate::clock::Clock;
use crate::day_ceiling::Effect;
use crate::earlier::book_reservation_in;
use crate::gate::{digest_proposal, Proposal};
use crate::locks::{ExecLock, ExecLocks};
use crate::reopen::reopens;
use crate::terms;
use crate::tx::{Staged, Tx};
use crate::types::*;

mod authorize;
mod cancel_reconcile;
mod completion;
mod reads;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelConfig {
    /// How many executions may hold a turn at once (§3.2a admission scheduler).
    pub admission_ceiling: u32,
    /// Deadline for an action whose tool declares none.
    pub default_deadline_ms: u64,
    /// The spend limit, in micro-dollars, of a new execution whose caller
    /// names none, and of an execution stored with a unit budget. Every open
    /// execution whose limit is the config's follows it at startup
    /// (theseus-3pj; startup's step 2).
    pub spend_limit_micros: Micros,
    /// A provider call past the limit goes on (`Kernel::overdraws`, theseus-usei).
    #[serde(default)]
    pub spend_limit_notify: bool,
    /// How long a confirmation stays valid.
    pub confirm_ttl_ms: u64,
    /// Reconciler cadence (the heartbeat, §3.3).
    pub heartbeat_ms: u64,
    /// Test hook: startup returns an error after this step, as if the process
    /// died there. Never set outside the simulator.
    #[serde(skip)]
    pub fault_after_startup_step: Option<u8>,
    /// The shortest span a repeating wake may take (37a, `[kernel]
    /// min_repeat_minutes`).
    #[serde(default = "default_min_repeat_ms")]
    pub min_repeat_ms: u64,
    /// The daemon's zone, in which a repeating wake's calendar spans run
    /// (37a): the system's, or a test's own.
    #[serde(skip, default = "jiff::tz::TimeZone::system")]
    pub zone: jiff::tz::TimeZone,
    /// The daemon's day ceiling over every model call (theseus-kp20).
    #[serde(default = "default_day_ceiling")]
    pub daily_ceiling_micros: Micros,
    /// The multiple of a limit that notifies at which its calls stop anyway
    /// (theseus-jse4, `overdraw.rs`); 0 never.
    #[serde(default = "crate::overdraw::default_hard_multiple")]
    pub spend_hard_multiple: u64,
    /// The day ceiling only warns (`[kernel] daily_spend_ceiling_mode =
    /// "notify"`, theseus-9e02); the kernel's own default is the stop.
    #[serde(default)]
    pub daily_notify: bool,
    /// Under notify, where the day stops (0 never).
    #[serde(default = "default_runaway")]
    pub daily_runaway_micros: Micros,
    /// The warning's step past the ceiling.
    #[serde(default = "default_step")]
    pub daily_step_micros: Micros,
    /// A session's burn-rate line over ten minutes (0 never).
    #[serde(default)]
    pub burn_line_micros: Micros,
    /// How far ahead and how many wakes (theseus-8ux6x).
    #[serde(default)]
    pub wakes: crate::wake_kinds::WakeLimits,
    /// How many levels of tasks may sit below a conversation
    /// (theseus-tecj5, `task_depth.rs`).
    #[serde(default = "default_task_depth")]
    pub task_depth_limit: u32,
}

fn default_task_depth() -> u32 {
    crate::task_depth::DEFAULT_DEPTH_LIMIT
}

fn default_runaway() -> Micros {
    crate::day_ceiling::DEFAULT_RUNAWAY_MICROS
}

fn default_step() -> Micros {
    crate::day_ceiling::DEFAULT_STEP_MICROS
}

fn default_day_ceiling() -> Micros {
    crate::day_ceiling::DEFAULT_CEILING_MICROS
}

fn default_min_repeat_ms() -> u64 {
    5 * 60_000
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            admission_ceiling: 8,
            default_deadline_ms: 10 * 60 * 1000,
            spend_limit_micros: 100 * MICROS_PER_USD,
            spend_limit_notify: false,
            confirm_ttl_ms: 15 * 60 * 1000,
            heartbeat_ms: 60_000,
            fault_after_startup_step: None,
            min_repeat_ms: default_min_repeat_ms(),
            zone: jiff::tz::TimeZone::system(),
            daily_ceiling_micros: default_day_ceiling(),
            spend_hard_multiple: crate::overdraw::default_hard_multiple(),
            daily_notify: false,
            daily_runaway_micros: default_runaway(),
            daily_step_micros: default_step(),
            burn_line_micros: 0,
            wakes: Default::default(),
            task_depth_limit: default_task_depth(),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error("admission ceiling reached ({ceiling}); execution stays queued")]
    AdmissionFull { ceiling: u32 },
    #[error("execution {id} is {state}, not runnable")]
    NotRunnable {
        id: ExecutionId,
        state: &'static str,
    },
    #[error("execution {id} already holds a turn in this process")]
    TurnHeld { id: ExecutionId },
    #[error("execution {id} is not running (state {state}); actions require a held turn")]
    NoTurn {
        id: ExecutionId,
        state: &'static str,
    },
    /// A reservation did not fit under the spend limit. Nothing was written:
    /// the caller asks the operator (`Kernel::ask_budget`).
    #[error(
        "over budget: the call needs {}, and {} of the {} limit is left ({} spent)",
        usd(*.needed),
        usd(*.available),
        usd(*.limit),
        usd(*.spent)
    )]
    OverBudget {
        needed: Micros,
        available: Micros,
        spent: Micros,
        limit: Micros,
    },
    #[error("action {correlation_id} is {state}; expected {expected}")]
    ActionState {
        correlation_id: CorrelationId,
        state: &'static str,
        expected: &'static str,
    },
    #[error("confirmation invalidated: the action changed after it was confirmed ({reason})")]
    ConfirmInvalidated { reason: String },
    #[error("confirmation required from {by} and none bound")]
    ConfirmRequired { by: String },
    #[error("unknown execution {0}")]
    UnknownExecution(ExecutionId),
    #[error("unknown action {0}")]
    UnknownAction(CorrelationId),
    #[error("kernel is not accepting events yet (startup step {step})")]
    NotAccepting { step: u8 },
    /// A task would sit deeper than the limit (DD7; theseus-tecj5).
    #[error(
        "execution {id} sits at task depth {depth}, and tasks may go {limit} deep: it cannot start another"
    )]
    TaskDepth {
        id: ExecutionId,
        depth: usize,
        limit: u32,
    },
    /// Nothing is left under the parent's limit to carve a task's budget from.
    #[error(
        "nothing to carve a task's budget from: {} of the {} limit is left ({} spent)",
        usd(*.available),
        usd(*.limit),
        usd(*.spent)
    )]
    NothingToCarve {
        available: Micros,
        spent: Micros,
        limit: Micros,
    },
    /// An execution holds as many pending wakes as it may (DD8).
    #[error("this session already has {max} pending wakes, the most it may hold")]
    TooManyWakes { max: usize },
    /// A `/stop` landed during the turn (W1): it plans nothing more.
    #[error("execution {id} was stopped by {by}: this turn plans nothing more")]
    Stopped { id: ExecutionId, by: String },
    /// A task is stopped by its cancel, not by a stop (W1): nothing would
    /// ever send a stopped task its next input.
    #[error("execution {id} is a task: task.cancel stops it")]
    StopTask { id: ExecutionId },
    /// The daemon's day ceiling refused a provider call (theseus-kp20).
    #[error("{0}")]
    DayCeiling(Box<crate::day_ceiling::Reached>),
    /// A limit that notifies refused a provider call at its hard multiple
    /// (theseus-jse4): nothing was written.
    #[error("{0}")]
    HardStop(Box<crate::overdraw::HardStop>),
    /// A session's burn-rate line stopped a provider call (theseus-9e02):
    /// nothing was written; the core asks the owner to resume.
    #[error("{0}")]
    BurnRate(Box<crate::day_ceiling::BurnStop>),
}

/// Refuse a transition that needs the turn `e`'s holder took: `e` must be
/// running, and no stop may have landed during that turn (W1).
pub(crate) fn require_turn(e: &Execution) -> Result<()> {
    if e.state != ExecState::Running {
        return Err(KernelError::NoTurn {
            id: e.id.clone(),
            state: e.state.as_str(),
        }
        .into());
    }
    if let Some(s) = &e.stopped {
        return Err(KernelError::Stopped {
            id: e.id.clone(),
            by: s.by.clone(),
        }
        .into());
    }
    Ok(())
}

/// What accepting a completion did (§3.16: idempotent, quarantines strays).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "result")]
pub enum Accepted {
    /// Action settled and the execution's continuation written in one frame.
    Settled {
        correlation_id: CorrelationId,
        execution_id: ExecutionId,
        execution_state: ExecState,
    },
    /// Already settled; this delivery was a duplicate. Logged, nothing changed.
    DuplicateNoop { correlation_id: CorrelationId },
    /// Already settled, by another taker of the same spooled completion
    /// (`take_completion_with`, Tier 7.1): nothing written.
    Taken { correlation_id: CorrelationId },
    /// No action carries this correlation id; recorded and surfaced, never inferred.
    Quarantined { correlation_id: CorrelationId },
    /// The action had been cancelled; the result is recorded as a resolution
    /// but the execution is not revived.
    LateAfterCancel { correlation_id: CorrelationId },
    /// An `outcome_unknown` record now has authoritative evidence.
    ResolvedUnknown {
        correlation_id: CorrelationId,
        outcome: Outcome,
    },
}

/// How a held turn ends. The Advancer decides; the kernel records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "end")]
pub enum TurnEnd {
    /// Work done; the execution is finished.
    Complete {
        reason: String,
    },
    /// No runnable work; park until the wake fires.
    Wait {
        wake: Wake,
    },
    /// Still runnable (yielding the turn to the scheduler, e.g. admission fairness).
    Requeue,
    Fail {
        reason: String,
    },
    Blocked {
        reason: String,
    },
}

/// A held turn. Ended with `Kernel::end_turn`; dropped without ending means
/// the process died mid-turn, which startup recovers as `interrupted`.
#[must_use = "end the turn with Kernel::end_turn"]
pub struct TurnGuard {
    pub execution_id: ExecutionId,
    pub turn: u64,
    pub started_at_ms: u64,
    held: Arc<Mutex<HashSet<ExecutionId>>>,
}

impl std::fmt::Debug for TurnGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnGuard")
            .field("execution_id", &self.execution_id)
            .field("turn", &self.turn)
            .finish()
    }
}

impl Drop for TurnGuard {
    fn drop(&mut self) {
        self.held.lock().unwrap().remove(&self.execution_id);
    }
}

/// Evidence the reconciler can consult about a dispatched action (§3.16):
/// the spool, the job wrapper's pid, an external service.
pub trait Evidence: Send + Sync {
    fn probe(&self, action: &Action) -> Probe;
    /// The verdict a wrapper left for a cancel that stopped `action`, if one did (theseus-1og8): a
    /// daemon that died after the stop and before it read the verdict finds it here at its start.
    fn stopped(&self, _action: &Action) -> Option<Verdict> {
        None
    }
    /// `action`'s verdict is taken: remove it.
    fn stop_taken(&self, _action: &Action) {}
    /// Whether it ran in the kernel's own process, with no evidence outside it: a provider call.
    fn in_process(&self, action: &Action) -> bool {
        action.tool == PROVIDER_TOOL
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Probe {
    /// A result exists; here it is.
    Completed(Completion),
    /// The job is alive.
    StillRunning,
    /// No result and no process: the outcome cannot be established from here.
    Gone,
}

/// No evidence available (unit tests, the in-process transport).
pub struct NoEvidence;
impl Evidence for NoEvidence {
    fn probe(&self, _: &Action) -> Probe {
        Probe::Gone
    }
}

/// What `cancel_execution_with` did.
#[derive(Debug, Clone, Default)]
pub struct Cancel {
    /// The dispatched calls and jobs now told to stop: the caller terminates
    /// their backends and walks each one's cancel.
    pub to_kill: Vec<CorrelationId>,
    /// What the execution planned and never sent, each now `Cancelled`
    /// (theseus-w98): tool calls, those waiting for the operator among them,
    /// and the budget question.
    pub not_run: Vec<Action>,
}

/// What a cancel ended, for the records its caller writes in the cancel's
/// own frame (`cancel_execution_with`).
#[derive(Debug)]
pub struct Ending<'a> {
    /// The execution as the cancel leaves it.
    pub execution: &'a Execution,
    /// What it planned and never sent, as the cancel leaves each one.
    pub not_run: &'a [Action],
    /// A turn held the execution when the cancel came: its transcript is the
    /// turn's to write, and the turn ends at its next step.
    pub turn_running: bool,
}

/// The cancel's own result (`Kernel::cancel`): what it ended, owned, and the
/// dispatched calls it told to stop.
struct Cancelled {
    execution: Execution,
    not_run: Vec<Action>,
    turn_running: bool,
    to_kill: Vec<CorrelationId>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ReconcileReport {
    pub woke_due: Vec<ExecutionId>,
    pub settled_from_evidence: Vec<CorrelationId>,
    pub marked_unknown: Vec<CorrelationId>,
    pub still_running_past_deadline: Vec<CorrelationId>,
    pub resolved_unknown: Vec<CorrelationId>,
    /// Calls a cancel had stopped before an earlier process died, settled from the verdict their
    /// wrapper left (theseus-1og8).
    #[serde(default)]
    pub settled_from_stop: Vec<CorrelationId>,
    pub open_actions: u64,
    pub open_executions: u64,
    pub elapsed_us: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StartupReport {
    pub steps: Vec<StartupStep>,
    pub requeued_interrupted: Vec<ExecutionId>,
    /// Executions the old unit budget ended whose dollar spend is under
    /// their limit, reopened in step 2 to wait on input (theseus-3ebd).
    #[serde(default)]
    pub reopened: Vec<ExecutionId>,
    /// Open executions that took a changed spend limit in step 2
    /// (theseus-3pj).
    #[serde(default)]
    pub limits_followed: Vec<LimitFollowed>,
    pub spool_drained: u32,
    pub spool_quarantined: u32,
    /// Completions whose rename a crash cut short, put in place by step 3
    /// and drained with the rest (theseus-yxiv).
    #[serde(default)]
    pub spool_recovered: u32,
    pub reconcile: ReconcileReport,
    pub elapsed_us: u64,
}

/// One open execution that took the config's changed spend limit
/// (theseus-3pj), as its `budget.limit_changed` row says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LimitFollowed {
    pub execution_id: ExecutionId,
    pub session_id: SessionId,
    pub from_micros: Micros,
    pub to_micros: Micros,
    /// The budget question a raise withdrew, still unanswered until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub withdrew: Option<CorrelationId>,
    /// A raise let the call that waited at the old limit proceed: the
    /// question joined the queued results, and a waiting execution was
    /// queued for the driver.
    #[serde(default)]
    pub proceeds: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartupStep {
    pub step: u8,
    pub name: String,
    pub elapsed_us: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct KernelStats {
    pub executions_by_state: BTreeMap<String, u64>,
    /// The waiting ones by wake (theseus-g00vv; `waiting.rs`).
    pub waiting_by_wake: crate::waiting::WaitingByWake,
    pub actions_by_state: BTreeMap<String, u64>,
    pub turns_held: u32,
    pub admission_ceiling: u32,
    pub quarantined_completions: u64,
    pub accepting: bool,
}

/// A frame the kernel committed, as its observer sees it (theseus-in3): the
/// records, with the WAL position of each, once the append has returned, so
/// nothing observed is a state the WAL does not hold.
pub struct Committed<'a> {
    pub records: &'a [NewRecord],
    pub positions: &'a [u64],
}

/// Sees every frame the kernel commits (theseus-in3, the push). It runs on
/// the committing thread, under the execution's lock, so it must only hand
/// the frame on: a filter and a send, never a wait.
pub type Observer = Arc<dyn Fn(Committed<'_>) + Send + Sync>;

pub struct Kernel {
    store: Arc<dyn Store>,
    clock: Arc<dyn Clock>,
    cfg: KernelConfig,
    held: Arc<Mutex<HashSet<ExecutionId>>>,
    /// One writer at a time per execution. Shared with every view.
    locks: Arc<ExecLocks>,
    /// 0..=4 while starting; 5 once accepting events. Shared with every view.
    phase: Arc<Mutex<u8>>,
    /// When this process's startup began (0 before it): a wake due before
    /// then fell due while the daemon was down (DD8). Shared with every view.
    pub(crate) started_at_ms: Arc<AtomicU64>,
    /// A turn's view: the results it reads itself (`turn_of`).
    own: Option<Arc<OwnResults>>,
    /// The push's observer, once something watches (theseus-in3). Shared
    /// with every view, so the turns already running see it when the first
    /// watcher installs it; until then the commit path pays one load.
    observer: Arc<std::sync::OnceLock<Observer>>,
    /// The transaction this view stages for (`Kernel::frame`), if it is one:
    /// its transitions take no locks, and its commits are staged.
    tx: Option<Arc<Tx>>,
    /// What startup found an earlier process left dispatched: its in-process
    /// calls (theseus-m9iy) and its jobs, probed after serving (theseus-vej5).
    pub(crate) earlier: Arc<Mutex<crate::earlier::Earlier>>,
    /// The day ceiling's counter (theseus-kp20). Shared with every view.
    pub(crate) ceiling: Arc<crate::day_ceiling::DayCeiling>,
}

/// A turn's own results (theseus-l6y): what a turn's view settles for the
/// turn's execution, which that turn reads itself (the provider call's
/// answer, an in-process call's result, a job it waited for). No later turn
/// needs them, so they are not queued. `settled` counts them: a turn that
/// faults after one settled is woken, as its queued entry once requeued it.
struct OwnResults {
    execution_id: ExecutionId,
    settled: AtomicU32,
}

const QUARANTINE_PREFIX: &str = "quarantine:";

/// Every execution state, as health counts them.
pub(crate) const EXEC_STATES: [ExecState; 8] = [
    ExecState::Queued,
    ExecState::Running,
    ExecState::Waiting,
    ExecState::Blocked,
    ExecState::Cancelled,
    ExecState::Failed,
    ExecState::BudgetExhausted,
    ExecState::Complete,
];
/// The states of an execution that has not ended.
pub(crate) const OPEN_EXECUTIONS: [ExecState; 4] = [
    ExecState::Queued,
    ExecState::Running,
    ExecState::Waiting,
    ExecState::Blocked,
];
/// Every action state, as health counts them.
pub(crate) const ACTION_STATES: [ActionState; 7] = [
    ActionState::Planned,
    ActionState::Authorized,
    ActionState::Dispatched,
    ActionState::Succeeded,
    ActionState::Failed,
    ActionState::OutcomeUnknown,
    ActionState::Cancelled,
];
/// The states of an action that is not settled.
pub(crate) const OPEN_ACTIONS: [ActionState; 4] = [
    ActionState::Planned,
    ActionState::Authorized,
    ActionState::Dispatched,
    ActionState::OutcomeUnknown,
];

impl Kernel {
    /// Wrap an opened store. The kernel is not accepting events until
    /// `startup` has run; `open_session`/`admit` etc. refuse before that.
    pub fn new(store: Arc<dyn Store>, clock: Arc<dyn Clock>, cfg: KernelConfig) -> Self {
        let ceiling = crate::day_ceiling::DayCeiling::new(
            cfg.daily_ceiling_micros,
            cfg.zone.clone(),
            clock.clone(),
        );
        ceiling.set_rules(
            cfg.daily_notify,
            cfg.daily_runaway_micros,
            cfg.daily_step_micros,
            cfg.burn_line_micros,
        );
        Self {
            store,
            clock: clock.clone(),
            held: Arc::new(Mutex::new(HashSet::new())),
            locks: Arc::default(),
            phase: Arc::new(Mutex::new(0)),
            started_at_ms: Arc::default(),
            own: None,
            observer: Arc::default(),
            tx: None,
            earlier: Arc::default(),
            ceiling: Arc::new(ceiling),
            cfg,
        }
    }

    /// This kernel, committing its frames through `store`, another handle on
    /// the same store: a turn's, whose frames also carry the ledger rows the
    /// turn has waiting (theseus-qa0). The turn locks, the execution locks,
    /// the startup phase, the clock, and the config are shared, so the view
    /// and the kernel are one.
    pub fn view(&self, store: Arc<dyn Store>) -> Kernel {
        Kernel {
            store,
            clock: self.clock.clone(),
            cfg: self.cfg.clone(),
            held: self.held.clone(),
            locks: self.locks.clone(),
            phase: self.phase.clone(),
            started_at_ms: self.started_at_ms.clone(),
            own: None,
            observer: self.observer.clone(),
            tx: None,
            earlier: self.earlier.clone(),
            ceiling: self.ceiling.clone(),
        }
    }

    /// This kernel as a transaction's view (`Kernel::frame`): the same turn
    /// (`turn_of`), locks, and clock, its commits staged in `staged`, and no
    /// observer, which sees the frame once it commits.
    pub(crate) fn transaction_view(&self, tx: Arc<Tx>, staged: Arc<Staged>) -> Kernel {
        Kernel {
            store: staged,
            clock: self.clock.clone(),
            cfg: self.cfg.clone(),
            held: self.held.clone(),
            locks: self.locks.clone(),
            phase: self.phase.clone(),
            started_at_ms: self.started_at_ms.clone(),
            own: self.own.clone(),
            observer: Arc::default(),
            tx: Some(tx),
            earlier: self.earlier.clone(),
            ceiling: self.ceiling.clone(),
        }
    }

    /// The transaction this view stages for, if it is one.
    pub(crate) fn tx(&self) -> Option<&Tx> {
        self.tx.as_deref()
    }

    /// Lock `ids` for one transition (K1), in id order. Inside a transaction,
    /// which locked what it touches first, nothing is locked: each id must
    /// be one it holds.
    pub(crate) fn lock(&self, ids: &[&str]) -> ExecLock<'_> {
        match &self.tx {
            None => self.locks.lock_all(ids),
            Some(tx) => {
                tx.require(ids);
                self.locks.none()
            }
        }
    }

    /// A turn ends: its guard is dropped once its end is written, which
    /// inside a transaction is when the transaction's frame commits.
    fn release(&self, guard: TurnGuard) {
        match &self.tx {
            None => drop(guard),
            Some(tx) => tx.release(guard),
        }
    }

    /// This view as the kernel of one turn of `execution_id` (theseus-l6y):
    /// a completion it accepts for that execution is the turn's own result,
    /// which the turn reads itself. It is not queued for a later turn, so a
    /// plain turn has nothing to consume at its end, and `own_settled`
    /// counts it.
    pub fn turn_of(mut self, execution_id: &str) -> Self {
        self.own = Some(Arc::new(OwnResults {
            execution_id: execution_id.to_string(),
            settled: AtomicU32::new(0),
        }));
        self
    }

    /// Whether a turn holds `execution_id` in this process now. A turn's end
    /// frees its hold under the execution's lock, so a caller that reads this
    /// inside `Kernel::frame` for the execution sees the end or not at all:
    /// a cancel's sweep of the calls it left unanswered waits for the turn
    /// that was running, which sweeps at its own end (theseus-0o8).
    pub fn holds_turn(&self, execution_id: &str) -> bool {
        self.held.lock().unwrap().contains(execution_id)
    }

    /// How many of its turn's own results this view has settled: a turn that
    /// faults after one did is woken, so its continuation reads them.
    pub fn own_settled(&self) -> u32 {
        self.own
            .as_ref()
            .map_or(0, |o| o.settled.load(std::sync::atomic::Ordering::SeqCst))
    }

    pub fn store(&self) -> &Arc<dyn Store> {
        &self.store
    }
    pub fn config(&self) -> &KernelConfig {
        &self.cfg
    }
    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }
    /// When this process's startup began, 0 before it: a time that passed
    /// before it passed while the daemon was down (DD8).
    pub fn process_started_ms(&self) -> u64 {
        self.started_at_ms
            .load(std::sync::atomic::Ordering::Relaxed)
    }
    /// Is a turn held on this execution in this process?
    pub fn is_held(&self, execution_id: &str) -> bool {
        self.held.lock().unwrap().contains(execution_id)
    }

    pub fn is_accepting(&self) -> bool {
        *self.phase.lock().unwrap() >= 5
    }

    pub(crate) fn require_accepting(&self) -> Result<()> {
        let p = *self.phase.lock().unwrap();
        if p < 5 {
            return Err(KernelError::NotAccepting { step: p }.into());
        }
        Ok(())
    }

    // ------------------------------------------------------------ reads

    /// Every read of an execution goes through the versioned reader, so an
    /// execution stored with a unit budget reads in dollars.
    fn read_execution(&self, r: &Record) -> Result<Execution> {
        Execution::from_stored(&r.payload, self.cfg.spend_limit_micros)
            .with_context(|| format!("execution record at {}", r.position))
    }

    pub fn execution(&self, id: &str) -> Result<Option<Execution>> {
        match self.store.latest_by_key(kinds::EXECUTION, id)? {
            Some(r) => Ok(Some(self.read_execution(&r)?)),
            None => Ok(None),
        }
    }
    pub fn action(&self, correlation_id: &str) -> Result<Option<Action>> {
        decode_opt(self.store.latest_by_key(kinds::ACTION, correlation_id)?)
    }

    /// An action, read under its execution's lock: read once to learn its
    /// execution, which never changes, then lock that and read it again, so
    /// that what the caller decides comes from the read inside the lock. A
    /// task's action locks every execution above it too (`lock_family`): what
    /// it settles is their spend as well (DD7, theseus-deaqt).
    pub(crate) fn locked_action(
        &self,
        correlation_id: &str,
    ) -> Result<Option<(ExecLock<'_>, Action)>> {
        let Some(a) = self.action(correlation_id)? else {
            return Ok(None);
        };
        let lock = self.lock_family(&a.execution_id)?;
        let a = self
            .action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?;
        Ok(Some((lock, a)))
    }

    /// `execution_id` and every execution above it, nearest first, up to the
    /// conversation; a conversation is its own whole family. Read outside any
    /// lock, which is sound because an execution's `parent` is written once,
    /// when it is opened, and never changed. A chain that names an id twice,
    /// which only a corrupt store could hold, stops there instead of looping.
    pub(crate) fn family(&self, execution_id: &str) -> Result<Vec<String>> {
        let mut out = vec![execution_id.to_string()];
        let mut seen: HashSet<String> = HashSet::new();
        seen.insert(execution_id.to_string());
        let mut at = execution_id.to_string();
        while let Some(p) = self.execution(&at)?.and_then(|e| e.parent) {
            if !seen.insert(p.clone()) {
                break;
            }
            out.push(p.clone());
            at = p;
        }
        Ok(out)
    }

    /// Lock an execution for a transition that may reach the executions above
    /// it: a task (DD7) and **every** execution above it, up to the
    /// conversation, in id order, since a task's settled spend is the spend of
    /// each of them and is written into all of them in the one frame
    /// (`carry_to_parent`, theseus-deaqt), and its end releases the carve in
    /// each of them too (`task_ended`); any other execution alone. The chain
    /// is read outside the lock (`family`).
    ///
    /// **The ordering rule: name every execution you may write in the one
    /// `lock_all` call (`lock`, `lock_family`, or `Kernel::frame`), never a
    /// lock inside a lock.** That is why a family of any length cannot
    /// deadlock, and `locks.rs`'s module doc argues it: one call, sorted ids,
    /// and a second call while a lock is held panics instead of waiting, so a
    /// thread waits only for an id greater than every id it holds.
    pub(crate) fn lock_family(&self, execution_id: &str) -> Result<ExecLock<'_>> {
        let family = self.family(execution_id)?;
        let ids: Vec<&str> = family.iter().map(String::as_str).collect();
        Ok(self.lock(&ids))
    }

    /// `locked_action`, for a transition that needs the action to exist.
    pub(crate) fn locked_known_action(
        &self,
        correlation_id: &str,
    ) -> Result<(ExecLock<'_>, Action)> {
        self.locked_action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()).into())
    }

    #[cfg(test)]
    pub(crate) fn exec_locks(&self) -> &ExecLocks {
        &self.locks
    }
    // ------------------------------------------------------------ frames

    pub(crate) fn ledger(
        &self,
        kind: LedgerKind,
        session: Option<&str>,
        data: Value,
    ) -> Result<NewRecord> {
        let row = LedgerRow {
            at_unix_ms: self.now_ms(),
            kind: kind.as_str().into(),
            session_id: session.map(str::to_string),
            turn_id: None,
            data,
        };
        let r = NewRecord::json(kinds::LEDGER, None, &row)?;
        Ok(match session {
            Some(s) => r.scoped(s),
            None => r,
        })
    }

    /// Append a frame, then hand it to the observer, if one is installed
    /// (theseus-in3). Every EXECUTION and ACTION record is written here, and
    /// each once a frame (Tier 7.3, `tx::once_each`): a transaction stages a
    /// copy at each of its transitions, and only the last can be read.
    pub(crate) fn commit(&self, frame: &[NewRecord]) -> Result<Vec<u64>> {
        let once = match self.tx() {
            // A transaction's view stages, and reads the newest copy it staged.
            Some(_) => None,
            None => crate::tx::once_each(frame),
        };
        let frame = once.as_deref().unwrap_or(frame);
        let positions = self.store.append(frame).context("kernel frame")?;
        if let Some(observe) = self.observer.get() {
            observe(Committed {
                records: frame,
                positions: &positions,
            });
        }
        Ok(positions)
    }

    /// Install the push's observer (theseus-in3): every frame committed from
    /// now on, by this kernel or any view of it, is handed to it. Once only:
    /// false if one was installed already.
    pub fn observe(&self, observer: Observer) -> bool {
        self.observer.set(observer).is_ok()
    }

    /// Whether an observer is installed: false until something watches.
    pub fn observed(&self) -> bool {
        self.observer.get().is_some()
    }

    /// The per-execution locks, which the outbox also takes by its own keys.
    pub(crate) fn locks(&self) -> &ExecLocks {
        &self.locks
    }

    // ------------------------------------------------------------ sessions

    /// Create the one execution for a session whose record someone else
    /// owns (the core's `SessionRecord`). Starts `Waiting` on input, with
    /// the configured spend limit, which it follows when the config changes,
    /// or `limit_micros`, which it keeps (`Budget::pinned`).
    pub fn open_execution(
        &self,
        session_id: &str,
        kind: SessionKind,
        authority: Authority,
        limit_micros: Option<Micros>,
        reports_to: Option<String>,
    ) -> Result<Execution> {
        self.require_accepting()?;
        let now = self.now_ms();
        let exec = Execution {
            id: new_id("exe"),
            schema: SCHEMA,
            session_id: session_id.to_string(),
            kind,
            state: ExecState::Waiting,
            authority,
            budget: Budget {
                pinned: limit_micros.is_some(),
                ..Budget::new(limit_micros.unwrap_or(self.cfg.spend_limit_micros))
            },
            wake: Some(Wake::Input),
            outstanding: vec![],
            queued_results: vec![],
            parent: None,
            reports_to,
            reports: vec![],
            wakes: vec![],
            wake_parent: false,
            report_wakes: vec![],
            helper: None,
            stopped: None,
            turns: 0,
            interrupted: 0,
            resume_pending: false,
            cancel: None,
            ended_reason: None,
            dismissed: None,
            retry_of: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.commit(&[
            exec_record(&exec)?,
            self.ledger(
                LedgerKind::ExecutionOpened,
                Some(session_id),
                json!({"execution_id": exec.id, "kind": kind, "limit_usd": micros_to_usd(exec.budget.limit_micros)}),
            )?,
        ])?;
        Ok(exec)
    }

    // ------------------------------------------------------------ turns

    /// Human input arrived on a session: its execution becomes runnable.
    pub fn wake_input(&self, execution_id: &str) -> Result<Execution> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        if e.state.is_terminal() {
            return Err(KernelError::NotRunnable {
                id: e.id.clone(),
                state: e.state.as_str(),
            }
            .into());
        }
        if e.state == ExecState::Waiting || e.state == ExecState::Blocked {
            e.state = ExecState::Queued;
            e.wake = None;
            e.updated_at_ms = self.now_ms();
            self.commit(&[
                exec_record(&e)?,
                self.ledger(
                    LedgerKind::ExecutionQueued,
                    Some(&e.session_id),
                    json!({"execution_id": e.id, "why": "input"}),
                )?,
            ])?;
        }
        Ok(e)
    }

    /// Something other than human input made the execution runnable (a confirm
    /// answer, an operator nudge): `Waiting`/`Blocked` → `Queued`, flagged so
    /// the harness's driver takes a turn for it without waiting for input.
    pub fn wake(&self, execution_id: &str, why: &str) -> Result<Execution> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        if e.state.is_terminal() {
            return Err(KernelError::NotRunnable {
                id: e.id.clone(),
                state: e.state.as_str(),
            }
            .into());
        }
        if matches!(
            e.state,
            ExecState::Waiting | ExecState::Blocked | ExecState::Queued
        ) {
            e.state = ExecState::Queued;
            e.wake = None;
            e.resume_pending = true;
            e.updated_at_ms = self.now_ms();
            self.commit(&[
                exec_record(&e)?,
                self.ledger(
                    LedgerKind::ExecutionQueued,
                    Some(&e.session_id),
                    json!({"execution_id": e.id, "why": why}),
                )?,
            ])?;
        }
        Ok(e)
    }

    /// A human declined a planned action (a confirm answered "no", or new
    /// input superseded the question). The action settles `Cancelled` with the
    /// reason as its resolution and its reservation released; it never ran.
    /// A declined budget question leaves its execution waiting on the budget.
    /// Rows written before theseus-8az say `action.denied` and `denied by`.
    pub fn decline_action(&self, correlation_id: &str, by: &str, reason: &str) -> Result<Action> {
        let (_w, mut a) = self.locked_known_action(correlation_id)?;
        if !matches!(a.state, ActionState::Planned | ActionState::Authorized) {
            return Err(KernelError::ActionState {
                correlation_id: a.correlation_id.clone(),
                state: a.state.as_str(),
                expected: "planned or authorized",
            }
            .into());
        }
        let now = self.now_ms();
        a.state = ActionState::Cancelled;
        a.settled_at_ms = Some(now);
        a.resolution = Some(format!("declined by {by}: {reason}"));
        let mut frame = vec![action_record(&a)?];
        let mut day = None;
        if let Some(mut e) = self.execution(&a.execution_id)? {
            let spent_before = e.budget.spent_micros;
            if let Some(r) = &a.reservation_id {
                settle_reservation_in(&mut e.budget, r, Some(0));
                day = Effect::of(&a, Some(0));
            }
            if e.budget.question.as_deref() == Some(correlation_id) {
                e.budget.question = None;
                e.budget.question_needs_micros = 0;
            }
            e.updated_at_ms = now;
            frame.push(exec_record(&e)?);
            self.carry_to_parent(&e, spent_before, &mut frame)?;
        }
        frame.push(self.ledger(
            LedgerKind::ActionDeclined,
            Some(&a.session_id),
            json!({"correlation_id": a.correlation_id, "tool": a.tool, "by": by, "reason": reason}),
        )?);
        self.commit(&frame)?;
        self.day_after(day);
        Ok(a)
    }

    /// Take a turn: the execution must be `Queued`, the ceiling must have
    /// room, and no turn may be held on it in this process. Writes `Running`.
    /// The guard frees the turn when dropped, so a caller whose frame fails
    /// lets it go.
    pub fn admit(&self, execution_id: &str) -> Result<TurnGuard> {
        self.admit_counted(execution_id, true)
    }

    /// `admit` for a turn the core's admission queue gave a slot
    /// (theseus-4g0l): the queue keeps the ceiling, so it is not counted
    /// here, where a turn that lent its slot while it waits on a job still
    /// holds its turn.
    pub fn admit_queued(&self, execution_id: &str) -> Result<TurnGuard> {
        self.admit_counted(execution_id, false)
    }

    fn admit_counted(&self, execution_id: &str, count: bool) -> Result<TurnGuard> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        if e.state != ExecState::Queued {
            return Err(KernelError::NotRunnable {
                id: e.id.clone(),
                state: e.state.as_str(),
            }
            .into());
        }
        let guard = self.hold(&e, count)?;
        // A frame that fails drops the guard, which frees the turn.
        self.write_running(e, &guard)?;
        Ok(guard)
    }

    /// The turn's slot in this process, and nothing written: no other turn
    /// may be held on `e`, and, with `count`, the ceiling must have room
    /// (`held.rs`); without it the core's admission queue keeps the ceiling
    /// (theseus-4g0l).
    pub(crate) fn hold(&self, e: &Execution, count: bool) -> Result<TurnGuard> {
        {
            let mut held = self.held.lock().unwrap();
            if held.contains(&e.id) {
                return Err(KernelError::TurnHeld { id: e.id.clone() }.into());
            }
            if count && held.len() as u32 >= self.cfg.admission_ceiling {
                return Err(KernelError::AdmissionFull {
                    ceiling: self.cfg.admission_ceiling,
                }
                .into());
            }
            held.insert(e.id.clone());
        }
        Ok(TurnGuard {
            execution_id: e.id.clone(),
            turn: e.turns + 1,
            started_at_ms: self.now_ms(),
            held: self.held.clone(),
        })
    }

    /// Write `e`, queued, as running the turn `guard` holds: the admission's
    /// record and row. The caller holds the execution's lock.
    pub(crate) fn write_running(&self, mut e: Execution, guard: &TurnGuard) -> Result<()> {
        e.state = ExecState::Running;
        e.turns = guard.turn;
        e.wake = None;
        let resumed = std::mem::take(&mut e.resume_pending);
        e.updated_at_ms = self.now_ms();
        self.commit(&[
            exec_record(&e)?,
            self.ledger(
                LedgerKind::ExecutionRunning,
                Some(&e.session_id),
                json!({"execution_id": e.id, "turn": e.turns, "queued_results": e.queued_results.len(), "resumed": resumed}),
            )?,
        ])?;
        Ok(())
    }

    /// Human input arrived, and its turn starts (theseus-l6y): `wake_input`
    /// and `admit` in one frame, a transaction (theseus-0owd). Each keeps its
    /// record and row, in that order, so the WAL reads as the two transitions
    /// did in two frames. When admission must wait (the ceiling, or a turn
    /// already held on the execution), only the wake is written, as
    /// `wake_input` alone would write it, and admission's error is returned.
    pub fn admit_input(&self, execution_id: &str) -> Result<TurnGuard> {
        self.frame(&[execution_id], |k| {
            k.wake_input(execution_id)?;
            Ok(k.admit(execution_id))
        })?
    }

    /// `admit_input` for the core's admission queue (theseus-4g0l): with a
    /// slot (`slotted`), as `admit_queued`; without one, only the wake is
    /// written, and admission's error returned, as a full ceiling's.
    pub fn admit_input_queued(&self, execution_id: &str, slotted: bool) -> Result<TurnGuard> {
        let ceiling = self.cfg.admission_ceiling;
        self.frame(&[execution_id], |k| {
            k.wake_input(execution_id)?;
            if slotted {
                return Ok(k.admit_queued(execution_id));
            }
            // What `admit` would say first: a state that takes no turn.
            Ok(match k.execution(execution_id)? {
                Some(e) if e.state != ExecState::Queued => Err(KernelError::NotRunnable {
                    id: e.id.clone(),
                    state: e.state.as_str(),
                }
                .into()),
                _ => Err(KernelError::AdmissionFull { ceiling }.into()),
            })
        })?
    }

    /// `take_results`, with what the turn makes of them (`extra`: the nodes
    /// it writes for them) in the same frame (theseus-kol), a transaction. A
    /// result leaves the queue only in the frame that writes it into the
    /// session, so no crash between the two can lose it. Nothing queued, and
    /// nothing is written; `extra` is not called.
    pub fn take_results_with(
        &self,
        guard: &TurnGuard,
        extra: impl FnOnce(&[Action]) -> Result<Vec<NewRecord>>,
    ) -> Result<Vec<Action>> {
        self.frame(&[&guard.execution_id], |k| {
            let before = k.staged_len();
            let taken = k.take_results(guard)?;
            if k.staged_len() > before {
                k.stage(&extra(&taken)?)?;
            }
            Ok(taken)
        })
    }

    /// The results queued for this execution (settled since its last turn),
    /// and clear them in the store as consumed. Call inside a held turn.
    /// Nothing queued, and nothing is written.
    pub fn take_results(&self, guard: &TurnGuard) -> Result<Vec<Action>> {
        let _w = self.lock(&[&guard.execution_id]);
        let mut e = self
            .execution(&guard.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(guard.execution_id.clone()))?;
        if e.queued_results.is_empty() {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        for c in &e.queued_results {
            if let Some(a) = self.action(c)? {
                out.push(a);
            }
        }
        let n = e.queued_results.len();
        e.queued_results.clear();
        e.updated_at_ms = self.now_ms();
        self.commit(&[
            exec_record(&e)?,
            self.ledger(
                LedgerKind::ExecutionResultsConsumed,
                Some(&e.session_id),
                json!({"execution_id": e.id, "count": n}),
            )?,
        ])?;
        Ok(out)
    }

    /// `end_turn`, with records `extra` builds from the execution as the
    /// frame writes it (a task's report, DD7; the turn's session record), a
    /// transaction. `extra` runs only when the end writes something; a turn
    /// that ends after a cancel landed writes nothing, so its report is the
    /// cancel's alone.
    pub fn end_turn_with(
        &self,
        guard: TurnGuard,
        end: TurnEnd,
        extra: impl FnOnce(&Execution) -> Result<Vec<NewRecord>>,
    ) -> Result<Execution> {
        let id = guard.execution_id.clone();
        self.frame(&[&id], |k| {
            let before = k.staged_len();
            let e = k.end_turn(guard, end)?;
            if k.staged_len() > before {
                k.stage(&extra(&e)?)?;
            }
            Ok(e)
        })
    }

    /// End the held turn with the Advancer's decision. Consumes the guard,
    /// which frees the turn once the end is written. A task that ends here
    /// (complete or failed) reaches its parent in the same frame (DD7): the
    /// carve is released but for what the task still has in flight, and the
    /// task joins the parent's `reports`.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub fn end_turn(&self, guard: TurnGuard, end: TurnEnd) -> Result<Execution> {
        let _w = self.lock_family(&guard.execution_id)?;
        let mut e = self
            .execution(&guard.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(guard.execution_id.clone()))?;
        let now = self.now_ms();
        // A cancel or budget exhaustion that landed during the turn wins over
        // the Advancer: terminal states are never overwritten. The lock makes
        // this read the last word: a cancel lands before it, or after the
        // frame below.
        if e.state.is_terminal() {
            self.release(guard);
            return Ok(e);
        }
        // A stop that landed during the turn (W1) wins over the Advancer too,
        // but keeps the execution: it waits on its next input, whatever the
        // turn would have waited on, and nothing queues it here. A wake of its
        // own, or a task's report that asks for a turn, runs at the driver's
        // next tick, as it would for any free execution.
        if let Some(s) = e.stopped.take() {
            e.state = ExecState::Waiting;
            e.wake = Some(Wake::Input);
            e.resume_pending = false;
            e.updated_at_ms = now;
            self.commit(&[
                exec_record(&e)?,
                self.ledger(
                    LedgerKind::ExecutionWaiting,
                    Some(&e.session_id),
                    json!({"execution_id": e.id, "turn": guard.turn, "turn_ms": now.saturating_sub(guard.started_at_ms),
                           "wake": e.wake, "why": "stopped", "by": s.by}),
                )?,
            ])?;
            self.release(guard);
            return Ok(e);
        }
        let kind;
        // Why a turn that would park is queued at once instead: a wake of its
        // own came due while it ran, or a task's report asked for a turn
        // (W1), and it would wait where a wake may fire (DD8). The ledger row
        // says so.
        let mut why = None;
        match end {
            TurnEnd::Complete { reason } => {
                e.state = ExecState::Complete;
                e.ended_reason = Some(reason);
                kind = LedgerKind::ExecutionComplete;
            }
            TurnEnd::Wait { wake } => {
                // Results that arrived during the turn make it runnable again.
                if !e.queued_results.is_empty() {
                    e.state = ExecState::Queued;
                    e.wake = None;
                    why = Some("result");
                    kind = LedgerKind::ExecutionQueued;
                } else {
                    if let Wake::Actions { correlation_ids } = &wake {
                        // All named actions must still be outstanding; otherwise
                        // the wake would never fire.
                        let open: HashSet<_> = e.outstanding.iter().cloned().collect();
                        if correlation_ids.iter().all(|c| !open.contains(c))
                            && !correlation_ids.is_empty()
                        {
                            e.state = ExecState::Queued;
                            e.wake = None;
                            e.updated_at_ms = now;
                            self.commit(&[
                                exec_record(&e)?,
                                self.ledger(
                                    LedgerKind::ExecutionQueued,
                                    Some(&e.session_id),
                                    json!({"execution_id": e.id, "why": "wake_actions_already_settled"}),
                                )?,
                            ])?;
                            self.release(guard);
                            return Ok(e);
                        }
                    }
                    e.state = ExecState::Waiting;
                    e.wake = Some(wake);
                    if let Some(w) = self.queued_at_end(&e, now)? {
                        e.state = ExecState::Queued;
                        e.wake = None;
                        e.resume_pending = true;
                        why = Some(w);
                        kind = LedgerKind::ExecutionQueued;
                    } else {
                        kind = LedgerKind::ExecutionWaiting;
                    }
                }
            }
            TurnEnd::Requeue => {
                e.state = ExecState::Queued;
                e.wake = None;
                kind = LedgerKind::ExecutionQueued;
            }
            TurnEnd::Fail { reason } => {
                e.state = ExecState::Failed;
                e.ended_reason = Some(reason);
                kind = LedgerKind::ExecutionFailed;
            }
            TurnEnd::Blocked { reason } => {
                e.state = ExecState::Blocked;
                e.ended_reason = Some(reason);
                kind = LedgerKind::ExecutionBlocked;
            }
        }
        e.updated_at_ms = now;
        let mut row = json!({"execution_id": e.id, "turn": guard.turn, "turn_ms": now.saturating_sub(guard.started_at_ms), "wake": e.wake, "reason": e.ended_reason});
        if let Some(w) = why {
            row["why"] = json!(w);
        }
        let mut frame = vec![
            exec_record(&e)?,
            self.ledger(kind, Some(&e.session_id), row)?,
        ];
        // Ending with work still outstanding is a cancel of that work: the
        // actions get `cancel_requested` in the same frame and the harness
        // terminates their backends (they stay in `outstanding` until verified).
        // What it planned and never sent closes with it, the budget question
        // among them (theseus-w98).
        let mut day = Vec::new();
        if e.state.is_terminal() {
            let why = format!("the execution ended ({})", e.state.as_str());
            let (_, effects) = self.end_unsent(&mut e, "the harness", &why, &mut frame)?;
            day = effects;
            frame[0] = exec_record(&e)?;
            // Its wakes end with it (DD8).
            if !e.wakes.is_empty() {
                let why = format!("the execution ended ({})", e.state.as_str());
                self.drop_wakes(&mut e, "the harness", &why, &mut frame)?;
                frame[0] = exec_record(&e)?;
            }
            for c in &e.outstanding {
                if let Some(mut a) = self.action(c)? {
                    if a.state == ActionState::Dispatched && a.cancel.is_none() {
                        a.cancel = Some(CancelState::Requested);
                        frame.push(action_record(&a)?);
                        frame.push(self.ledger(
                            LedgerKind::ActionCancel,
                            Some(&a.session_id),
                            json!({"correlation_id": a.correlation_id, "cancel": CancelState::Requested, "why": "execution ended", "settled": false}),
                        )?);
                    }
                }
            }
            self.task_ended(&e, &mut frame)?;
        }
        self.commit(&frame)?;
        self.day_after(day);
        self.release(guard);
        Ok(e)
    }

    // ------------------------------------------------------------ actions

    /// `planned`: mint the correlation id and commit before anything else
    /// happens (§3.16). Requires a held turn (the execution is `Running`).
    /// `reserve_micros` reserves budget in the same frame. A reservation that
    /// does not fit is refused with `OverBudget` and writes nothing; the
    /// execution goes on running, and the caller asks the operator.
    pub fn plan_action(
        &self,
        guard: &TurnGuard,
        proposal: &Proposal,
        retry_class: RetryClass,
        deadline_ms: Option<u64>,
        reserve_micros: Micros,
    ) -> Result<Action> {
        self.plan(
            guard,
            proposal,
            retry_class,
            deadline_ms,
            reserve_micros,
            false,
        )
    }

    /// `planned` for a call that waits for the operator's confirm (the policy
    /// stopped it for approval). It reserves nothing, and the action keeps its
    /// proposal, which the confirm binds and the resumed turn authorizes
    /// (theseus-0g4).
    pub fn plan_confirm(
        &self,
        guard: &TurnGuard,
        proposal: &Proposal,
        retry_class: RetryClass,
        deadline_ms: Option<u64>,
    ) -> Result<Action> {
        self.plan(guard, proposal, retry_class, deadline_ms, 0, true)
    }

    /// `plan_action`, with records the caller builds from the minted action
    /// (its correlation id) written in the same frame, a transaction: a
    /// `ToolCall` node, say.
    pub fn plan_action_with(
        &self,
        guard: &TurnGuard,
        proposal: &Proposal,
        retry_class: RetryClass,
        deadline_ms: Option<u64>,
        reserve_micros: Micros,
        extra: impl FnOnce(&Action) -> Result<Vec<NewRecord>>,
    ) -> Result<Action> {
        self.frame(&[&guard.execution_id], |k| {
            let a = k.plan_action(guard, proposal, retry_class, deadline_ms, reserve_micros)?;
            k.stage(&extra(&a)?)?;
            Ok(a)
        })
    }

    /// `plan_action_with`, `authorize`, and `dispatch` in one frame, a
    /// transaction, for an action that needs no confirm: the provider call,
    /// and a tool call the policy runs (theseus-qa0). Each transition keeps
    /// its own record and row, in that order, so the record reads as it did
    /// in three frames; a crash leaves all three or none of them. Over
    /// budget, it writes nothing.
    pub fn plan_and_dispatch(
        &self,
        guard: &TurnGuard,
        proposal: &Proposal,
        retry_class: RetryClass,
        deadline_ms: Option<u64>,
        reserve_micros: Micros,
        extra: impl FnOnce(&Action) -> Result<Vec<NewRecord>>,
    ) -> Result<Action> {
        self.frame(&[&guard.execution_id], |k| {
            let a = k.plan_action(guard, proposal, retry_class, deadline_ms, reserve_micros)?;
            k.stage(&extra(&a)?)?;
            // The digest the plan took of the same proposal (Tier 7.3).
            k.authorize_or_invalid(&a.correlation_id, &a.args_digest, None)??;
            k.dispatch(&a.correlation_id, None)
        })
    }

    /// `plan_confirm`, with records the caller builds from the minted action
    /// written in the same frame, a transaction.
    pub fn plan_confirm_with(
        &self,
        guard: &TurnGuard,
        proposal: &Proposal,
        retry_class: RetryClass,
        deadline_ms: Option<u64>,
        extra: impl FnOnce(&Action) -> Result<Vec<NewRecord>>,
    ) -> Result<Action> {
        self.frame(&[&guard.execution_id], |k| {
            let a = k.plan_confirm(guard, proposal, retry_class, deadline_ms)?;
            k.stage(&extra(&a)?)?;
            Ok(a)
        })
    }

    /// `planned`, and its frame: the execution's record when it reserves,
    /// the action, and its `action.planned` row.
    fn plan(
        &self,
        guard: &TurnGuard,
        proposal: &Proposal,
        retry_class: RetryClass,
        deadline_ms: Option<u64>,
        reserve_micros: Micros,
        keep_proposal: bool,
    ) -> Result<Action> {
        let _w = self.lock(&[&guard.execution_id]);
        let mut e = self
            .execution(&guard.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(guard.execution_id.clone()))?;
        require_turn(&e)?;
        let now = self.now_ms();
        let mut frame = Vec::new();
        let mut reservation_id = None;
        if reserve_micros > 0 {
            let available = e.budget.available();
            if reserve_micros > available && !self.overdraws_for(&e, &proposal.tool) {
                return Err(KernelError::OverBudget {
                    needed: reserve_micros,
                    available,
                    spent: e.budget.spent_micros,
                    limit: e.budget.limit_micros,
                }
                .into());
            }
            self.hard_stop(&e, &proposal.tool, reserve_micros)?;
            self.burn_stopped(&e, &proposal.tool)?;
            let id = new_id("rsv");
            if proposal.tool == PROVIDER_TOOL {
                self.day_hold(&id, reserve_micros)?;
            }
            e.budget.reserved_micros += reserve_micros;
            e.budget.reservations.insert(id.clone(), reserve_micros);
            reservation_id = Some(id);
            e.updated_at_ms = now;
            frame.push(exec_record(&e)?);
        }
        let a = Action {
            correlation_id: new_id("act"),
            schema: SCHEMA,
            execution_id: e.id.clone(),
            session_id: e.session_id.clone(),
            tool: proposal.tool.clone(),
            args_digest: digest_proposal(proposal),
            proposal: keep_proposal.then(|| proposal.clone()),
            resource: proposal.resource.clone(),
            retry_class,
            state: ActionState::Planned,
            deadline_at_ms: now + deadline_ms.unwrap_or(self.cfg.default_deadline_ms),
            planned_at_ms: now,
            authorized_at_ms: None,
            dispatched_at_ms: None,
            settled_at_ms: None,
            external_op_id: None,
            result_ref: None,
            confirm: None,
            cancel: None,
            verdict: None,
            reservation_id,
            reserved_micros: reserve_micros,
            resolution: None,
            completions_seen: 0,
            detail: None,
            parent: None,
        };
        frame.push(action_record(&a)?);
        frame.push(self.ledger(
            LedgerKind::ActionPlanned,
            Some(&a.session_id),
            json!({"execution_id": a.execution_id, "correlation_id": a.correlation_id, "tool": a.tool, "args_digest": a.args_digest, "retry_class": a.retry_class, "deadline_at_ms": a.deadline_at_ms, "reserved_usd": micros_to_usd(reserve_micros)}),
        )?);
        if let Err(err) = self.commit(&frame) {
            if let (None, Some(r)) = (self.tx(), &a.reservation_id) {
                self.ceiling.release(r);
            }
            return Err(err);
        }
        Ok(a)
    }

    /// Every action waiting for the operator's answer (`Action::awaits_confirm`):
    /// the one derivation of pending confirms (theseus-0g4). A budget question
    /// comes first, then the rest in the order they were planned.
    pub fn pending_confirms(&self) -> Result<Vec<Action>> {
        let mut v: Vec<Action> = self
            .actions_by(&[terms::one(&terms::action_state(ActionState::Planned))])?
            .into_iter()
            .filter(Action::awaits_confirm)
            .collect();
        v.sort_by_key(|a| {
            (
                a.tool != BUDGET_TOOL,
                a.planned_at_ms,
                a.correlation_id.clone(),
            )
        });
        Ok(v)
    }

    /// An open execution whose limit is the config's and differs from it.
    fn follows_limit(&self, e: &Execution) -> bool {
        !e.state.is_terminal()
            && !e.budget.pinned
            && e.budget.limit_micros != self.cfg.spend_limit_micros
    }

    /// Give `e`, read under its lock, the config's spend limit, if it follows
    /// it: the records beside `e`'s own for the frame (a withdrawn question,
    /// and the rows), and what changed. A raise withdraws the budget question
    /// the execution waits on, or has open, and puts it with the results the
    /// next turn consumes, as an approved reset does, so the call that did
    /// not fit proceeds (and asks again, in the new figures, if it still does
    /// not fit); a waiting execution is queued for the driver.
    fn follow_limit(
        &self,
        e: &mut Execution,
        now: u64,
    ) -> Result<Option<(LimitFollowed, Vec<NewRecord>)>> {
        if !self.follows_limit(e) {
            return Ok(None);
        }
        self.limit_to(e, self.cfg.spend_limit_micros, now, "config")
            .map(Some)
    }

    /// Give `e`, read under its lock, the limit `to`, as `follow_limit` says;
    /// `why` names what chose it: the `config`, or a `place`'s ceiling
    /// (`place_limit.rs`).
    pub(crate) fn limit_to(
        &self,
        e: &mut Execution,
        to: Micros,
        now: u64,
        why: &str,
    ) -> Result<(LimitFollowed, Vec<NewRecord>)> {
        let from = e.budget.limit_micros;
        e.budget.limit_micros = to;
        e.updated_at_ms = now;
        let mut records = Vec::new();
        let mut withdrew = None;
        let mut proceeds = false;
        // The question open, or the one a budget wait names (answered no).
        // A lower limit leaves it as it is.
        let asked = e.budget.question.clone().or_else(|| match &e.wake {
            Some(Wake::Budget { correlation_id }) => Some(correlation_id.clone()),
            _ => None,
        });
        if let Some(qid) = asked.filter(|_| to > from) {
            if let Some(mut q) = self.action(&qid)? {
                if q.state == ActionState::Planned {
                    q.state = ActionState::Cancelled;
                    q.settled_at_ms = Some(now);
                    q.resolution = Some(format!(
                        "withdrawn: the spend limit was raised from {} to {}",
                        usd(from),
                        usd(to)
                    ));
                    records.push(action_record(&q)?);
                    withdrew = Some(qid.clone());
                }
                if e.budget.question.as_deref() == Some(qid.as_str()) {
                    e.budget.question = None;
                    e.budget.question_needs_micros = 0;
                }
                if !e.queued_results.contains(&qid) {
                    e.queued_results.push(qid);
                }
                proceeds = true;
            }
        }
        let woke = proceeds && e.state == ExecState::Waiting;
        if woke {
            e.state = ExecState::Queued;
            e.wake = None;
            e.resume_pending = true;
        }
        let b = &e.budget;
        records.push(self.ledger(
            LedgerKind::BudgetLimitChanged,
            Some(&e.session_id),
            json!({
                "execution_id": e.id,
                "from_usd": micros_to_usd(from),
                "to_usd": micros_to_usd(to),
                "spent_usd": micros_to_usd(b.spent_micros),
                "reserved_usd": micros_to_usd(b.reserved_micros),
                "held_unknown_usd": micros_to_usd(b.held_unknown_micros),
                "available_usd": micros_to_usd(b.available()),
                "state": e.state,
                "withdrew": withdrew,
                "proceeds": proceeds,
                "why": why,
            }),
        )?);
        if woke {
            records.push(self.ledger(
                LedgerKind::ExecutionQueued,
                Some(&e.session_id),
                json!({"execution_id": e.id, "why": "limit_raised"}),
            )?);
        }
        Ok((
            LimitFollowed {
                execution_id: e.id.clone(),
                session_id: e.session_id.clone(),
                from_micros: from,
                to_micros: to,
                withdrew,
                proceeds,
            },
            records,
        ))
    }

    // ------------------------------------------------------------ cancel

    // ------------------------------------------------------------ reconcile

    // ------------------------------------------------------------ startup

    /// The five-step startup (§3.3, Part II M2). Each step is idempotent, so a
    /// crash inside any of them is recovered by running startup again:
    /// 1. store opened and recovered (done by the caller; recorded here),
    /// 2. executions found `Running` were mid-turn when the process died:
    ///    requeue them as interrupted,
    /// 3. drain the completion spool (each file settles, then is removed; a
    ///    rename a crash cut short is finished first, and a completion whose
    ///    action is settled already writes nothing),
    /// 4. reconcile against evidence,
    /// 5. accept events.
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    pub fn startup(
        &self,
        spool: Option<&crate::spool::Spool>,
        evidence: &dyn Evidence,
    ) -> Result<StartupReport> {
        let t0 = std::time::Instant::now();
        let mut rep = StartupReport::default();
        let fault = self.cfg.fault_after_startup_step;
        // Each step's `startup.step` row waits for the last step and goes in
        // its frame: a clean start pays one fsync for its record, not five
        // (theseus-qa0; about 7 ms each on this WSL disk). A step that changes
        // state still commits that change in its own frame, as it happens.
        let mut step_rows: Vec<NewRecord> = Vec::with_capacity(5);
        let mut step = |n: u8, name: &'static str, t: std::time::Instant| -> Result<()> {
            rep.steps.push(StartupStep {
                step: n,
                name: name.to_string(),
                elapsed_us: t.elapsed().as_micros() as u64,
            });
            if fault == Some(n) {
                anyhow::bail!("injected crash after startup step {n} ({name})");
            }
            Ok(())
        };

        // 1. store
        let t = std::time::Instant::now();
        *self.phase.lock().unwrap() = 1;
        self.started_at_ms
            .store(self.now_ms(), std::sync::atomic::Ordering::Relaxed);
        let st = self.store.stats()?;
        step_rows.push(self.ledger(
            LedgerKind::StartupStep,
            None,
            json!({"step": 1, "name": "store", "last_position": st.last_position, "truncated_bytes": st.truncated_bytes, "replayed_into_index": st.replayed_into_index}),
        )?);
        step(1, "store", t)?;

        // 2. load executions; requeue interrupted turns; reopen those a unit
        //    budget ended under their dollar limit (theseus-3ebd); and give
        //    every open execution that follows the config a changed spend limit
        //    (theseus-3pj). The rewrites share one frame: the first start under this
        //    binary, or under a changed limit, pays one fsync for them, and
        //    every later start finds none. It reads only those executions,
        //    by their terms (theseus-lv2): a turn running, one a unit budget
        //    ended (dollars end none), a limit other than the config's. A
        //    parked one is read by none.
        let t = std::time::Instant::now();
        *self.phase.lock().unwrap() = 2;
        let now = self.now_ms();
        let mut migrated = Vec::new();
        let mut wanted = vec![
            terms::one(&terms::state(ExecState::Running)),
            terms::one(&terms::state(ExecState::BudgetExhausted)),
        ];
        wanted.extend(terms::limits_other_than(self.cfg.spend_limit_micros));
        let all = self.executions_by(&wanted)?;
        // The executions this step rewrites are read again under their locks,
        // taken together in id order and held to the step's end, so the scan
        // decides nothing it writes. A clean start reads and rewrites none.
        let rewrites =
            |e: &Execution| e.state == ExecState::Running || reopens(e) || self.follows_limit(e);
        let ids: Vec<&str> = all
            .iter()
            .filter(|e| rewrites(e))
            .map(|e| e.id.as_str())
            .collect();
        let rewriting = self.lock(&ids);
        for e in all {
            let mut e = if rewrites(&e) {
                self.execution(&e.id)?.unwrap_or(e)
            } else {
                e
            };
            let mut rows = Vec::new();
            self.reopen(&mut e, now, &mut rows, &mut rep.reopened)?;
            let interrupted = e.state == ExecState::Running;
            if interrupted {
                e.interrupted += 1;
                e.updated_at_ms = now;
                // A turn a stop had already stopped (W1) is not resumed: its
                // execution waits on its next input, as the turn's end would
                // have left it.
                let stopped = e.stopped.take();
                if stopped.is_some() {
                    e.state = ExecState::Waiting;
                    e.wake = Some(Wake::Input);
                    e.resume_pending = false;
                } else {
                    e.state = ExecState::Queued;
                    e.resume_pending = true;
                }
                rows.insert(
                    0,
                    self.ledger(
                        LedgerKind::ExecutionInterrupted,
                        Some(&e.session_id),
                        json!({"execution_id": e.id, "interrupted": e.interrupted, "turn": e.turns,
                               "stopped_by": stopped.map(|s| s.by)}),
                    )?,
                );
            }
            if let Some((f, records)) = self.follow_limit(&mut e, now)? {
                rows.extend(records);
                rep.limits_followed.push(f);
            }
            if interrupted {
                let mut frame = vec![exec_record(&e)?];
                frame.extend(rows);
                self.commit(&frame)?;
                if e.state == ExecState::Queued {
                    rep.requeued_interrupted.push(e.id.clone());
                }
            } else if !rows.is_empty() {
                migrated.push(exec_record(&e)?);
                migrated.extend(rows);
            }
        }
        if !migrated.is_empty() {
            self.commit(&migrated)?;
        }
        drop(rewriting);
        step_rows.push(self.ledger(
            LedgerKind::StartupStep,
            None,
            json!({"step": 2, "name": "load", "requeued": rep.requeued_interrupted, "budgets_reopened": rep.reopened, "limits_followed": rep.limits_followed.len()}),
        )?);
        step(2, "load", t)?;

        // 3. drain spool
        let t = std::time::Instant::now();
        *self.phase.lock().unwrap() = 3;
        if let Some(sp) = spool {
            // A wrapper's completion is durable under its tmp name once synced;
            // its rename may be lost to a crash, so the drain finishes it
            // (theseus-yxiv). Each completion is taken: one whose action the
            // WAL settled before the crash (its file's removal, or its rename,
            // lost) writes nothing.
            let drained = sp.drain_recovering()?;
            for (path, c) in drained.completions {
                self.take_completion_with(&c, vec![])?;
                sp.remove(&path)?; // after the frame: a crash here redelivers, which is a no-op
                rep.spool_drained += 1;
            }
            rep.spool_quarantined = drained.malformed as u32;
            rep.spool_recovered = drained.recovered as u32;
        }
        step_rows.push(self.ledger(
            LedgerKind::StartupStep,
            None,
            json!({"step": 3, "name": "spool", "drained": rep.spool_drained, "malformed": rep.spool_quarantined, "recovered": rep.spool_recovered}),
        )?);
        step(3, "spool", t)?;

        // 4. reconcile
        let t = std::time::Instant::now();
        *self.phase.lock().unwrap() = 4;
        // Due wakes are left to the driver's first tick and the heartbeat
        // (DD8): queueing one here would write before the socket serves, and
        // its turn waits for the vault's word on the config anyway. So this
        // pass reads the actions sent, and counts the open executions.
        rep.reconcile = self.reconcile_with(evidence, false)?;
        step_rows.push(self.ledger(
            LedgerKind::StartupStep,
            None,
            json!({"step": 4, "name": "reconcile", "report": rep.reconcile}),
        )?);
        step(4, "reconcile", t)?;

        // 5. accept
        let t = std::time::Instant::now();
        *self.phase.lock().unwrap() = 5;
        step_rows.push(self.ledger(
            LedgerKind::StartupStep,
            None,
            json!({"step": 5, "name": "accepting"}),
        )?);
        self.commit(&step_rows)?;
        step(5, "accepting", t)?;
        rep.elapsed_us = t0.elapsed().as_micros() as u64;
        Ok(rep)
    }
}

/// Release a reservation into what the call really cost, which is booked in
/// full even past the reservation (a real cost is never hidden), or, with no
/// cost known, into `held_unknown_micros`.
pub(crate) fn settle_reservation_in(b: &mut Budget, reservation_id: &str, actual: Option<Micros>) {
    let Some(reserved) = b.reservations.remove(reservation_id) else {
        return;
    };
    b.reserved_micros = b.reserved_micros.saturating_sub(reserved);
    match actual {
        Some(cost) => b.spent_micros = b.spent_micros.saturating_add(cost),
        None => b.held_unknown_micros = b.held_unknown_micros.saturating_add(reserved),
    }
}

pub(crate) fn hold_reservation_in(b: &mut Budget, reservation_id: &str) {
    settle_reservation_in(b, reservation_id, None)
}

fn decode_opt<T: serde::de::DeserializeOwned>(r: Option<Record>) -> Result<Option<T>> {
    match r {
        Some(r) => Ok(Some(r.decode()?)),
        None => Ok(None),
    }
}
fn decode_all<T: serde::de::DeserializeOwned>(rs: &[Record]) -> Result<Vec<T>> {
    rs.iter().map(|r| r.decode()).collect()
}

pub(crate) fn exec_record(e: &Execution) -> Result<NewRecord> {
    Ok(NewRecord::json(kinds::EXECUTION, Some(&e.id), e)?.scoped(&e.session_id))
}
pub(crate) fn action_record(a: &Action) -> Result<NewRecord> {
    Ok(NewRecord::json(kinds::ACTION, Some(&a.correlation_id), a)?.scoped(&a.session_id))
}
