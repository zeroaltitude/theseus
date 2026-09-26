//! The kernel: every state transition of executions and actions, written as
//! WAL frames through the `Store` contract. Synchronous and deterministic:
//! time comes from an injected `Clock`, randomness from nowhere, and the only
//! process state is the set of turn locks currently held. Drop the `Kernel`
//! and reopen the store and nothing is lost but the locks, which is the point.
//!
//! Every mutating method writes exactly one frame: the records that change
//! plus the ledger rows that describe the change. A crash between two frames
//! leaves the store in a state some earlier method call produced, never in a
//! state no method produces.

use std::collections::{BTreeMap, HashSet};
use std::sync::{Arc, Mutex};

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use theseus_store::{kinds, NewRecord, Record, Store};

use crate::clock::Clock;
use crate::gate::{digest_proposal, Proposal};
use crate::types::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelConfig {
    /// How many executions may hold a turn at once (§3.2a admission scheduler).
    pub admission_ceiling: u32,
    /// Deadline for an action whose tool declares none.
    pub default_deadline_ms: u64,
    /// Budget units a new execution gets when the caller names none.
    pub default_budget: u64,
    /// Units kept back for control and cleanup.
    pub control_reserve: u64,
    /// How long a confirmation stays valid.
    pub confirm_ttl_ms: u64,
    /// Reconciler cadence (the heartbeat, §3.3).
    pub heartbeat_ms: u64,
    /// Test hook: startup returns an error after this step, as if the process
    /// died there. Never set outside the simulator.
    #[serde(skip)]
    pub fault_after_startup_step: Option<u8>,
}

impl Default for KernelConfig {
    fn default() -> Self {
        Self {
            admission_ceiling: 8,
            default_deadline_ms: 10 * 60 * 1000,
            default_budget: 1_000_000,
            control_reserve: 10_000,
            confirm_ttl_ms: 15 * 60 * 1000,
            heartbeat_ms: 60_000,
            fault_after_startup_step: None,
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
    #[error("budget exhausted: needed {needed} units, {available} available (limit {limit})")]
    BudgetExhausted {
        needed: u64,
        available: u64,
        limit: u64,
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
    #[error("policy denied: {reason}")]
    PolicyDenied { reason: String },
    #[error("unknown execution {0}")]
    UnknownExecution(ExecutionId),
    #[error("unknown action {0}")]
    UnknownAction(CorrelationId),
    #[error("unknown session {0}")]
    UnknownSession(SessionId),
    #[error("kernel is not accepting events yet (startup step {step})")]
    NotAccepting { step: u8 },
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
    pub spool_drained: u32,
    pub spool_quarantined: u32,
    pub reconcile: ReconcileReport,
    pub elapsed_us: u64,
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

pub struct Kernel {
    store: Arc<dyn Store>,
    clock: Arc<dyn Clock>,
    cfg: KernelConfig,
    held: Arc<Mutex<HashSet<ExecutionId>>>,
    /// 0..=4 while starting; 5 once accepting events.
    phase: Mutex<u8>,
}

const QUARANTINE_PREFIX: &str = "quarantine:";

impl Kernel {
    /// Wrap an opened store. The kernel is not accepting events until
    /// `startup` has run; `open_session`/`admit` etc. refuse before that.
    pub fn new(store: Arc<dyn Store>, clock: Arc<dyn Clock>, cfg: KernelConfig) -> Self {
        Self {
            store,
            clock,
            cfg,
            held: Arc::new(Mutex::new(HashSet::new())),
            phase: Mutex::new(0),
        }
    }

    pub fn store(&self) -> &Arc<dyn Store> {
        &self.store
    }
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }
    pub fn config(&self) -> &KernelConfig {
        &self.cfg
    }
    pub fn now_ms(&self) -> u64 {
        self.clock.now_ms()
    }
    pub fn is_accepting(&self) -> bool {
        *self.phase.lock().unwrap() >= 5
    }

    fn require_accepting(&self) -> Result<()> {
        let p = *self.phase.lock().unwrap();
        if p < 5 {
            return Err(KernelError::NotAccepting { step: p }.into());
        }
        Ok(())
    }

    // ------------------------------------------------------------ reads

    pub fn execution(&self, id: &str) -> Result<Option<Execution>> {
        decode_opt(self.store.latest_by_key(kinds::EXECUTION, id)?)
    }
    pub fn action(&self, correlation_id: &str) -> Result<Option<Action>> {
        decode_opt(self.store.latest_by_key(kinds::ACTION, correlation_id)?)
    }
    pub fn session(&self, id: &str) -> Result<Option<Session>> {
        decode_opt(self.store.latest_by_key(kinds::SESSION, id)?)
    }
    pub fn executions(&self) -> Result<Vec<Execution>> {
        decode_all(self.store.latest_of_kind(kinds::EXECUTION)?)
    }
    pub fn open_executions(&self) -> Result<Vec<Execution>> {
        Ok(self
            .executions()?
            .into_iter()
            .filter(|e| !e.state.is_terminal())
            .collect())
    }
    pub fn actions(&self) -> Result<Vec<Action>> {
        decode_all(self.store.latest_of_kind(kinds::ACTION)?)
    }
    pub fn open_actions(&self) -> Result<Vec<Action>> {
        Ok(self
            .actions()?
            .into_iter()
            .filter(|a| !a.state.is_settled())
            .collect())
    }
    pub fn sessions(&self) -> Result<Vec<Session>> {
        decode_all(self.store.latest_of_kind(kinds::SESSION)?)
    }
    /// Completions that matched no action, newest last.
    pub fn quarantined(&self) -> Result<Vec<Completion>> {
        let mut out = Vec::new();
        for r in self.store.latest_of_kind(kinds::COMPLETION)? {
            if r.key
                .as_deref()
                .is_some_and(|k| k.starts_with(QUARANTINE_PREFIX))
            {
                out.push(r.decode()?);
            }
        }
        Ok(out)
    }
    /// Records in a session's scope after `after`, oldest first: the tail walk.
    pub fn session_tail(&self, session_id: &str, after: u64, limit: usize) -> Result<Vec<Record>> {
        self.store.scan_scope(session_id, after, limit)
    }

    pub fn stats(&self) -> Result<KernelStats> {
        let mut s = KernelStats {
            turns_held: self.held.lock().unwrap().len() as u32,
            admission_ceiling: self.cfg.admission_ceiling,
            accepting: self.is_accepting(),
            ..Default::default()
        };
        for e in self.executions()? {
            *s.executions_by_state
                .entry(e.state.as_str().to_string())
                .or_default() += 1;
        }
        for a in self.actions()? {
            *s.actions_by_state
                .entry(a.state.as_str().to_string())
                .or_default() += 1;
        }
        s.quarantined_completions = self.quarantined()?.len() as u64;
        Ok(s)
    }

    // ------------------------------------------------------------ frames

    fn ledger(&self, kind: &str, session: Option<&str>, data: Value) -> Result<NewRecord> {
        let row = LedgerRow {
            at_unix_ms: self.now_ms(),
            kind: kind.into(),
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

    fn exec_record(&self, e: &Execution) -> Result<NewRecord> {
        Ok(NewRecord::json(kinds::EXECUTION, Some(&e.id), e)?.scoped(&e.session_id))
    }
    fn action_record(&self, a: &Action) -> Result<NewRecord> {
        Ok(NewRecord::json(kinds::ACTION, Some(&a.correlation_id), a)?.scoped(&a.session_id))
    }

    fn commit(&self, frame: Vec<NewRecord>) -> Result<Vec<u64>> {
        self.store.append(&frame).context("kernel frame")
    }

    // ------------------------------------------------------------ sessions

    /// Create the one execution for a session whose record someone else
    /// owns (the core's `SessionRecord`). Starts `Waiting` on input.
    pub fn open_execution(
        &self,
        session_id: &str,
        kind: SessionKind,
        authority: Authority,
        budget_limit: Option<u64>,
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
            budget: Budget::new(
                budget_limit.unwrap_or(self.cfg.default_budget),
                self.cfg.control_reserve,
            ),
            wake: Some(Wake::Input),
            outstanding: vec![],
            queued_results: vec![],
            parent: None,
            reports_to,
            turns: 0,
            interrupted: 0,
            cancel: None,
            ended_reason: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        self.commit(vec![
            self.exec_record(&exec)?,
            self.ledger(
                "execution.opened",
                Some(session_id),
                json!({"execution_id": exec.id, "kind": kind, "budget": exec.budget.limit}),
            )?,
        ])?;
        Ok(exec)
    }

    /// Create a session and its one execution in one frame (§3.2a).
    pub fn open_session(
        &self,
        kind: SessionKind,
        roots: Vec<String>,
        authority: Authority,
        budget_limit: Option<u64>,
        label: Option<String>,
    ) -> Result<(Session, Execution)> {
        self.require_accepting()?;
        let now = self.now_ms();
        let exec = Execution {
            id: new_id("exe"),
            schema: SCHEMA,
            session_id: new_id("ses"),
            kind,
            state: ExecState::Waiting,
            authority,
            budget: Budget::new(
                budget_limit.unwrap_or(self.cfg.default_budget),
                self.cfg.control_reserve,
            ),
            wake: Some(Wake::Input),
            outstanding: vec![],
            queued_results: vec![],
            parent: None,
            reports_to: roots.first().cloned(),
            turns: 0,
            interrupted: 0,
            cancel: None,
            ended_reason: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        let session = Session {
            id: exec.session_id.clone(),
            schema: SCHEMA,
            kind,
            roots,
            execution_id: exec.id.clone(),
            tail_from: self.store.last_position(),
            label,
            created_at_ms: now,
        };
        self.commit(vec![
            NewRecord::json(kinds::SESSION, Some(&session.id), &session)?.scoped(&session.id),
            self.exec_record(&exec)?,
            self.ledger(
                "session.opened",
                Some(&session.id),
                json!({"execution_id": exec.id, "kind": kind, "roots": session.roots}),
            )?,
        ])?;
        Ok((session, exec))
    }

    /// Promotion (§3.2a): fork a task execution from a running one. Authority
    /// is derived (never widened), budget is carved from the parent's
    /// available units, `reports_to` is the parent, and the `derived_from`
    /// edge is written in the same frame.
    pub fn promote(
        &self,
        parent_id: &str,
        roots: Vec<String>,
        requested_ceilings: &BTreeMap<String, String>,
        budget_units: u64,
        label: Option<String>,
    ) -> Result<(Session, Execution)> {
        self.require_accepting()?;
        let mut parent = self
            .execution(parent_id)?
            .ok_or_else(|| KernelError::UnknownExecution(parent_id.into()))?;
        if parent.state.is_terminal() {
            return Err(KernelError::NotRunnable {
                id: parent.id.clone(),
                state: parent.state.as_str(),
            }
            .into());
        }
        let authority = parent
            .authority
            .derive(requested_ceilings)
            .map_err(|e| anyhow!("authority: {e}"))?;
        let carve = budget_units.saturating_add(self.cfg.control_reserve);
        if parent.budget.available() < carve {
            return Err(KernelError::BudgetExhausted {
                needed: carve,
                available: parent.budget.available(),
                limit: parent.budget.limit,
            }
            .into());
        }
        parent.budget.limit -= carve;
        let now = self.now_ms();
        parent.updated_at_ms = now;
        let child = Execution {
            id: new_id("exe"),
            schema: SCHEMA,
            session_id: new_id("ses"),
            kind: SessionKind::Task,
            state: ExecState::Queued,
            authority,
            budget: Budget::new(carve, self.cfg.control_reserve),
            wake: None,
            outstanding: vec![],
            queued_results: vec![],
            parent: Some(parent.id.clone()),
            reports_to: Some(parent.id.clone()),
            turns: 0,
            interrupted: 0,
            cancel: None,
            ended_reason: None,
            created_at_ms: now,
            updated_at_ms: now,
        };
        let session = Session {
            id: child.session_id.clone(),
            schema: SCHEMA,
            kind: SessionKind::Task,
            roots,
            execution_id: child.id.clone(),
            tail_from: self.store.last_position(),
            label,
            created_at_ms: now,
        };
        let edge = json!({
            "type": "derived_from",
            "from": session.id,
            "to": parent.session_id,
            "at_ms": now,
        });
        let edge_key = format!("derived_from|{}|{}", session.id, parent.session_id);
        self.commit(vec![
            NewRecord::json(kinds::SESSION, Some(&session.id), &session)?.scoped(&session.id),
            self.exec_record(&child)?,
            self.exec_record(&parent)?,
            NewRecord::json(kinds::EDGE, Some(&edge_key), &edge)?.scoped(&session.id),
            self.ledger(
                "execution.promoted",
                Some(&parent.session_id),
                json!({"parent": parent.id, "child": child.id, "child_session": session.id, "budget_carved": carve}),
            )?,
        ])?;
        Ok((session, child))
    }

    // ------------------------------------------------------------ turns

    /// Human input arrived on a session: its execution becomes runnable.
    pub fn wake_input(&self, execution_id: &str) -> Result<Execution> {
        self.require_accepting()?;
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
            self.commit(vec![
                self.exec_record(&e)?,
                self.ledger(
                    "execution.queued",
                    Some(&e.session_id),
                    json!({"execution_id": e.id, "why": "input"}),
                )?,
            ])?;
        }
        Ok(e)
    }

    /// Take a turn: the execution must be `Queued`, the ceiling must have
    /// room, and no turn may be held on it in this process. Writes `Running`.
    pub fn admit(&self, execution_id: &str) -> Result<TurnGuard> {
        self.require_accepting()?;
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
        e.state = ExecState::Running;
        e.turns += 1;
        e.wake = None;
        e.updated_at_ms = now;
        let res = self.commit(vec![
            self.exec_record(&e)?,
            self.ledger(
                "execution.running",
                Some(&e.session_id),
                json!({"execution_id": e.id, "turn": e.turns, "queued_results": e.queued_results.len()}),
            )?,
        ]);
        if let Err(err) = res {
            self.held.lock().unwrap().remove(&e.id);
            return Err(err);
        }
        Ok(TurnGuard {
            execution_id: e.id,
            turn: e.turns,
            started_at_ms: now,
            held: self.held.clone(),
        })
    }

    /// The results queued for this execution (settled since its last turn),
    /// and clear them in the store as consumed. Call inside a held turn.
    pub fn take_results(&self, guard: &TurnGuard) -> Result<Vec<Action>> {
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
        self.commit(vec![
            self.exec_record(&e)?,
            self.ledger(
                "execution.results_consumed",
                Some(&e.session_id),
                json!({"execution_id": e.id, "count": n}),
            )?,
        ])?;
        Ok(out)
    }

    /// End the held turn with the Advancer's decision. Consumes the guard.
    pub fn end_turn(&self, guard: TurnGuard, end: TurnEnd) -> Result<Execution> {
        let mut e = self
            .execution(&guard.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(guard.execution_id.clone()))?;
        let now = self.now_ms();
        // A cancel or budget exhaustion that landed during the turn wins over
        // the Advancer: terminal states are never overwritten.
        if e.state.is_terminal() {
            drop(guard);
            return Ok(e);
        }
        let kind;
        match end {
            TurnEnd::Complete { reason } => {
                e.state = ExecState::Complete;
                e.ended_reason = Some(reason);
                kind = "execution.complete";
            }
            TurnEnd::Wait { wake } => {
                // Results that arrived during the turn make it runnable again.
                if !e.queued_results.is_empty() {
                    e.state = ExecState::Queued;
                    e.wake = None;
                    kind = "execution.queued";
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
                            let frame = vec![
                                self.exec_record(&e)?,
                                self.ledger(
                                    "execution.queued",
                                    Some(&e.session_id),
                                    json!({"execution_id": e.id, "why": "wake_actions_already_settled"}),
                                )?,
                            ];
                            self.commit(frame)?;
                            drop(guard);
                            return Ok(e);
                        }
                    }
                    e.state = ExecState::Waiting;
                    e.wake = Some(wake);
                    kind = "execution.waiting";
                }
            }
            TurnEnd::Requeue => {
                e.state = ExecState::Queued;
                e.wake = None;
                kind = "execution.queued";
            }
            TurnEnd::Fail { reason } => {
                e.state = ExecState::Failed;
                e.ended_reason = Some(reason);
                kind = "execution.failed";
            }
            TurnEnd::Blocked { reason } => {
                e.state = ExecState::Blocked;
                e.ended_reason = Some(reason);
                kind = "execution.blocked";
            }
        }
        e.updated_at_ms = now;
        let mut frame = vec![
            self.exec_record(&e)?,
            self.ledger(
                kind,
                Some(&e.session_id),
                json!({"execution_id": e.id, "turn": guard.turn, "turn_ms": now.saturating_sub(guard.started_at_ms), "wake": e.wake, "reason": e.ended_reason}),
            )?,
        ];
        // Ending with work still outstanding is a cancel of that work: the
        // actions get `cancel_requested` in the same frame and the harness
        // terminates their backends (they stay in `outstanding` until verified).
        if e.state.is_terminal() {
            for c in &e.outstanding {
                if let Some(mut a) = self.action(c)? {
                    if a.state == ActionState::Dispatched && a.cancel.is_none() {
                        a.cancel = Some(CancelState::Requested);
                        frame.push(self.action_record(&a)?);
                        frame.push(self.ledger(
                            "action.cancel",
                            Some(&a.session_id),
                            json!({"correlation_id": a.correlation_id, "cancel": CancelState::Requested, "why": "execution ended", "settled": false}),
                        )?);
                    }
                }
            }
        }
        // A child reaching a terminal state wakes a parent waiting on it.
        if e.state.is_terminal() {
            if let Some(pid) = &e.parent {
                if let Some(mut p) = self.execution(pid)? {
                    if let Some(Wake::Execution { execution_id }) = &p.wake {
                        if execution_id == &e.id && p.state == ExecState::Waiting {
                            p.state = ExecState::Queued;
                            p.wake = None;
                            p.updated_at_ms = now;
                            frame.push(self.exec_record(&p)?);
                            frame.push(self.ledger(
                                "execution.queued",
                                Some(&p.session_id),
                                json!({"execution_id": p.id, "why": "child_terminal", "child": e.id}),
                            )?);
                        }
                    }
                }
            }
        }
        self.commit(frame)?;
        drop(guard);
        Ok(e)
    }

    // ------------------------------------------------------------ budget

    /// Reserve units against an execution's budget. Hard limit: refusal is
    /// `BudgetExhausted`, and the execution is moved to that terminal state in
    /// the same call so nothing keeps spending against a dry budget.
    pub fn reserve(&self, execution_id: &str, units: u64, purpose: &str) -> Result<String> {
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        let available = e.budget.available();
        if units > available {
            e.state = ExecState::BudgetExhausted;
            e.ended_reason = Some(format!(
                "reservation of {units} for {purpose} exceeds {available} available"
            ));
            e.updated_at_ms = self.now_ms();
            self.commit(vec![
                self.exec_record(&e)?,
                self.ledger(
                    "execution.budget_exhausted",
                    Some(&e.session_id),
                    json!({"execution_id": e.id, "needed": units, "available": available, "budget": e.budget}),
                )?,
            ])?;
            return Err(KernelError::BudgetExhausted {
                needed: units,
                available,
                limit: e.budget.limit,
            }
            .into());
        }
        let id = new_id("rsv");
        e.budget.reserved += units;
        e.budget.reservations.insert(id.clone(), units);
        e.updated_at_ms = self.now_ms();
        self.commit(vec![
            self.exec_record(&e)?,
            self.ledger(
                "budget.reserved",
                Some(&e.session_id),
                json!({"execution_id": e.id, "reservation": id, "units": units, "purpose": purpose, "available_after": e.budget.available()}),
            )?,
        ])?;
        Ok(id)
    }

    /// Settle a reservation: known usage converts to spent (and the remainder
    /// releases); `None` means usage is unknown and the reservation is held.
    pub fn settle_reservation(
        &self,
        execution_id: &str,
        reservation_id: &str,
        actual: Option<u64>,
    ) -> Result<Execution> {
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        settle_reservation_in(&mut e.budget, reservation_id, actual);
        e.updated_at_ms = self.now_ms();
        self.commit(vec![
            self.exec_record(&e)?,
            self.ledger(
                "budget.settled",
                Some(&e.session_id),
                json!({"execution_id": e.id, "reservation": reservation_id, "actual": actual, "budget": e.budget}),
            )?,
        ])?;
        Ok(e)
    }

    // ------------------------------------------------------------ actions

    /// `planned`: mint the correlation id and commit before anything else
    /// happens (§3.16). Requires a held turn (the execution is `Running`).
    /// `reserve_units` reserves budget in the same frame; exhaustion refuses.
    pub fn plan_action(
        &self,
        guard: &TurnGuard,
        proposal: &Proposal,
        retry_class: RetryClass,
        deadline_ms: Option<u64>,
        reserve_units: u64,
    ) -> Result<Action> {
        let mut e = self
            .execution(&guard.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(guard.execution_id.clone()))?;
        if e.state != ExecState::Running {
            return Err(KernelError::NoTurn {
                id: e.id.clone(),
                state: e.state.as_str(),
            }
            .into());
        }
        let now = self.now_ms();
        let mut frame = Vec::new();
        let mut reservation_id = None;
        if reserve_units > 0 {
            let available = e.budget.available();
            if reserve_units > available {
                e.state = ExecState::BudgetExhausted;
                e.ended_reason = Some(format!(
                    "action {} needs {reserve_units} units, {available} available",
                    proposal.tool
                ));
                e.updated_at_ms = now;
                self.commit(vec![
                    self.exec_record(&e)?,
                    self.ledger(
                        "execution.budget_exhausted",
                        Some(&e.session_id),
                        json!({"execution_id": e.id, "needed": reserve_units, "available": available}),
                    )?,
                ])?;
                return Err(KernelError::BudgetExhausted {
                    needed: reserve_units,
                    available,
                    limit: e.budget.limit,
                }
                .into());
            }
            let id = new_id("rsv");
            e.budget.reserved += reserve_units;
            e.budget.reservations.insert(id.clone(), reserve_units);
            reservation_id = Some(id);
            e.updated_at_ms = now;
            frame.push(self.exec_record(&e)?);
        }
        let a = Action {
            correlation_id: new_id("act"),
            schema: SCHEMA,
            execution_id: e.id.clone(),
            session_id: e.session_id.clone(),
            tool: proposal.tool.clone(),
            args_digest: digest_proposal(proposal),
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
            reserved_units: reserve_units,
            resolution: None,
            completions_seen: 0,
        };
        frame.push(self.action_record(&a)?);
        frame.push(self.ledger(
            "action.planned",
            Some(&a.session_id),
            json!({"execution_id": a.execution_id, "correlation_id": a.correlation_id, "tool": a.tool, "args_digest": a.args_digest, "retry_class": a.retry_class, "deadline_at_ms": a.deadline_at_ms, "reserved": reserve_units}),
        )?);
        self.commit(frame)?;
        Ok(a)
    }

    /// Bind a confirmation to the action's *current* digest (§3.9).
    pub fn bind_confirm(
        &self,
        correlation_id: &str,
        by: &str,
        proposal: &Proposal,
    ) -> Result<Action> {
        let mut a = self
            .action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?;
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
        self.commit(vec![
            self.action_record(&a)?,
            self.ledger(
                "action.confirmed",
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
        let mut a = self
            .action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?;
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
        self.commit(vec![
            self.action_record(&a)?,
            self.ledger(
                "action.authorized",
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "confirmed": a.confirm.is_some()}),
            )?,
        ])?;
        Ok(a)
    }

    /// `dispatched`: committed before the call is made (transactional
    /// outbox). The execution records the action as outstanding.
    pub fn dispatch(&self, correlation_id: &str, external_op_id: Option<&str>) -> Result<Action> {
        let mut a = self
            .action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?;
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
        if e.state == ExecState::Cancelled {
            // A cancel landed between plan and dispatch: the action never leaves.
            a.state = ActionState::Cancelled;
            a.cancel = Some(CancelState::TerminationVerified);
            a.settled_at_ms = Some(self.now_ms());
            self.commit(vec![
                self.action_record(&a)?,
                self.ledger(
                    "action.cancelled",
                    Some(&a.session_id),
                    json!({"correlation_id": a.correlation_id, "why": "execution cancelled before dispatch"}),
                )?,
            ])?;
            return Err(KernelError::NotRunnable {
                id: e.id,
                state: "cancelled",
            }
            .into());
        }
        let now = self.now_ms();
        a.state = ActionState::Dispatched;
        a.dispatched_at_ms = Some(now);
        a.external_op_id = external_op_id.map(str::to_string);
        if !e.outstanding.contains(&a.correlation_id) {
            e.outstanding.push(a.correlation_id.clone());
        }
        e.updated_at_ms = now;
        self.commit(vec![
            self.action_record(&a)?,
            self.exec_record(&e)?,
            self.ledger(
                "action.dispatched",
                Some(&a.session_id),
                json!({"correlation_id": a.correlation_id, "execution_id": e.id, "tool": a.tool, "external_op_id": a.external_op_id, "deadline_at_ms": a.deadline_at_ms}),
            )?,
        ])?;
        Ok(a)
    }

    /// Accept a completion from any transport (§3.16). Idempotent; atomic
    /// with the owning execution's continuation.
    pub fn accept_completion(&self, c: &Completion) -> Result<Accepted> {
        let now = self.now_ms();
        let Some(mut a) = self.action(&c.correlation_id)? else {
            let key = format!("{QUARANTINE_PREFIX}{}", c.correlation_id);
            self.commit(vec![
                NewRecord::json(kinds::COMPLETION, Some(&key), c)?,
                self.ledger(
                    "completion.quarantined",
                    None,
                    json!({"correlation_id": c.correlation_id, "producer": c.producer, "outcome": c.outcome}),
                )?,
            ])?;
            return Ok(Accepted::Quarantined {
                correlation_id: c.correlation_id.clone(),
            });
        };
        a.completions_seen += 1;
        let completion_rec =
            NewRecord::json(kinds::COMPLETION, Some(&c.correlation_id), c)?.scoped(&a.session_id);
        match a.state {
            ActionState::Succeeded | ActionState::Failed => {
                self.commit(vec![
                    self.action_record(&a)?,
                    self.ledger(
                        "completion.duplicate",
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
                self.commit(vec![
                    completion_rec,
                    self.action_record(&a)?,
                    self.ledger(
                        "completion.late_after_cancel",
                        Some(&a.session_id),
                        json!({"correlation_id": a.correlation_id, "outcome": c.outcome, "producer": c.producer}),
                    )?,
                ])?;
                return Ok(Accepted::LateAfterCancel {
                    correlation_id: a.correlation_id,
                });
            }
            ActionState::Planned | ActionState::Authorized => {
                // A result for something never dispatched: only the in-process
                // transport can do this legitimately (dispatch and completion in
                // one call). Treat as dispatched-then-settled.
            }
            ActionState::Dispatched | ActionState::OutcomeUnknown => {}
        }
        let was_unknown = a.state == ActionState::OutcomeUnknown;
        let mut e = self
            .execution(&a.execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(a.execution_id.clone()))?;

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
        let mut frame = vec![completion_rec, self.action_record(&a)?];
        e.outstanding.retain(|x| x != &a.correlation_id);
        if a.cancel.is_some() && a.resolution.is_none() {
            a.resolution = Some(format!(
                "completed as {:?} after cancel was requested",
                c.outcome
            ));
            frame[1] = self.action_record(&a)?;
        }
        if let Some(r) = &a.reservation_id {
            if was_unknown {
                if c.outcome != Outcome::Unknown {
                    e.budget.held_unknown = e.budget.held_unknown.saturating_sub(a.reserved_units);
                    e.budget.spent = e
                        .budget
                        .spent
                        .saturating_add(c.usage_units.unwrap_or(a.reserved_units));
                }
            } else {
                match c.outcome {
                    Outcome::Unknown => hold_reservation_in(&mut e.budget, r),
                    _ => settle_reservation_in(&mut e.budget, r, c.usage_units),
                }
            }
        }
        if !e.state.is_terminal() {
            if !e.queued_results.contains(&a.correlation_id) {
                e.queued_results.push(a.correlation_id.clone());
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
        frame.push(self.exec_record(&e)?);
        let kind = match (was_unknown, c.outcome) {
            (true, _) => "action.resolved",
            (false, Outcome::Unknown) => "action.outcome_unknown",
            (false, Outcome::Succeeded) => "action.succeeded",
            (false, Outcome::Failed) => "action.failed",
        };
        frame.push(self.ledger(
            kind,
            Some(&a.session_id),
            json!({"correlation_id": a.correlation_id, "execution_id": e.id, "outcome": c.outcome, "producer": c.producer, "duration_ms": c.finished_at_ms.saturating_sub(c.started_at_ms), "execution_state": exec_state, "usage_units": c.usage_units}),
        )?);
        self.commit(frame)?;
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
        let c = Completion {
            correlation_id: correlation_id.into(),
            outcome: Outcome::Unknown,
            result_ref: None,
            external_op_id: None,
            started_at_ms: self.now_ms(),
            finished_at_ms: self.now_ms(),
            producer: format!("reconciler:{reason}"),
            signature: None,
            usage_units: None,
        };
        let a = self
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
        self.accept_completion(&c)?;
        self.action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))
            .map_err(Into::into)
    }

    // ------------------------------------------------------------ cancel

    /// `/cancel`: a deterministic control path (§3.15). Acts directly, never
    /// through admission or Jev. Returns the correlation ids whose backends
    /// must now be terminated.
    pub fn cancel_execution(&self, execution_id: &str, by: &str) -> Result<Vec<CorrelationId>> {
        let mut e = self
            .execution(execution_id)?
            .ok_or_else(|| KernelError::UnknownExecution(execution_id.into()))?;
        if e.state.is_terminal() {
            return Ok(vec![]);
        }
        let now = self.now_ms();
        e.state = ExecState::Cancelled;
        e.cancel = Some(CancelState::Requested);
        e.ended_reason = Some(format!("cancelled by {by}"));
        e.wake = None;
        e.updated_at_ms = now;
        let mut frame = vec![self.exec_record(&e)?];
        let mut to_kill = Vec::new();
        for c in &e.outstanding {
            if let Some(mut a) = self.action(c)? {
                if a.state == ActionState::Dispatched {
                    a.cancel = Some(CancelState::Requested);
                    frame.push(self.action_record(&a)?);
                    to_kill.push(a.correlation_id.clone());
                }
            }
        }
        // Planned/authorized actions that never dispatched are simply cancelled.
        frame.push(self.ledger(
            "execution.cancelled",
            Some(&e.session_id),
            json!({"execution_id": e.id, "by": by, "outstanding": to_kill}),
        )?);
        self.commit(frame)?;
        Ok(to_kill)
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
        let mut a = self
            .action(correlation_id)?
            .ok_or_else(|| KernelError::UnknownAction(correlation_id.into()))?;
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
                e.outstanding.retain(|x| x != &a.correlation_id);
                if let Some(r) = &a.reservation_id {
                    if st == CancelState::TerminationVerified {
                        settle_reservation_in(&mut e.budget, r, Some(0));
                    } else {
                        hold_reservation_in(&mut e.budget, r);
                    }
                }
                e.updated_at_ms = now;
                frame.push(self.exec_record(&e)?);
            }
        }
        frame.push(self.action_record(&a)?);
        frame.push(self.ledger(
            "action.cancel",
            Some(&a.session_id),
            json!({"correlation_id": a.correlation_id, "cancel": st, "settled": settle}),
        )?);
        self.commit(frame)?;
        Ok(a)
    }

    // ------------------------------------------------------------ reconcile

    /// The heartbeat reconciler (§3.3, §3.16): due wakes fire; dispatched
    /// actions past their deadline are checked against evidence and settled
    /// or marked unknown; unknowns are re-probed. Cost scales with open work.
    pub fn reconcile(&self, evidence: &dyn Evidence) -> Result<ReconcileReport> {
        let t0 = std::time::Instant::now();
        let now = self.now_ms();
        let mut rep = ReconcileReport::default();
        let execs = self.open_executions()?;
        rep.open_executions = execs.len() as u64;
        for mut e in execs {
            if let (ExecState::Waiting, Some(Wake::DueAt { at_ms })) = (e.state, &e.wake) {
                if *at_ms <= now {
                    e.state = ExecState::Queued;
                    e.wake = None;
                    e.updated_at_ms = now;
                    self.commit(vec![
                        self.exec_record(&e)?,
                        self.ledger(
                            "execution.queued",
                            Some(&e.session_id),
                            json!({"execution_id": e.id, "why": "due"}),
                        )?,
                    ])?;
                    rep.woke_due.push(e.id.clone());
                }
            }
        }
        let actions = self.open_actions()?;
        rep.open_actions = actions.len() as u64;
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
                            self.mark_unknown(&a.correlation_id, "overdue_no_evidence")?;
                            rep.marked_unknown.push(a.correlation_id);
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
            self.commit(vec![self.ledger(
                "reconcile",
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
        let st = self.store.stats()?;
        self.commit(vec![self.ledger(
            "startup.step",
            None,
            json!({"step": 1, "name": "store", "last_position": st.last_position, "truncated_bytes": st.truncated_bytes, "replayed_into_index": st.replayed_into_index}),
        )?])?;
        step(1, "store", t)?;

        // 2. load executions; requeue interrupted turns
        let t = std::time::Instant::now();
        *self.phase.lock().unwrap() = 2;
        let now = self.now_ms();
        for mut e in self.open_executions()? {
            if e.state == ExecState::Running {
                e.state = ExecState::Queued;
                e.interrupted += 1;
                e.updated_at_ms = now;
                self.commit(vec![
                    self.exec_record(&e)?,
                    self.ledger(
                        "execution.interrupted",
                        Some(&e.session_id),
                        json!({"execution_id": e.id, "interrupted": e.interrupted, "turn": e.turns}),
                    )?,
                ])?;
                rep.requeued_interrupted.push(e.id.clone());
            }
        }
        self.commit(vec![self.ledger(
            "startup.step",
            None,
            json!({"step": 2, "name": "load", "requeued": rep.requeued_interrupted}),
        )?])?;
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
        self.commit(vec![self.ledger(
            "startup.step",
            None,
            json!({"step": 3, "name": "spool", "drained": rep.spool_drained, "malformed": rep.spool_quarantined}),
        )?])?;
        step(3, "spool", t)?;

        // 4. reconcile
        let t = std::time::Instant::now();
        *self.phase.lock().unwrap() = 4;
        rep.reconcile = self.reconcile(evidence)?;
        self.commit(vec![self.ledger(
            "startup.step",
            None,
            json!({"step": 4, "name": "reconcile", "report": rep.reconcile}),
        )?])?;
        step(4, "reconcile", t)?;

        // 5. accept
        let t = std::time::Instant::now();
        *self.phase.lock().unwrap() = 5;
        self.commit(vec![self.ledger(
            "startup.step",
            None,
            json!({"step": 5, "name": "accepting"}),
        )?])?;
        step(5, "accepting", t)?;
        rep.elapsed_us = t0.elapsed().as_micros() as u64;
        Ok(rep)
    }
}

fn settle_reservation_in(b: &mut Budget, reservation_id: &str, actual: Option<u64>) {
    let Some(reserved) = b.reservations.remove(reservation_id) else {
        return;
    };
    b.reserved = b.reserved.saturating_sub(reserved);
    match actual {
        Some(u) => b.spent = b.spent.saturating_add(u),
        None => b.held_unknown = b.held_unknown.saturating_add(reserved),
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
fn decode_all<T: serde::de::DeserializeOwned>(rs: Vec<Record>) -> Result<Vec<T>> {
    rs.iter().map(|r| r.decode()).collect()
}
