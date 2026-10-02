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
use crate::gate::{digest_proposal, Proposal};
use crate::locks::{ExecLock, ExecLocks};
use crate::terms;
use crate::tx::{Staged, Tx};
use crate::types::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelConfig {
    /// How many executions may hold a turn at once (§3.2a admission scheduler).
    pub admission_ceiling: u32,
    /// Deadline for an action whose tool declares none.
    pub default_deadline_ms: u64,
    /// The spend limit, in micro-dollars, of a new execution whose caller
    /// names none, and of an execution stored with a unit budget. Every open
    /// execution whose limit is the config's follows it once this config may
    /// act (theseus-3pj; `Kernel::follow_spend_limit`).
    pub spend_limit_micros: Micros,
    /// How long a confirmation stays valid.
    pub confirm_ttl_ms: u64,
    /// Reconciler cadence (the heartbeat, §3.3).
    pub heartbeat_ms: u64,
    /// Test hook: startup returns an error after this step, as if the process
    /// died there. Never set outside the simulator.
    #[serde(skip)]
    pub fault_after_startup_step: Option<u8>,
    /// The config this kernel runs under came from a copy the vault has not
    /// confirmed yet (theseus-2fo). Startup then writes nothing that config
    /// decides: an execution stored with a unit budget takes its dollar
    /// limit from `spend_limit_micros` when startup rewrites it, so a store
    /// that still holds one refuses to start (`KernelError::UnconfirmedConfig`);
    /// and the open executions follow the config's limit only once the vault
    /// confirms it (`Kernel::follow_spend_limit`), not in startup.
    #[serde(skip)]
    pub unconfirmed_config: bool,
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            admission_ceiling: 8,
            default_deadline_ms: 10 * 60 * 1000,
            spend_limit_micros: 100 * MICROS_PER_USD,
            confirm_ttl_ms: 15 * 60 * 1000,
            heartbeat_ms: 60_000,
            fault_after_startup_step: None,
            unconfirmed_config: false,
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
    /// Startup found executions stored with unit budgets under a config the
    /// vault has not confirmed; nothing was written (theseus-2fo).
    #[error(
        "{executions} execution(s) still have unit budgets (from before theseus-0sg), and their \
         dollar limit comes from the config, which the vault has not confirmed; nothing was written"
    )]
    UnconfirmedConfig { executions: usize },
    /// A task cannot open tasks (DD7: depth one).
    #[error("execution {id} is a task, and a task cannot start tasks (depth one)")]
    TaskDepth { id: ExecutionId },
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
    pub open_actions: u64,
    pub open_executions: u64,
    pub elapsed_us: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StartupReport {
    pub steps: Vec<StartupStep>,
    pub requeued_interrupted: Vec<ExecutionId>,
    /// Open executions that took a changed spend limit in step 2 (a config
    /// that may act at once; theseus-3pj).
    #[serde(default)]
    pub limits_followed: Vec<LimitFollowed>,
    pub spool_drained: u32,
    pub spool_quarantined: u32,
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
    pub actions_by_state: BTreeMap<String, u64>,
    pub turns_held: u32,
    pub admission_ceiling: u32,
    pub quarantined_completions: u64,
    pub accepting: bool,
}

/// A session's recorded spend in micro-dollars, by session id: what an
/// execution stored with a unit budget had spent in dollars (theseus-0sg).
pub type LegacySpend = Arc<dyn Fn(&str) -> Micros + Send + Sync>;

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
    legacy_spend: Option<LegacySpend>,
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
        Self {
            store,
            clock,
            cfg,
            held: Arc::new(Mutex::new(HashSet::new())),
            locks: Arc::default(),
            phase: Arc::new(Mutex::new(0)),
            legacy_spend: None,
            started_at_ms: Arc::default(),
            own: None,
            observer: Arc::default(),
            tx: None,
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
            legacy_spend: self.legacy_spend.clone(),
            started_at_ms: self.started_at_ms.clone(),
            own: None,
            observer: self.observer.clone(),
            tx: None,
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
            legacy_spend: self.legacy_spend.clone(),
            started_at_ms: self.started_at_ms.clone(),
            own: self.own.clone(),
            observer: Arc::default(),
            tx: Some(tx),
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

    /// How many of its turn's own results this view has settled: a turn that
    /// faults after one did is woken, so its continuation reads them.
    pub fn own_settled(&self) -> u32 {
        self.own
            .as_ref()
            .map_or(0, |o| o.settled.load(std::sync::atomic::Ordering::SeqCst))
    }

    /// Where startup finds the dollar spend of an execution stored with a
    /// unit budget: the session's own record, which the core keeps. Without
    /// one, such an execution starts again from $0.
    pub fn with_legacy_spend(mut self, f: LegacySpend) -> Self {
        self.legacy_spend = Some(f);
        self
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
    /// task's action locks the parent too (`lock_family`): what it settles
    /// is the parent's spend as well (DD7).
    fn locked_action(&self, correlation_id: &str) -> Result<Option<(ExecLock<'_>, Action)>> {
        let Some(a) = self.action(correlation_id)? else {
            return Ok(None);
        };
        let lock = self.lock_family(&a.execution_id)?;
        let a = self
            .action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?;
        Ok(Some((lock, a)))
    }

    /// Lock an execution for a transition that may reach its parent: a task
    /// (DD7) and its parent together, in id order, since a task's spend
    /// and its end are written into the parent in the same frame; any other
    /// execution alone. The parent is read outside the lock, which is sound
    /// because an execution's parent never changes.
    pub(crate) fn lock_family(&self, execution_id: &str) -> Result<ExecLock<'_>> {
        let parent = self.execution(execution_id)?.and_then(|e| e.parent);
        Ok(match parent {
            Some(p) => self.lock(&[execution_id, &p]),
            None => self.lock(&[execution_id]),
        })
    }

    /// A task's change reaches its parent (DD7): under both locks, which the
    /// caller holds (`lock_family`), the parent is read, `carry` applies, and
    /// the parent's record joins `frame` when it changed. `spent_before` is
    /// the task's spend before the change. Nothing for an execution with no
    /// parent.
    pub(crate) fn carry_to_parent(
        &self,
        child: &Execution,
        spent_before: Micros,
        frame: &mut Vec<NewRecord>,
    ) -> Result<Option<Execution>> {
        let Some(pid) = &child.parent else {
            return Ok(None);
        };
        let Some(mut parent) = self.execution(pid)? else {
            return Ok(None);
        };
        if crate::tasks::carry(&mut parent, child, spent_before) {
            parent.updated_at_ms = self.now_ms();
            frame.push(exec_record(&parent)?);
            return Ok(Some(parent));
        }
        Ok(None)
    }

    /// `locked_action`, for a transition that needs the action to exist.
    fn locked_known_action(&self, correlation_id: &str) -> Result<(ExecLock<'_>, Action)> {
        self.locked_action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()).into())
    }

    #[cfg(test)]
    pub(crate) fn exec_locks(&self) -> &ExecLocks {
        &self.locks
    }
    /// Every execution: O(all), for the listings that show them all.
    pub fn executions(&self) -> Result<Vec<Execution>> {
        self.store
            .latest_of_kind(kinds::EXECUTION)?
            .iter()
            .map(|r| self.read_execution(r))
            .collect()
    }
    /// Every execution that has not ended, in id order: parked
    /// conversations too. A reader that needs less asks by its own terms
    /// (`maybe_runnable`, `executions_by`).
    pub fn open_executions(&self) -> Result<Vec<Execution>> {
        self.executions_by(&OPEN_EXECUTIONS.map(|s| terms::one(&terms::state(s))))
    }
    /// What the driver's tick and the reconcile look at (theseus-lv2): the
    /// queued executions, and the waiting ones a due time or a task's report
    /// may wake (`wakes::due_now`'s cases), in id order. Never a parked one.
    pub fn maybe_runnable(&self) -> Result<Vec<Execution>> {
        self.executions_by(&[
            terms::one(&terms::state(ExecState::Queued)),
            terms::one("due"),
        ])
    }
    /// The executions with a term in any of `ranges` (`kernel::terms`), in
    /// id order (theseus-lv2).
    pub fn executions_by(&self, ranges: &[(String, String)]) -> Result<Vec<Execution>> {
        self.records_by(kinds::EXECUTION, ranges)?
            .iter()
            .map(|r| self.read_execution(r))
            .collect()
    }
    pub fn actions(&self) -> Result<Vec<Action>> {
        decode_all(&self.store.latest_of_kind(kinds::ACTION)?)
    }
    /// Every action not settled, in id order: O(open), from the store's
    /// terms (theseus-lv2).
    pub fn open_actions(&self) -> Result<Vec<Action>> {
        self.actions_by(&OPEN_ACTIONS.map(|s| terms::one(&terms::action_state(s))))
    }
    /// The actions with a term in any of `ranges`, in id order.
    pub fn actions_by(&self, ranges: &[(String, String)]) -> Result<Vec<Action>> {
        decode_all(&self.records_by(kinds::ACTION, ranges)?)
    }
    /// An execution's actions that are not settled (theseus-2qt), in id
    /// order: its own term, never a read of every action.
    pub fn unsettled_actions(&self, execution_id: &str) -> Result<Vec<Action>> {
        Ok(self
            .actions_by(&[terms::one(&terms::unsettled_of(execution_id))])?
            .into_iter()
            .filter(|a| a.execution_id == execution_id && !a.state.is_settled())
            .collect())
    }

    /// The latest records of `kind` with a term in any of `ranges`, each
    /// once, in key order (theseus-lv2): from the store's terms, or, from a
    /// store that keeps none, every record of the kind, by the same terms.
    fn records_by(
        &self,
        kind: theseus_store::RecordKind,
        ranges: &[(String, String)],
    ) -> Result<Vec<Record>> {
        let mut by_key: BTreeMap<String, Record> = BTreeMap::new();
        for (lo, hi) in ranges {
            let Some(found) = self.store.latest_by_terms(kind, lo, hi)? else {
                return Ok(self
                    .store
                    .latest_of_kind(kind)?
                    .into_iter()
                    .filter(|r| terms::any_in(&terms::of(kind, &r.payload), ranges))
                    .collect());
            };
            for r in found {
                if let Some(k) = r.key.clone() {
                    by_key.insert(k, r);
                }
            }
        }
        Ok(by_key.into_values().collect())
    }

    /// How many latest records of `kind` have each of `wanted`'s terms
    /// (theseus-lv2): counted in the store's terms, or, from a store that
    /// keeps none, over every record of the kind, once.
    fn counts_by(&self, kind: theseus_store::RecordKind, wanted: &[String]) -> Result<Vec<u64>> {
        let mut out = Vec::with_capacity(wanted.len());
        for t in wanted {
            let (lo, hi) = terms::one(t);
            match self.store.count_by_terms(kind, &lo, &hi)? {
                Some(n) => out.push(n),
                None => {
                    let all: Vec<Vec<String>> = self
                        .store
                        .latest_of_kind(kind)?
                        .iter()
                        .map(|r| terms::of(kind, &r.payload))
                        .collect();
                    return Ok(wanted
                        .iter()
                        .map(|t| all.iter().filter(|ts| ts.contains(t)).count() as u64)
                        .collect());
                }
            }
        }
        Ok(out)
    }

    /// How many executions have not ended, counted by state.
    pub fn count_open_executions(&self) -> Result<u64> {
        let wanted = OPEN_EXECUTIONS.map(terms::state);
        Ok(self.counts_by(kinds::EXECUTION, &wanted)?.iter().sum())
    }

    /// How many actions are not settled, counted by state.
    pub fn count_open_actions(&self) -> Result<u64> {
        let wanted = OPEN_ACTIONS.map(terms::action_state);
        Ok(self.counts_by(kinds::ACTION, &wanted)?.iter().sum())
    }

    /// Completions that matched no action, in key order.
    pub fn quarantined(&self) -> Result<Vec<Completion>> {
        decode_all(
            &self
                .store
                .latest_with_prefix(kinds::COMPLETION, QUARANTINE_PREFIX)?,
        )
    }

    /// The kernel's counts, for health: each from the store's terms, so a
    /// health answer reads no execution and no action (theseus-lv2).
    pub fn stats(&self) -> Result<KernelStats> {
        let mut s = KernelStats {
            turns_held: self.held.lock().unwrap().len() as u32,
            admission_ceiling: self.cfg.admission_ceiling,
            accepting: self.is_accepting(),
            ..Default::default()
        };
        let wanted = EXEC_STATES.map(terms::state);
        for (st, n) in EXEC_STATES
            .iter()
            .zip(self.counts_by(kinds::EXECUTION, &wanted)?)
        {
            if n > 0 {
                s.executions_by_state.insert(st.as_str().to_string(), n);
            }
        }
        let wanted = ACTION_STATES.map(terms::action_state);
        for (st, n) in ACTION_STATES
            .iter()
            .zip(self.counts_by(kinds::ACTION, &wanted)?)
        {
            if n > 0 {
                s.actions_by_state.insert(st.as_str().to_string(), n);
            }
        }
        s.quarantined_completions = self
            .store
            .latest_with_prefix(kinds::COMPLETION, QUARANTINE_PREFIX)?
            .len() as u64;
        Ok(s)
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
    /// (theseus-in3). Every EXECUTION and ACTION record is written here.
    pub(crate) fn commit(&self, frame: &[NewRecord]) -> Result<Vec<u64>> {
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

    /// Every execution with the WAL position of its record (theseus-in3):
    /// the push's seed reads these first, then the actions.
    pub fn executions_at(&self) -> Result<Vec<(u64, Execution)>> {
        self.store
            .latest_of_kind(kinds::EXECUTION)?
            .iter()
            .map(|r| Ok((r.position, self.read_execution(r)?)))
            .collect()
    }

    /// Every action with the WAL position of its record (theseus-in3).
    pub fn actions_at(&self) -> Result<Vec<(u64, Action)>> {
        self.store
            .latest_of_kind(kinds::ACTION)?
            .iter()
            .map(|r| Ok((r.position, r.decode()?)))
            .collect()
    }

    /// An execution as stored, decoded as `executions` decodes it (a unit
    /// budget read in dollars): what the push's observer hands on.
    pub fn decode_execution(&self, payload: &[u8]) -> Result<Execution> {
        Execution::from_stored(payload, self.cfg.spend_limit_micros)
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
            stopped: None,
            turns: 0,
            interrupted: 0,
            resume_pending: false,
            cancel: None,
            ended_reason: None,
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
        if let Some(mut e) = self.execution(&a.execution_id)? {
            let spent_before = e.budget.spent_micros;
            if let Some(r) = &a.reservation_id {
                settle_reservation_in(&mut e.budget, r, Some(0));
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
        Ok(a)
    }

    /// Take a turn: the execution must be `Queued`, the ceiling must have
    /// room, and no turn may be held on it in this process. Writes `Running`.
    /// The guard frees the turn when dropped, so a caller whose frame fails
    /// lets it go.
    pub fn admit(&self, execution_id: &str) -> Result<TurnGuard> {
        self.require_accepting()?;
        let _w = self.lock(&[execution_id]);
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        if e.state != ExecState::Queued {
            return Err(KernelError::NotRunnable {
                id: e.id.clone(),
                state: e.state.as_str(),
            }
            .into());
        }
        {
            let mut held = self.held.lock().unwrap();
            if held.contains(&e.id) {
                return Err(KernelError::TurnHeld { id: e.id.clone() }.into());
            }
            if held.len() as u32 >= self.cfg.admission_ceiling {
                return Err(KernelError::AdmissionFull {
                    ceiling: self.cfg.admission_ceiling,
                }
                .into());
            }
            held.insert(e.id.clone());
        }
        let now = self.now_ms();
        let guard = TurnGuard {
            execution_id: e.id.clone(),
            turn: e.turns + 1,
            started_at_ms: now,
            held: self.held.clone(),
        };
        e.state = ExecState::Running;
        e.turns += 1;
        e.wake = None;
        let resumed = std::mem::take(&mut e.resume_pending);
        e.updated_at_ms = now;
        // A frame that fails drops the guard, which frees the turn.
        self.commit(&[
            exec_record(&e)?,
            self.ledger(
                LedgerKind::ExecutionRunning,
                Some(&e.session_id),
                json!({"execution_id": e.id, "turn": e.turns, "queued_results": e.queued_results.len(), "resumed": resumed}),
            )?,
        ])?;
        Ok(guard)
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
                    let reported = !e.report_wakes.is_empty();
                    if crate::wakes::free(&e) && (crate::wakes::wake_due(&e, now) || reported) {
                        e.state = ExecState::Queued;
                        e.wake = None;
                        e.resume_pending = true;
                        why = Some(if crate::wakes::wake_due(&e, now) {
                            "wake"
                        } else {
                            "report"
                        });
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
        if e.state.is_terminal() {
            let why = format!("the execution ended ({})", e.state.as_str());
            self.end_unsent(&mut e, "the harness", &why, &mut frame)?;
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
            k.authorize(&a.correlation_id, proposal, None)?;
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
            if reserve_micros > available {
                return Err(KernelError::OverBudget {
                    needed: reserve_micros,
                    available,
                    spent: e.budget.spent_micros,
                    limit: e.budget.limit_micros,
                }
                .into());
            }
            let id = new_id("rsv");
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
            reservation_id,
            reserved_micros: reserve_micros,
            resolution: None,
            completions_seen: 0,
            detail: None,
        };
        frame.push(action_record(&a)?);
        frame.push(self.ledger(
            LedgerKind::ActionPlanned,
            Some(&a.session_id),
            json!({"execution_id": a.execution_id, "correlation_id": a.correlation_id, "tool": a.tool, "args_digest": a.args_digest, "retry_class": a.retry_class, "deadline_at_ms": a.deadline_at_ms, "reserved_usd": micros_to_usd(reserve_micros)}),
        )?);
        self.commit(&frame)?;
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

    // ------------------------------------------------------------ budget

    /// A reservation did not fit (`OverBudget`): ask the operator whether
    /// the execution's spend may go back to $0 (theseus-0sg). Requires the
    /// held turn. The question is a planned `budget.reset` action with no
    /// reservation that keeps its proposal, like every action that waits for
    /// the operator, answered through the confirm path: `reset_budget` on an
    /// approval, `decline_action` otherwise. The turn then ends waiting on
    /// `Wake::Budget`. An earlier question still open is superseded in the
    /// same frame, so one is open at a time.
    pub fn ask_budget(&self, guard: &TurnGuard, needed_micros: Micros) -> Result<Action> {
        self.ask_budget_for(guard, needed_micros, Value::Null)
    }

    /// `ask_budget`, with what the core knows of the call that did not fit
    /// (`call`: its profile, model, and output cap) kept in the question's
    /// proposal as `args.call`, so every surface that renders the question
    /// later can name them (theseus-kks). `Null` keeps the proposal as
    /// `ask_budget` writes it. The `budget.asked` row says `exceeds_limit`
    /// when the call alone needs more than the whole limit: no reset can
    /// make it fit.
    pub fn ask_budget_for(
        &self,
        guard: &TurnGuard,
        needed_micros: Micros,
        call: Value,
    ) -> Result<Action> {
        let _w = self.lock(&[&guard.execution_id]);
        let mut e = self
            .execution(&guard.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(guard.execution_id.clone()))?;
        require_turn(&e)?;
        let now = self.now_ms();
        let mut frame = Vec::new();
        if let Some(old) = e.budget.question.take() {
            if let Some(mut q) = self.action(&old)? {
                if q.state == ActionState::Planned {
                    q.state = ActionState::Cancelled;
                    q.settled_at_ms = Some(now);
                    q.resolution = Some("superseded by a newer budget question".into());
                    frame.push(action_record(&q)?);
                    frame.push(self.ledger(
                        LedgerKind::ActionDeclined,
                        Some(&q.session_id),
                        json!({"correlation_id": q.correlation_id, "tool": q.tool, "by": "harness", "reason": "superseded by a newer budget question"}),
                    )?);
                }
            }
        }
        let b = &e.budget;
        let mut args = json!({"spent_micros": b.spent_micros, "limit_micros": b.limit_micros, "needed_micros": needed_micros, "resets": b.resets});
        if !call.is_null() {
            args["call"] = call;
        }
        let proposal = Proposal {
            tool: BUDGET_TOOL.into(),
            args,
            resource: None,
            policy_context: json!({}),
        };
        let q = Action {
            correlation_id: new_id("act"),
            schema: SCHEMA,
            execution_id: e.id.clone(),
            session_id: e.session_id.clone(),
            tool: BUDGET_TOOL.into(),
            args_digest: digest_proposal(&proposal),
            proposal: Some(proposal),
            resource: None,
            retry_class: RetryClass::NonRepeatable,
            state: ActionState::Planned,
            deadline_at_ms: now + self.cfg.confirm_ttl_ms,
            planned_at_ms: now,
            authorized_at_ms: None,
            dispatched_at_ms: None,
            settled_at_ms: None,
            external_op_id: None,
            result_ref: None,
            confirm: None,
            cancel: None,
            reservation_id: None,
            reserved_micros: 0,
            resolution: None,
            completions_seen: 0,
            detail: None,
        };
        let asked = json!({
            "execution_id": e.id,
            "correlation_id": q.correlation_id,
            "spent_usd": micros_to_usd(b.spent_micros),
            "limit_usd": micros_to_usd(b.limit_micros),
            "needed_usd": micros_to_usd(needed_micros),
            "available_usd": micros_to_usd(b.available()),
            "resets": b.resets,
            "exceeds_limit": needed_micros > b.limit_micros,
        });
        e.budget.question = Some(q.correlation_id.clone());
        e.budget.question_needs_micros = needed_micros;
        e.updated_at_ms = now;
        frame.push(exec_record(&e)?);
        frame.push(action_record(&q)?);
        frame.push(self.ledger(
            LedgerKind::ActionPlanned,
            Some(&q.session_id),
            json!({"execution_id": q.execution_id, "correlation_id": q.correlation_id, "tool": q.tool, "args_digest": q.args_digest, "retry_class": q.retry_class, "deadline_at_ms": q.deadline_at_ms, "reserved_usd": 0.0}),
        )?);
        frame.push(self.ledger(LedgerKind::BudgetAsked, Some(&e.session_id), asked)?);
        self.commit(&frame)?;
        Ok(q)
    }

    /// The operator approved a budget question: the execution's spend goes
    /// back to $0 and the waiting call proceeds (theseus-0sg). One frame: the
    /// question settles `Succeeded`; `spent_micros` becomes zero while the
    /// reservations and held amounts stay (they are calls in flight, or not
    /// yet accounted for); `resets` counts one more; the question joins the
    /// results the next turn consumes; and a waiting execution is queued for
    /// the driver. `budget.reset` records who approved
    /// it, the spend before, and the limit. This is the only transition that
    /// lowers spend. Returns the execution and the spend before.
    pub fn reset_budget(&self, correlation_id: &str, by: &str) -> Result<(Execution, Micros)> {
        let (_w, mut q) = self.locked_known_action(correlation_id)?;
        if q.tool != BUDGET_TOOL || q.state != ActionState::Planned {
            return Err(KernelError::ActionState {
                correlation_id: q.correlation_id.clone(),
                state: q.state.as_str(),
                expected: "a planned budget question",
            }
            .into());
        }
        let mut e = self
            .execution(&q.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(q.execution_id.clone()))?;
        if e.state.is_terminal() {
            return Err(KernelError::NotRunnable {
                id: e.id.clone(),
                state: e.state.as_str(),
            }
            .into());
        }
        let now = self.now_ms();
        let before = e.budget.spent_micros;
        q.state = ActionState::Succeeded;
        q.settled_at_ms = Some(now);
        q.resolution = Some(format!(
            "approved by {by}: spend reset from {} to $0",
            usd(before)
        ));
        e.budget.spent_micros = 0;
        e.budget.resets += 1;
        e.budget.question = None;
        e.budget.question_needs_micros = 0;
        if !e.queued_results.contains(&q.correlation_id) {
            e.queued_results.push(q.correlation_id.clone());
        }
        // Approved means go on: a waiting execution is queued whatever it
        // waits on (after a crash between asking and parking, it may wait on
        // input with the question still open).
        let woke = e.state == ExecState::Waiting;
        if woke {
            e.state = ExecState::Queued;
            e.wake = None;
            e.resume_pending = true;
        }
        e.updated_at_ms = now;
        let mut frame = vec![
            action_record(&q)?,
            exec_record(&e)?,
            self.ledger(
                LedgerKind::BudgetReset,
                Some(&e.session_id),
                json!({
                    "execution_id": e.id,
                    "correlation_id": q.correlation_id,
                    "by": by,
                    "spent_before_usd": micros_to_usd(before),
                    "limit_usd": micros_to_usd(e.budget.limit_micros),
                    "reserved_usd": micros_to_usd(e.budget.reserved_micros),
                    "held_unknown_usd": micros_to_usd(e.budget.held_unknown_micros),
                    "resets": e.budget.resets,
                }),
            )?,
        ];
        if woke {
            frame.push(self.ledger(
                LedgerKind::ExecutionQueued,
                Some(&e.session_id),
                json!({"execution_id": e.id, "why": "budget_reset"}),
            )?);
        }
        self.commit(&frame)?;
        Ok((e, before))
    }

    /// Every open execution whose limit is the config's takes the config's
    /// spend limit, when it has changed (theseus-3pj): the transition the
    /// core runs when the vault confirms the copy a start served from.
    /// (A start whose config may act at once does this in startup's step 2.)
    /// The rewrites share one frame, and each is ledgered as
    /// `budget.limit_changed`. Spend, reservations, held amounts, and resets
    /// are untouched: a lower limit refuses the next reservation that does
    /// not fit, which asks as usual. A higher one lets a call that waited at
    /// the old limit proceed (`follow_limit`).
    pub fn follow_spend_limit(&self) -> Result<Vec<LimitFollowed>> {
        self.require_accepting()?;
        let ids: Vec<ExecutionId> = self
            .executions_by(&terms::limits_other_than(self.cfg.spend_limit_micros))?
            .into_iter()
            .filter(|e| self.follows_limit(e))
            .map(|e| e.id)
            .collect();
        if ids.is_empty() {
            return Ok(vec![]);
        }
        // Decided from the scan; each is read again under the locks.
        let refs: Vec<&str> = ids.iter().map(String::as_str).collect();
        let _w = self.lock(&refs);
        let now = self.now_ms();
        let mut frame = Vec::new();
        let mut followed = Vec::new();
        for id in &ids {
            let Some(mut e) = self.execution(id)? else {
                continue;
            };
            if let Some((f, records)) = self.follow_limit(&mut e, now)? {
                frame.push(exec_record(&e)?);
                frame.extend(records);
                followed.push(f);
            }
        }
        if !frame.is_empty() {
            self.commit(&frame)?;
        }
        Ok(followed)
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
        let (from, to) = (e.budget.limit_micros, self.cfg.spend_limit_micros);
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
            }),
        )?);
        if woke {
            records.push(self.ledger(
                LedgerKind::ExecutionQueued,
                Some(&e.session_id),
                json!({"execution_id": e.id, "why": "limit_raised"}),
            )?);
        }
        Ok(Some((
            LimitFollowed {
                execution_id: e.id.clone(),
                session_id: e.session_id.clone(),
                from_micros: from,
                to_micros: to,
                withdrew,
                proceeds,
            },
            records,
        )))
    }

    /// Bind a confirmation to the action's *current* digest (§3.9).
    pub fn bind_confirm(
        &self,
        correlation_id: &str,
        by: &str,
        proposal: &Proposal,
    ) -> Result<Action> {
        let (_w, mut a) = self.locked_known_action(correlation_id)?;
        if a.state != ActionState::Planned {
            return Err(KernelError::ActionState {
                correlation_id: a.correlation_id.clone(),
                state: a.state.as_str(),
                expected: "planned",
            }
            .into());
        }
        let d = digest_proposal(proposal);
        if d != a.args_digest {
            return Err(KernelError::ConfirmInvalidated {
                reason: "confirmation presented for different arguments than were planned".into(),
            }
            .into());
        }
        let now = self.now_ms();
        a.confirm = Some(ConfirmBinding {
            confirm_id: new_id("cfm"),
            by: by.into(),
            bound_digest: d,
            expires_at_ms: now + self.cfg.confirm_ttl_ms,
        });
        self.commit(&[
            action_record(&a)?,
            self.ledger(
                LedgerKind::ActionConfirmed,
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "by": by, "confirm": a.confirm}),
            )?,
        ])?;
        Ok(a)
    }

    /// `authorized`: the gate has passed (§3.17 ordering, see `gate.rs`). The
    /// final revalidation happens here: the proposal's digest must equal the
    /// planned digest, and if a confirmation is required it must be bound to
    /// that same digest and unexpired.
    pub fn authorize(
        &self,
        correlation_id: &str,
        proposal: &Proposal,
        confirm_required_from: Option<&str>,
    ) -> Result<Action> {
        self.authorize_or_invalid(correlation_id, proposal, confirm_required_from)?
    }

    /// `authorize`, with its two refusals apart, and nothing written by
    /// either: `Err` when the action's execution ended it (a cancel or a
    /// stop settled it) or it cannot be read; `Ok(Err(why))` when the
    /// proposal or its confirm no longer holds (`authorize_frame`'s checks).
    fn authorize_or_invalid(
        &self,
        correlation_id: &str,
        proposal: &Proposal,
        confirm_required_from: Option<&str>,
    ) -> Result<Result<Action>> {
        let (_w, mut a) = self.locked_known_action(correlation_id)?;
        if let Some(end) = self.ended_with_execution(&a)? {
            return Err(end);
        }
        let frame = match self.authorize_frame(&mut a, proposal, confirm_required_from) {
            Ok(frame) => frame,
            Err(why) => return Ok(Err(why)),
        };
        self.commit(&frame)?;
        Ok(Ok(a))
    }

    /// The refusal for a transition of `a`, which the end of its execution
    /// already settled: a cancel settles everything the execution planned
    /// and never sent (theseus-w98), and a stop declines it (W1). The turn
    /// that planned it hears what `dispatch` tells it when the cancel or the
    /// stop comes first: the execution is cancelled, or stopped. `None` for
    /// any other action.
    fn ended_with_execution(&self, a: &Action) -> Result<Option<anyhow::Error>> {
        if a.state != ActionState::Cancelled {
            return Ok(None);
        }
        let Some(e) = self.execution(&a.execution_id)? else {
            return Ok(None);
        };
        Ok(if e.state == ExecState::Cancelled {
            Some(
                KernelError::NotRunnable {
                    id: e.id,
                    state: "cancelled",
                }
                .into(),
            )
        } else {
            e.stopped
                .map(|s| KernelError::Stopped { id: e.id, by: s.by }.into())
        })
    }

    /// `authorize`'s checks on `a`, and its frame: the action authorized and
    /// its `action.authorized` row.
    fn authorize_frame(
        &self,
        a: &mut Action,
        proposal: &Proposal,
        confirm_required_from: Option<&str>,
    ) -> Result<Vec<NewRecord>> {
        if a.state != ActionState::Planned {
            return Err(KernelError::ActionState {
                correlation_id: a.correlation_id.clone(),
                state: a.state.as_str(),
                expected: "planned",
            }
            .into());
        }
        let d = digest_proposal(proposal);
        if d != a.args_digest {
            return Err(KernelError::ConfirmInvalidated {
                reason: format!("planned digest {} != final digest {}", a.args_digest, d),
            }
            .into());
        }
        if let Some(by) = confirm_required_from {
            match &a.confirm {
                None => return Err(KernelError::ConfirmRequired { by: by.into() }.into()),
                Some(c) if c.bound_digest != d => {
                    return Err(KernelError::ConfirmInvalidated {
                        reason: "bound digest differs from final digest".into(),
                    }
                    .into())
                }
                Some(c) if c.expires_at_ms < self.now_ms() => {
                    return Err(KernelError::ConfirmInvalidated {
                        reason: "confirmation expired".into(),
                    }
                    .into())
                }
                Some(c) if c.by != by => {
                    return Err(KernelError::ConfirmInvalidated {
                        reason: format!("confirmed by {} but {by} is required", c.by),
                    }
                    .into())
                }
                Some(_) => {}
            }
        }
        let now = self.now_ms();
        a.state = ActionState::Authorized;
        a.authorized_at_ms = Some(now);
        Ok(vec![
            action_record(a)?,
            self.ledger(
                LedgerKind::ActionAuthorized,
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "confirmed": a.confirm.is_some()}),
            )?,
        ])
    }

    /// `dispatched`: committed before the call is made (transactional
    /// outbox). The execution records the action as outstanding.
    pub fn dispatch(&self, correlation_id: &str, external_op_id: Option<&str>) -> Result<Action> {
        self.dispatch_or_refuse(correlation_id, external_op_id)?
    }

    /// `authorize` and `dispatch` in one frame, a transaction, for a call the
    /// operator confirmed (theseus-l6y). Each keeps its record and row, in
    /// that order, so the WAL reads as the two transitions did in two
    /// frames. `Ok(Err(why))`: the confirm no longer holds (`authorize`'s
    /// refusal), and nothing was written. A cancel or a stop that landed
    /// first is the error, as `dispatch` returns it. It settled the action
    /// itself, so nothing is written (theseus-w98), except for an action an
    /// older build's cancel left planned: that frame writes the authorization
    /// and the action's cancel.
    pub fn authorize_and_dispatch(
        &self,
        correlation_id: &str,
        proposal: &Proposal,
        confirm_required_from: Option<&str>,
        external_op_id: Option<&str>,
    ) -> Result<Result<Action>> {
        /// How the transaction ended. Each commits what it staged: nothing
        /// when the confirm no longer holds, and the action's cancel when a
        /// cancel or a stop landed first.
        enum Answered {
            Dispatched(Box<Action>),
            Invalid(anyhow::Error),
            Refused(anyhow::Error),
        }
        let execution_id = self
            .action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?
            .execution_id;
        let answered = self.frame(&[&execution_id], |k| {
            Ok(
                match k.authorize_or_invalid(correlation_id, proposal, confirm_required_from)? {
                    Err(why) => Answered::Invalid(why),
                    Ok(_) => match k.dispatch_or_refuse(correlation_id, external_op_id)? {
                        Ok(a) => Answered::Dispatched(Box::new(a)),
                        Err(refused) => Answered::Refused(refused),
                    },
                },
            )
        })?;
        match answered {
            Answered::Dispatched(a) => Ok(Ok(*a)),
            Answered::Invalid(why) => Ok(Err(why)),
            Answered::Refused(refused) => Err(refused),
        }
    }

    /// `dispatch`, with its refusal apart. `Ok(Err(why))`: a cancel or a stop
    /// (W1) that landed between plan and dispatch kept the action from
    /// leaving; it is cancelled instead, and that is written. `Err`: nothing
    /// is written. The frame: the action, the execution with the action
    /// outstanding, and the `action.dispatched` row.
    fn dispatch_or_refuse(
        &self,
        correlation_id: &str,
        external_op_id: Option<&str>,
    ) -> Result<Result<Action>> {
        let (_w, mut a) = self.locked_known_action(correlation_id)?;
        if let Some(end) = self.ended_with_execution(&a)? {
            return Err(end);
        }
        if a.state != ActionState::Authorized {
            return Err(KernelError::ActionState {
                correlation_id: a.correlation_id.clone(),
                state: a.state.as_str(),
                expected: "authorized",
            }
            .into());
        }
        let mut e = self
            .execution(&a.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(a.execution_id.clone()))?;
        if e.state == ExecState::Cancelled || e.stopped.is_some() {
            let stopped = e.stopped.as_ref().map(|s| s.by.clone());
            a.state = ActionState::Cancelled;
            a.cancel = Some(CancelState::TerminationVerified);
            a.settled_at_ms = Some(self.now_ms());
            let why = if stopped.is_some() {
                "execution stopped before dispatch"
            } else {
                "execution cancelled before dispatch"
            };
            self.commit(&[
                action_record(&a)?,
                self.ledger(
                    LedgerKind::ActionCancelled,
                    Some(&a.session_id),
                    json!({"correlation_id": a.correlation_id, "why": why}),
                )?,
            ])?;
            return Ok(Err(match stopped {
                Some(by) => KernelError::Stopped { id: e.id, by },
                None => KernelError::NotRunnable {
                    id: e.id,
                    state: "cancelled",
                },
            }
            .into()));
        }
        let now = self.now_ms();
        a.state = ActionState::Dispatched;
        a.dispatched_at_ms = Some(now);
        a.external_op_id = external_op_id.map(str::to_string);
        if !e.outstanding.contains(&a.correlation_id) {
            e.outstanding.push(a.correlation_id.clone());
        }
        e.updated_at_ms = now;
        self.commit(&[
            action_record(&a)?,
            exec_record(&e)?,
            self.ledger(
                LedgerKind::ActionDispatched,
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "execution_id": e.id, "tool": a.tool, "external_op_id": a.external_op_id, "deadline_at_ms": a.deadline_at_ms}),
            )?,
        ])?;
        Ok(Ok(a))
    }

    /// `accept_completion`, with `extra` records (the node that carries the
    /// result) in the same frame as the settlement, a transaction. `extra` is
    /// written only when this call settles or resolves the action; a
    /// duplicate, a late arrival after cancel, and a quarantined stray write
    /// nothing extra. A stray stays one inside the transaction: an action's
    /// id is minted at its plan, before anything can complete it.
    pub fn accept_completion_with(
        &self,
        c: &Completion,
        extra: Vec<NewRecord>,
    ) -> Result<Accepted> {
        let execution = self.action(&c.correlation_id)?.map(|a| a.execution_id);
        let ids: Vec<&str> = execution.iter().map(String::as_str).collect();
        self.frame(&ids, |k| {
            let accepted = k.accept_completion(c)?;
            if matches!(
                accepted,
                Accepted::Settled { .. } | Accepted::ResolvedUnknown { .. }
            ) {
                k.stage(&extra)?;
            }
            Ok(accepted)
        })
    }

    /// Accept a completion from any transport (§3.16). Idempotent; atomic
    /// with the owning execution's continuation. A task's action locks the
    /// parent too (`locked_action`).
    pub fn accept_completion(&self, c: &Completion) -> Result<Accepted> {
        let Some((_w, mut a)) = self.locked_action(&c.correlation_id)? else {
            // No action, so no execution to lock.
            let key = format!("{QUARANTINE_PREFIX}{}", c.correlation_id);
            self.commit(&[
                NewRecord::json(kinds::COMPLETION, Some(&key), c)?,
                self.ledger(
                    LedgerKind::CompletionQuarantined,
                    None,
                    json!({"correlation_id": c.correlation_id, "producer": c.producer, "outcome": c.outcome}),
                )?,
            ])?;
            return Ok(Accepted::Quarantined {
                correlation_id: c.correlation_id.clone(),
            });
        };
        let now = self.now_ms();
        a.completions_seen += 1;
        let completion_rec =
            NewRecord::json(kinds::COMPLETION, Some(&c.correlation_id), c)?.scoped(&a.session_id);
        match a.state {
            ActionState::Succeeded | ActionState::Failed => {
                self.commit(&[
                    action_record(&a)?,
                    self.ledger(
                        LedgerKind::CompletionDuplicate,
                        Some(&a.session_id),
                        json!({"correlation_id": a.correlation_id, "seen": a.completions_seen, "producer": c.producer}),
                    )?,
                ])?;
                return Ok(Accepted::DuplicateNoop {
                    correlation_id: a.correlation_id,
                });
            }
            ActionState::Cancelled => {
                a.resolution = Some(format!(
                    "late completion after cancel: {:?} from {}",
                    c.outcome, c.producer
                ));
                let mut frame = vec![completion_rec, action_record(&a)?];
                // A reservation the cancel held as unknown (its backend could
                // not be stopped) is reconciled by the cost this first late
                // completion brings: booked as spent, and the hold released
                // (DD7: else a cancelled task keeps it carved from its parent).
                let held = matches!(
                    a.cancel,
                    Some(CancelState::Unsupported | CancelState::OutcomeUncertain)
                );
                if held
                    && a.reservation_id.is_some()
                    && a.completions_seen == 1
                    && c.outcome != Outcome::Unknown
                {
                    if let Some(mut e) = self.execution(&a.execution_id)? {
                        let spent_before = e.budget.spent_micros;
                        e.budget.held_unknown_micros = e
                            .budget
                            .held_unknown_micros
                            .saturating_sub(a.reserved_micros);
                        e.budget.spent_micros = e
                            .budget
                            .spent_micros
                            .saturating_add(c.cost_micros.unwrap_or(a.reserved_micros));
                        e.updated_at_ms = now;
                        frame.push(exec_record(&e)?);
                        self.carry_to_parent(&e, spent_before, &mut frame)?;
                    }
                }
                frame.push(self.ledger(
                    LedgerKind::CompletionLateAfterCancel,
                    Some(&a.session_id),
                    json!({"correlation_id": a.correlation_id, "outcome": c.outcome, "producer": c.producer, "cost_usd": c.cost_micros.map(micros_to_usd)}),
                )?);
                self.commit(&frame)?;
                return Ok(Accepted::LateAfterCancel {
                    correlation_id: a.correlation_id,
                });
            }
            // `Planned` and `Authorized` are a result for something never
            // dispatched: only the in-process transport can do this legitimately
            // (dispatch and completion in one call). Treat as dispatched-then-settled.
            ActionState::Planned
            | ActionState::Authorized
            | ActionState::Dispatched
            | ActionState::OutcomeUnknown => {}
        }
        let was_unknown = a.state == ActionState::OutcomeUnknown;
        let mut e = self
            .execution(&a.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(a.execution_id.clone()))?;
        let spent_before = e.budget.spent_micros;

        // Settle the action.
        a.state = match c.outcome {
            Outcome::Succeeded => ActionState::Succeeded,
            Outcome::Failed => ActionState::Failed,
            Outcome::Unknown => ActionState::OutcomeUnknown,
        };
        a.settled_at_ms = Some(now);
        a.result_ref = c.result_ref.clone().or(a.result_ref.take());
        if c.external_op_id.is_some() {
            a.external_op_id = c.external_op_id.clone();
        }
        if was_unknown {
            a.resolution = Some(format!("resolved by {} as {:?}", c.producer, c.outcome));
        }

        // Continue the execution, in the same frame. A terminal execution
        // still drops the action from `outstanding` and settles its budget;
        // it just does not wake or queue a result (nothing will consume it).
        let mut frame = vec![completion_rec, action_record(&a)?];
        e.outstanding.retain(|x| x != &a.correlation_id);
        if a.cancel.is_some() && a.resolution.is_none() {
            a.resolution = Some(format!(
                "completed as {:?} after cancel was requested",
                c.outcome
            ));
            frame[1] = action_record(&a)?;
        }
        if let Some(r) = &a.reservation_id {
            if was_unknown {
                if c.outcome != Outcome::Unknown {
                    e.budget.held_unknown_micros = e
                        .budget
                        .held_unknown_micros
                        .saturating_sub(a.reserved_micros);
                    e.budget.spent_micros = e
                        .budget
                        .spent_micros
                        .saturating_add(c.cost_micros.unwrap_or(a.reserved_micros));
                }
            } else {
                match c.outcome {
                    Outcome::Unknown => hold_reservation_in(&mut e.budget, r),
                    _ => settle_reservation_in(&mut e.budget, r, c.cost_micros),
                }
            }
        }
        if !e.state.is_terminal() {
            // A turn's own result is read by that turn (theseus-l6y): only
            // what settles outside it waits in the queue for the next turn.
            match self.own.as_deref().filter(|o| o.execution_id == e.id) {
                Some(own) => {
                    own.settled
                        .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                }
                None => {
                    if !e.queued_results.contains(&a.correlation_id) {
                        e.queued_results.push(a.correlation_id.clone());
                    }
                }
            }
            // A waiting execution wakes if this completion is what it waited on.
            let wakes = match &e.wake {
                Some(Wake::Actions { correlation_ids }) => {
                    correlation_ids.contains(&a.correlation_id) || correlation_ids.is_empty()
                }
                _ => false,
            };
            if e.state == ExecState::Waiting && wakes {
                e.state = ExecState::Queued;
                e.wake = None;
            }
        }
        e.updated_at_ms = now;
        let exec_state = e.state;
        frame.push(exec_record(&e)?);
        // A task's cost is its parent's spend too, in this frame (DD7).
        self.carry_to_parent(&e, spent_before, &mut frame)?;
        let kind = match (was_unknown, c.outcome) {
            (true, _) => LedgerKind::ActionResolved,
            (false, Outcome::Unknown) => LedgerKind::ActionOutcomeUnknown,
            (false, Outcome::Succeeded) => LedgerKind::ActionSucceeded,
            (false, Outcome::Failed) => LedgerKind::ActionFailed,
        };
        frame.push(self.ledger(
            kind,
            Some(&a.session_id),
            json!({"correlation_id": a.correlation_id, "execution_id": e.id, "outcome": c.outcome, "producer": c.producer, "duration_ms": c.finished_at_ms.saturating_sub(c.started_at_ms), "execution_state": exec_state, "cost_usd": c.cost_micros.map(micros_to_usd)}),
        )?);
        self.commit(&frame)?;
        if was_unknown {
            Ok(Accepted::ResolvedUnknown {
                correlation_id: a.correlation_id,
                outcome: c.outcome,
            })
        } else {
            Ok(Accepted::Settled {
                correlation_id: a.correlation_id,
                execution_id: e.id,
                execution_state: exec_state,
            })
        }
    }

    /// The reconciler (or a wrapper's deadline) could not establish an outcome.
    /// `OutcomeUnknown` is knowledge: the execution gets it as a result and the
    /// reservation is held, never released (§3.16).
    pub fn mark_unknown(&self, correlation_id: &str, reason: &str) -> Result<Action> {
        // The check and the settlement are one transaction, under one lock:
        // nothing can settle the action between them.
        let execution_id = self
            .action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?
            .execution_id;
        self.frame(&[&execution_id], |k| {
            let a = k
                .action(correlation_id)?
                .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?;
            if a.state != ActionState::Dispatched {
                return Err(KernelError::ActionState {
                    correlation_id: a.correlation_id,
                    state: a.state.as_str(),
                    expected: "dispatched",
                }
                .into());
            }
            k.accept_completion(&Completion {
                correlation_id: correlation_id.into(),
                outcome: Outcome::Unknown,
                result_ref: None,
                external_op_id: None,
                started_at_ms: k.now_ms(),
                finished_at_ms: k.now_ms(),
                producer: format!("reconciler:{reason}"),
                signature: None,
                cost_micros: None,
                detail: None,
            })?;
            Ok(k.action(correlation_id)?
                .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?)
        })
    }

    // ------------------------------------------------------------ cancel

    /// `/cancel`: a deterministic control path (§3.15). Acts directly, never
    /// through admission or Jev. Returns the correlation ids whose backends
    /// must now be terminated.
    pub fn cancel_execution(&self, execution_id: &str, by: &str) -> Result<Vec<CorrelationId>> {
        Ok(self
            .cancel(execution_id, by)?
            .map(|c| c.to_kill)
            .unwrap_or_default())
    }

    /// `cancel_execution`, with records `extra` builds from what the cancel
    /// ended, in the same frame, a transaction: a task's report that it was
    /// cancelled (DD7), and the result of each tool call it planned and never
    /// sent (theseus-w98). `extra` runs only when this call cancels it, so a
    /// second cancel writes nothing, and says nothing twice.
    pub fn cancel_execution_with(
        &self,
        execution_id: &str,
        by: &str,
        extra: impl FnOnce(&Ending<'_>) -> Result<Vec<NewRecord>>,
    ) -> Result<Cancel> {
        self.frame(&[execution_id], |k| {
            let Some(c) = k.cancel(execution_id, by)? else {
                return Ok(Cancel::default());
            };
            k.stage(&extra(&Ending {
                execution: &c.execution,
                not_run: &c.not_run,
                turn_running: c.turn_running,
            })?)?;
            Ok(Cancel {
                to_kill: c.to_kill,
                not_run: c.not_run,
            })
        })
    }

    /// The cancel (`cancel_execution`). A task reaches its parent here as it
    /// does at `end_turn`. `None`: the execution had ended already, and
    /// nothing is written.
    ///
    /// Everything the execution planned and never sent ends in this frame:
    /// a tool call waiting for the operator, a call planned or authorized and
    /// not yet dispatched, and the budget question. Each settles `Cancelled`,
    /// its resolution "the execution was cancelled by <by>", its reservation
    /// released, with an `action.cancelled` row; nothing can run it now, and
    /// nothing counts it as waiting (theseus-w98). Only a dispatched call has
    /// a backend to stop: it is marked `cancel = requested` and returned in
    /// `to_kill`.
    fn cancel(&self, execution_id: &str, by: &str) -> Result<Option<Cancelled>> {
        let _w = self.lock_family(execution_id)?;
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        if e.state.is_terminal() {
            return Ok(None);
        }
        let now = self.now_ms();
        let turn_running = e.state == ExecState::Running;
        e.state = ExecState::Cancelled;
        e.cancel = Some(CancelState::Requested);
        e.ended_reason = Some(format!("cancelled by {by}"));
        e.wake = None;
        e.stopped = None;
        e.report_wakes.clear();
        e.updated_at_ms = now;
        let mut frame = Vec::new();
        let why = format!("the execution was cancelled by {by}");
        let not_run = self.end_unsent(&mut e, by, &why, &mut frame)?;
        // Its wakes end with it (DD8): `/stop` and a cancel clear them.
        self.drop_wakes(&mut e, by, "the execution was cancelled", &mut frame)?;
        frame.insert(0, exec_record(&e)?);
        let mut to_kill = Vec::new();
        for c in &e.outstanding {
            if let Some(mut a) = self.action(c)? {
                if a.state == ActionState::Dispatched {
                    a.cancel = Some(CancelState::Requested);
                    frame.push(action_record(&a)?);
                    to_kill.push(a.correlation_id.clone());
                }
            }
        }
        frame.push(self.ledger(
            LedgerKind::ExecutionCancelled,
            Some(&e.session_id),
            json!({"execution_id": e.id, "by": by, "outstanding": to_kill,
                   "not_run": not_run.iter().map(|a| a.correlation_id.as_str()).collect::<Vec<_>>()}),
        )?);
        self.task_ended(&e, &mut frame)?;
        self.commit(&frame)?;
        Ok(Some(Cancelled {
            execution: e,
            not_run,
            turn_running,
            to_kill,
        }))
    }

    /// Settle everything `e` planned and never sent, as `e` ends: a cancel,
    /// or a turn that ends it (theseus-w98). Each action of `e` planned or
    /// authorized and not yet dispatched (a tool call waiting for the
    /// operator, a call a crash left between its plan and its dispatch, the
    /// budget question) settles `Cancelled` with `why` as its resolution, its
    /// reservation released, in `frame`, with an `action.cancelled` row. The
    /// caller writes `e` after. Returns them as they are now.
    fn end_unsent(
        &self,
        e: &mut Execution,
        by: &str,
        why: &str,
        frame: &mut Vec<NewRecord>,
    ) -> Result<Vec<Action>> {
        e.budget.question = None;
        e.budget.question_needs_micros = 0;
        let now = self.now_ms();
        let mut ended = Vec::new();
        for mut a in self
            .unsettled_actions(&e.id)?
            .into_iter()
            .filter(|a| matches!(a.state, ActionState::Planned | ActionState::Authorized))
        {
            a.state = ActionState::Cancelled;
            a.settled_at_ms = Some(now);
            a.resolution = Some(why.to_string());
            if let Some(r) = &a.reservation_id {
                settle_reservation_in(&mut e.budget, r, Some(0));
            }
            frame.push(action_record(&a)?);
            frame.push(self.ledger(
                LedgerKind::ActionCancelled,
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "tool": a.tool, "by": by, "why": why}),
            )?);
            ended.push(a);
        }
        Ok(ended)
    }

    /// The backend acknowledged the cancel (signal delivered, stop requested).
    pub fn cancel_acknowledged(&self, correlation_id: &str) -> Result<Action> {
        self.cancel_step(correlation_id, CancelState::Acknowledged, false)
    }
    /// Termination verified (process gone, task stopped): the action settles `Cancelled`.
    pub fn cancel_verified(&self, correlation_id: &str) -> Result<Action> {
        self.cancel_step(correlation_id, CancelState::TerminationVerified, true)
    }
    /// The backend offers no external termination; the action settles
    /// `Cancelled` but the side effect may still complete (`LateAfterCancel`).
    pub fn cancel_unsupported(&self, correlation_id: &str) -> Result<Action> {
        self.cancel_step(correlation_id, CancelState::Unsupported, true)
    }
    pub fn cancel_uncertain(&self, correlation_id: &str) -> Result<Action> {
        self.cancel_step(correlation_id, CancelState::OutcomeUncertain, true)
    }

    fn cancel_step(&self, correlation_id: &str, st: CancelState, settle: bool) -> Result<Action> {
        let (_w, mut a) = self.locked_known_action(correlation_id)?;
        if a.state.is_settled() {
            return Ok(a);
        }
        let now = self.now_ms();
        a.cancel = Some(st);
        let mut frame = Vec::new();
        if settle {
            a.state = ActionState::Cancelled;
            a.settled_at_ms = Some(now);
            if let Some(mut e) = self.execution(&a.execution_id)? {
                let spent_before = e.budget.spent_micros;
                e.outstanding.retain(|x| x != &a.correlation_id);
                if let Some(r) = &a.reservation_id {
                    if st == CancelState::TerminationVerified {
                        settle_reservation_in(&mut e.budget, r, Some(0));
                    } else {
                        hold_reservation_in(&mut e.budget, r);
                    }
                }
                // A stop (W1) leaves the execution open: its next turn reads
                // the call as cancelled, as it reads any late result. Nothing
                // queues the execution for it.
                if !e.state.is_terminal() && !e.queued_results.contains(&a.correlation_id) {
                    e.queued_results.push(a.correlation_id.clone());
                }
                e.updated_at_ms = now;
                frame.push(exec_record(&e)?);
                self.carry_to_parent(&e, spent_before, &mut frame)?;
            }
        }
        frame.push(action_record(&a)?);
        frame.push(self.ledger(
            LedgerKind::ActionCancel,
            Some(&a.session_id),
            json!({"correlation_id": a.correlation_id, "cancel": st, "settled": settle}),
        )?);
        self.commit(&frame)?;
        Ok(a)
    }

    // ------------------------------------------------------------ reconcile

    /// The heartbeat reconciler (§3.3, §3.16): due wakes fire; dispatched
    /// actions past their deadline are checked against evidence and settled
    /// or marked unknown; unknowns are re-probed. Cost scales with the work
    /// it may act on (theseus-lv2): the executions a due time may wake, and
    /// the actions sent; the open ones are counted, not read.
    pub fn reconcile(&self, evidence: &dyn Evidence) -> Result<ReconcileReport> {
        self.reconcile_with(evidence, true)
    }

    /// `reconcile`; `due`: whether due wakes fire in this pass (startup's
    /// pass leaves them to the driver, DD8, and reads no execution).
    fn reconcile_with(&self, evidence: &dyn Evidence, due: bool) -> Result<ReconcileReport> {
        let t0 = std::time::Instant::now();
        let now = self.now_ms();
        let mut rep = ReconcileReport {
            open_executions: self.count_open_executions()?,
            open_actions: self.count_open_actions()?,
            ..Default::default()
        };
        // A due time it waits on, or a wake of its own (DD8). The scan may be
        // a frame stale: `fire_due` decides again from a read under the lock
        // (a cancel may have landed since), and queues it for the driver.
        if due {
            let execs = self.executions_by(&[terms::one("due")])?;
            for e in execs.iter().filter(|e| crate::wakes::due_now(e, now)) {
                if let Some(e) = self.fire_due(&e.id)? {
                    rep.woke_due.push(e.id);
                }
            }
        }
        let actions = self.actions_by(&[
            terms::one(&terms::action_state(ActionState::Dispatched)),
            terms::one(&terms::action_state(ActionState::OutcomeUnknown)),
        ])?;
        for a in actions {
            match a.state {
                ActionState::Dispatched => {
                    if a.deadline_at_ms > now && a.cancel.is_none() {
                        continue; // not overdue; event-first, poll only stuck work
                    }
                    match evidence.probe(&a) {
                        Probe::Completed(c) => {
                            self.accept_completion(&c)?;
                            rep.settled_from_evidence.push(a.correlation_id);
                        }
                        Probe::StillRunning => {
                            rep.still_running_past_deadline.push(a.correlation_id);
                        }
                        Probe::Gone => {
                            // Decided from the scan; `mark_unknown` checks again under
                            // the lock, and an action settled since is left as it is.
                            match self.mark_unknown(&a.correlation_id, "overdue_no_evidence") {
                                Ok(_) => rep.marked_unknown.push(a.correlation_id),
                                Err(e)
                                    if matches!(
                                        e.downcast_ref::<KernelError>(),
                                        Some(KernelError::ActionState { .. })
                                    ) => {}
                                Err(e) => return Err(e),
                            }
                        }
                    }
                }
                ActionState::OutcomeUnknown => {
                    if let Probe::Completed(c) = evidence.probe(&a) {
                        self.accept_completion(&c)?;
                        rep.resolved_unknown.push(a.correlation_id);
                    }
                }
                _ => {}
            }
        }
        rep.elapsed_us = t0.elapsed().as_micros() as u64;
        if !(rep.woke_due.is_empty()
            && rep.settled_from_evidence.is_empty()
            && rep.marked_unknown.is_empty()
            && rep.resolved_unknown.is_empty())
        {
            self.commit(&[self.ledger(
                LedgerKind::Reconcile,
                None,
                serde_json::to_value(&rep)?,
            )?])?;
        }
        Ok(rep)
    }

    // ------------------------------------------------------------ startup

    /// The five-step startup (§3.3, Part II M2). Each step is idempotent, so a
    /// crash inside any of them is recovered by running startup again:
    /// 1. store opened and recovered (done by the caller; recorded here),
    /// 2. executions found `Running` were mid-turn when the process died:
    ///    requeue them as interrupted,
    /// 3. drain the completion spool (each file settles, then is removed),
    /// 4. reconcile against evidence,
    /// 5. accept events.
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

        // 2. load executions; requeue interrupted turns; rewrite, once, the
        //    executions stored with a unit budget (theseus-0sg); and, under a
        //    config that may act, give every open execution that follows the
        //    config a changed spend limit (theseus-3pj; under a copy the vault
        //    has not confirmed, `follow_spend_limit` does it on the vault's
        //    word). The rewrites share one frame: the first start under this
        //    binary, or under a changed limit, pays one fsync for them, and
        //    every later start finds none. It reads only those executions,
        //    by their terms (theseus-lv2): a turn running, a unit budget, a
        //    limit other than the config's. A parked one is read by none.
        let t = std::time::Instant::now();
        *self.phase.lock().unwrap() = 2;
        let now = self.now_ms();
        let mut migrated = Vec::new();
        let mut rewritten = 0u32;
        let follow = !self.cfg.unconfirmed_config;
        let mut wanted = vec![
            terms::one(&terms::state(ExecState::Running)),
            terms::one("legacy"),
        ];
        if follow {
            wanted.extend(terms::limits_other_than(self.cfg.spend_limit_micros));
        }
        let all = self.executions_by(&wanted)?;
        let legacy = all.iter().filter(|e| e.schema < SCHEMA).count();
        if legacy > 0 && self.cfg.unconfirmed_config {
            return Err(KernelError::UnconfirmedConfig { executions: legacy }.into());
        }
        // The executions this step rewrites are read again under their locks,
        // taken together in id order and held to the step's end, so the scan
        // decides nothing it writes. A clean start reads and rewrites none.
        let rewrites = |e: &Execution| {
            e.state == ExecState::Running || e.schema < SCHEMA || (follow && self.follows_limit(e))
        };
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
            if e.schema < SCHEMA {
                e.budget.spent_micros = self.legacy_spend.as_ref().map_or(0, |f| f(&e.session_id));
                e.schema = SCHEMA;
                rows.push(self.ledger(
                    LedgerKind::BudgetMigrated,
                    Some(&e.session_id),
                    json!({"execution_id": e.id, "state": e.state, "limit_usd": micros_to_usd(e.budget.limit_micros), "spent_usd": micros_to_usd(e.budget.spent_micros), "units_before": e.budget.units_before}),
                )?);
                rewritten += 1;
            }
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
            if follow {
                if let Some((f, records)) = self.follow_limit(&mut e, now)? {
                    rows.extend(records);
                    rep.limits_followed.push(f);
                }
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
            json!({"step": 2, "name": "load", "requeued": rep.requeued_interrupted, "budgets_in_dollars": rewritten, "limits_followed": rep.limits_followed.len()}),
        )?);
        step(2, "load", t)?;

        // 3. drain spool
        let t = std::time::Instant::now();
        *self.phase.lock().unwrap() = 3;
        if let Some(sp) = spool {
            let drained = sp.drain()?;
            for (path, c) in drained.completions {
                self.accept_completion(&c)?;
                sp.remove(&path)?; // after the frame: a crash here redelivers, which is a no-op
                rep.spool_drained += 1;
            }
            rep.spool_quarantined = drained.malformed as u32;
        }
        step_rows.push(self.ledger(
            LedgerKind::StartupStep,
            None,
            json!({"step": 3, "name": "spool", "drained": rep.spool_drained, "malformed": rep.spool_quarantined}),
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

fn hold_reservation_in(b: &mut Budget, reservation_id: &str) {
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
