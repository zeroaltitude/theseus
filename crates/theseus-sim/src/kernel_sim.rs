//! The deterministic kernel simulator (spec §8, Part II M2 "Prove").
//!
//! One process, no network, a virtual clock, a real store in a temp dir, a
//! real spool, and a fake tool whose jobs finish after a scripted virtual
//! delay, never, twice, or after the execution that asked was cancelled. A
//! seeded RNG drives every choice, so a failing run is reproducible from its
//! seed. "Crash" means: drop the kernel (turn guards included), reopen the
//! store, run the five-step startup, optionally dying inside a startup step
//! and starting again.
//!
//! After every step the invariants below are checked over the whole store.
//! At the end the world is quiesced (every job delivered, reconciled) and the
//! terminal invariants are checked: no committed completion lost, no
//! execution left running, no action left open.
//!
//! Budgets are dollars (theseus-0sg). A call that does not fit asks the
//! operator and waits with the reason `budget`; the operator approves (the
//! spend resets to $0), declines (it keeps waiting), or sends new input (it
//! asks again). Executions stored with unit budgets are seeded before the
//! first startup and must come out in dollars. The budget invariants: nothing
//! is reserved past the limit; spend goes down only through an approved
//! reset, and every reset is a `budget.reset` row; one question is open per
//! execution, and a budget wait names its question; terminal stays terminal,
//! and nothing new ever ends `budget_exhausted`. A question carries the
//! proposal its answer binds, and any proposal an action keeps is the one it
//! was planned under (theseus-0g4). Half the actions are planned, authorized,
//! and dispatched in one frame (`plan_and_dispatch`, theseus-qa0); such an
//! action is never found planned or authorized, crash or no crash, and its
//! three transitions carry their times in order.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Result};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde_json::json;
use theseus_kernel::job::WrapperEvidence;
use theseus_kernel::*;
use theseus_store::{Store, WalConfig, WalStore};

#[derive(Debug, Clone)]
pub struct SimParams {
    pub seed: u64,
    pub steps: u32,
    pub sessions: u32,
    pub ceiling: u32,
    pub p_crash: f64,
    pub p_drop_notify: f64,
    pub p_dup: f64,
    pub p_lost_job: f64,
    pub p_cancel: f64,
    pub fsync: bool,
    pub verbose: bool,
}

#[derive(Debug, Default, serde::Serialize)]
pub struct SimReport {
    pub seed: u64,
    pub steps: u32,
    pub crashes: u32,
    pub startup_faults: u32,
    pub sessions: u32,
    pub turns: u64,
    pub actions: u64,
    pub completions_delivered: u64,
    pub duplicates: u64,
    pub notify_dropped: u64,
    pub lost_jobs: u64,
    pub cancels: u64,
    pub late_after_cancel: u64,
    pub unknowns: u64,
    pub resolved_unknowns: u64,
    pub quarantined: u64,
    pub budget_questions: u64,
    pub budget_resets: u64,
    pub budget_declines: u64,
    /// Actions planned, authorized, and dispatched in one frame.
    pub one_frame_dispatches: u64,
    pub legacy_migrated: u64,
    pub reconciles: u64,
    pub invariant_checks: u64,
    pub final_positions: u64,
    pub wall_ms: u64,
}

#[derive(Debug, Clone)]
struct Job {
    corr: String,
    /// Virtual time it finishes; None = lost (never finishes).
    finish_at: Option<u64>,
    outcome: Outcome,
    /// The result was made durable in the spool.
    spooled: bool,
    /// The spool file was removed after a settled frame.
    settled: bool,
    cancelled: bool,
    /// What its action reserved; the job costs at most this.
    reserved: Micros,
}

struct World {
    dir: PathBuf,
    clock: Arc<VirtualClock>,
    kernel: Kernel,
    spool: Spool,
    guards: HashMap<String, TurnGuard>,
    jobs: Vec<Job>,
    execs: Vec<String>,
    rng: StdRng,
    p: SimParams,
    rep: SimReport,
    last_reconcile_ms: u64,
    /// Each execution's (spent, resets) at the last check.
    budgets: HashMap<String, (Micros, u32)>,
    /// Resets the sim approved and the kernel accepted, per execution.
    resets_done: HashMap<String, u32>,
    /// Every execution seen terminal, and the state it ended in.
    terminal: HashMap<String, ExecState>,
    /// Actions `plan_and_dispatch` returned: each was committed in one frame.
    one_frame: HashSet<String>,
}

/// Executions stored with unit budgets before theseus-0sg, seeded into the
/// store before the first startup: (execution, session, state, extra JSON).
const LEGACY: &[(&str, &str, &str)] = &[
    (
        "exe_legacy_waiting",
        "ses_legacy_waiting",
        r#""state":"waiting","wake":{"on":"input"},"budget":{"limit":20000000,"spent":154321,"reserved":0,"held_unknown":0,"control_reserve":10000,"reservations":{}}"#,
    ),
    (
        "exe_legacy_running",
        "ses_legacy_running",
        r#""state":"running","budget":{"limit":1000000,"spent":5000,"reserved":133351,"held_unknown":2000,"control_reserve":10000,"reservations":{"rsv_old":133351}}"#,
    ),
    (
        "exe_legacy_exhausted",
        "ses_legacy_exhausted",
        r#""state":"budget_exhausted","ended_reason":"action provider.messages needs 172068 units, 112317 available","budget":{"limit":1000000,"spent":877683,"reserved":0,"held_unknown":0,"control_reserve":10000,"reservations":{}}"#,
    ),
];

/// What each legacy session's record says it spent (the core's lookup).
fn legacy_spend(session: &str) -> Micros {
    match session {
        "ses_legacy_waiting" => 20_000,
        "ses_legacy_running" => 5_000,
        "ses_legacy_exhausted" => 90_000,
        _ => 0,
    }
}

fn legacy_records() -> Result<Vec<theseus_store::NewRecord>> {
    LEGACY
        .iter()
        .map(|(id, session, rest)| {
            let json = format!(
                r#"{{"id":"{id}","schema":1,"session_id":"{session}","kind":"conversation","authority":{{"principal":"operator","ceilings":{{}}}},"outstanding":[],"queued_results":[],"turns":4,"interrupted":0,"resume_pending":false,"created_at_ms":1690000000000,"updated_at_ms":1690000000000,{rest}}}"#
            );
            let v: serde_json::Value = serde_json::from_str(&json)?;
            Ok(theseus_store::NewRecord::json(theseus_store::kinds::EXECUTION, Some(id), &v)?
                .scoped(session))
        })
        .collect()
}

fn new_kernel(store: Arc<dyn Store>, clock: Arc<VirtualClock>, c: KernelConfig) -> Kernel {
    Kernel::new(store, clock, c).with_legacy_spend(Arc::new(legacy_spend))
}

fn open_store(dir: &Path, p: &SimParams) -> Result<Arc<dyn Store>> {
    Ok(Arc::new(
        WalStore::open(
            &dir.join("store"),
            WalConfig {
                fsync: p.fsync,
                ..Default::default()
            },
        )?
        .with_checkpoint_every(500),
    ))
}

fn cfg(p: &SimParams) -> KernelConfig {
    KernelConfig {
        admission_ceiling: p.ceiling,
        default_deadline_ms: 30_000,
        spend_limit_micros: 100_000,
        confirm_ttl_ms: 60_000,
        heartbeat_ms: 60_000,
        fault_after_startup_step: None,
    }
}

fn auth() -> Authority {
    Authority {
        principal: "sim".into(),
        delegated_by: None,
        ceilings: BTreeMap::from([("shell".into(), "l1".into())]),
    }
}

pub fn run(p: SimParams) -> Result<SimReport> {
    let t0 = std::time::Instant::now();
    let tmp = tempfile::tempdir()?;
    let dir = tmp.path().to_path_buf();
    let clock = VirtualClock::new(1_700_000_000_000);
    let spool = Spool::open(&dir.join("spool"))?;
    let store = open_store(&dir, &p)?;
    store.append(&legacy_records()?)?;
    let kernel = new_kernel(store, clock.clone(), cfg(&p));
    kernel.startup(
        Some(&spool),
        &WrapperEvidence {
            spool: spool.clone(),
        },
    )?;
    let mut w = World {
        dir,
        clock,
        kernel,
        spool,
        guards: HashMap::new(),
        jobs: Vec::new(),
        execs: Vec::new(),
        rng: StdRng::seed_from_u64(p.seed),
        rep: SimReport {
            seed: p.seed,
            ..Default::default()
        },
        p,
        last_reconcile_ms: 0,
        budgets: HashMap::new(),
        resets_done: HashMap::new(),
        terminal: HashMap::new(),
        one_frame: HashSet::new(),
    };
    w.rep.legacy_migrated = w
        .kernel
        .executions()?
        .iter()
        .filter(|e| e.budget.units_before.is_some())
        .count() as u64;
    if w.rep.legacy_migrated != LEGACY.len() as u64 {
        bail!(
            "{} of {} unit-budget executions read after startup",
            w.rep.legacy_migrated,
            LEGACY.len()
        );
    }

    for step in 0..w.p.steps {
        w.step(step)?;
        w.check_invariants(&format!("after step {step}"))?;
    }
    w.quiesce()?;
    w.check_invariants("after quiesce")?;
    w.check_terminal()?;
    w.rep.steps = w.p.steps;
    w.rep.sessions = w.execs.len() as u32;
    w.rep.final_positions = w.kernel.store().last_position();
    w.rep.wall_ms = t0.elapsed().as_millis() as u64;
    Ok(w.rep)
}

impl World {
    fn now(&self) -> u64 {
        self.clock.now_ms()
    }

    fn chance(&mut self, p: f64) -> bool {
        self.rng.random_bool(p.clamp(0.0, 1.0))
    }

    /// A crash may happen between any two kernel frames.
    fn maybe_crash(&mut self, where_: &str) -> Result<bool> {
        if !self.chance(self.p.p_crash) {
            return Ok(false);
        }
        self.crash(where_)?;
        Ok(true)
    }

    fn crash(&mut self, where_: &str) -> Result<()> {
        self.rep.crashes += 1;
        if self.p.verbose {
            eprintln!("  crash {where_} at t={}", self.now());
        }
        // The process dies: guards vanish without ending turns.
        let guards: Vec<_> = self.guards.drain().map(|(_, g)| g).collect();
        for g in guards {
            std::mem::forget(g);
        }
        let old = std::mem::replace(
            &mut self.kernel,
            Kernel::new(
                Arc::new(NullStore),
                self.clock.clone(),
                KernelConfig::default(),
            ),
        );
        drop(old);
        // Sometimes the restart itself dies inside a startup step; try again.
        loop {
            let mut c = cfg(&self.p);
            if self.chance(0.3) {
                c.fault_after_startup_step = Some(self.rng.random_range(1..=4));
            }
            let k = new_kernel(
                open_store(&self.dir, &self.p)?,
                self.clock.clone(),
                c.clone(),
            );
            let ev = WrapperEvidence {
                spool: self.spool.clone(),
            };
            match k.startup(Some(&self.spool), &ev) {
                Ok(rep) => {
                    if self.p.verbose {
                        eprintln!(
                            "  startup: requeued {} drained {} reconcile {:?}",
                            rep.requeued_interrupted.len(),
                            rep.spool_drained,
                            rep.reconcile.marked_unknown.len()
                        );
                    }
                    self.rep.unknowns += rep.reconcile.marked_unknown.len() as u64;
                    self.rep.resolved_unknowns += rep.reconcile.resolved_unknown.len() as u64;
                    self.mark_settled_from_spool();
                    self.kernel = k;
                    break;
                }
                Err(e) => {
                    if c.fault_after_startup_step.is_none() {
                        return Err(e);
                    }
                    self.rep.startup_faults += 1;
                    drop(k);
                }
            }
        }
        Ok(())
    }

    /// After a drain, any job whose spool file is gone was settled by a frame.
    fn mark_settled_from_spool(&mut self) {
        for j in &mut self.jobs {
            if j.spooled && !j.settled && !self.spool.has_completion(&j.corr) {
                j.settled = true;
            }
        }
    }

    fn step(&mut self, step: u32) -> Result<()> {
        // Time passes.
        let dt = self.rng.random_range(0..3_000);
        self.clock.advance(dt);

        // Jobs that finished are delivered (or not).
        self.deliver_due()?;

        // Heartbeat.
        if self.now() - self.last_reconcile_ms >= 60_000 {
            self.last_reconcile_ms = self.now();
            let ev = WrapperEvidence {
                spool: self.spool.clone(),
            };
            let rep = self.kernel.reconcile(&ev)?;
            self.rep.reconciles += 1;
            self.rep.unknowns += rep.marked_unknown.len() as u64;
            self.rep.resolved_unknowns += rep.resolved_unknown.len() as u64;
            // Drain the spool as the heartbeat would.
            let drained = self.spool.drain()?;
            for (path, c) in drained.completions {
                self.kernel.accept_completion(&c)?;
                self.spool.remove(&path)?;
            }
            self.mark_settled_from_spool();
            if self.maybe_crash("after heartbeat")? {
                return Ok(());
            }
        }

        // One random operation.
        let roll = self.rng.random_range(0..100);
        if (self.execs.len() as u32) < self.p.sessions && roll < 15 {
            let kind = if self.chance(0.5) {
                SessionKind::Conversation
            } else {
                SessionKind::Task
            };
            // Small enough that a few calls reach it, so the budget wait runs.
            let budget = self.rng.random_range(1_000..15_000);
            // The product's path: the core keeps the session's own record,
            // and the kernel opens the session's one execution.
            let e = self.kernel.open_execution(
                &format!("ses_{step}"),
                kind,
                auth(),
                Some(budget),
                None,
            )?;
            self.execs.push(e.id.clone());
            if self.maybe_crash("after open_execution")? {
                return Ok(());
            }
            self.kernel.wake_input(&e.id)?;
            return Ok(());
        }
        if roll < 70 {
            return self.take_a_turn();
        }
        if roll < 70 + (self.p.p_cancel * 100.0) as i32 {
            return self.cancel_one();
        }
        if roll % 2 == 0 {
            return self.answer_budget();
        }
        // Wake a waiting conversation with input; on a budget wait, new
        // input is how its next call asks again.
        let waiting: Vec<_> = self
            .kernel
            .open_executions()?
            .into_iter()
            .filter(|e| {
                e.state == ExecState::Waiting
                    && matches!(e.wake, Some(Wake::Input) | Some(Wake::Budget { .. }))
            })
            .collect();
        if !waiting.is_empty() {
            let i = self.rng.random_range(0..waiting.len());
            self.kernel.wake_input(&waiting[i].id)?;
        }
        Ok(())
    }

    /// The operator answers an open budget question: approve (the spend
    /// resets to $0 and the execution continues) or decline (it waits on).
    fn answer_budget(&mut self) -> Result<()> {
        let asked: Vec<_> = self
            .kernel
            .open_executions()?
            .into_iter()
            .filter(|e| e.budget.question.is_some())
            .collect();
        if asked.is_empty() {
            return Ok(());
        }
        let e = &asked[self.rng.random_range(0..asked.len())];
        let q = e.budget.question.clone().unwrap();
        if self.chance(0.6) {
            self.kernel.reset_budget(&q, "sim")?;
            *self.resets_done.entry(e.id.clone()).or_default() += 1;
            self.rep.budget_resets += 1;
        } else {
            self.kernel.decline_action(&q, "sim", "not now")?;
            self.rep.budget_declines += 1;
        }
        self.maybe_crash("after a budget answer")?;
        Ok(())
    }

    fn take_a_turn(&mut self) -> Result<()> {
        let queued: Vec<_> = self
            .kernel
            .open_executions()?
            .into_iter()
            .filter(|e| e.state == ExecState::Queued)
            .collect();
        if queued.is_empty() {
            return Ok(());
        }
        let e = &queued[self.rng.random_range(0..queued.len())];
        let g = match self.kernel.admit(&e.id) {
            Ok(g) => g,
            Err(err) => {
                let k = err.downcast_ref::<KernelError>();
                if matches!(
                    k,
                    Some(KernelError::AdmissionFull { .. }) | Some(KernelError::TurnHeld { .. })
                ) {
                    return Ok(());
                }
                return Err(err);
            }
        };
        self.rep.turns += 1;
        let exec_id = e.id.clone();
        self.guards.insert(exec_id.clone(), g);
        if self.maybe_crash("after admit")? {
            return Ok(());
        }
        // Consume results.
        let g = self.guards.get(&exec_id).unwrap();
        let _results = self.kernel.take_results(g)?;
        if self.maybe_crash("after take_results")? {
            return Ok(());
        }
        // 0..=2 actions.
        let n_actions = self.rng.random_range(0..=2);
        let mut dispatched = Vec::new();
        for _ in 0..n_actions {
            let g = self.guards.get(&exec_id).unwrap();
            let prop = Proposal {
                tool: if self.rng.random_bool(0.5) {
                    "fake.slow".into()
                } else {
                    "fake.fast".into()
                },
                args: json!({"n": self.rng.random_range(0..1000)}),
                resource: None,
                policy_context: json!({}),
            };
            let reserve = self.rng.random_range(0..3_000);
            // Half the time, planned, authorized, and dispatched in one frame.
            let one_frame = self.rng.random_bool(0.5);
            let planned = if one_frame {
                self.kernel.plan_and_dispatch(
                    g,
                    &prop,
                    RetryClass::SafeToRepeat,
                    Some(20_000),
                    reserve,
                    |_| Ok(vec![]),
                )
            } else {
                self.kernel
                    .plan_action(g, &prop, RetryClass::SafeToRepeat, Some(20_000), reserve)
            };
            let a = match planned {
                Ok(a) => a,
                Err(err) => {
                    if !matches!(
                        err.downcast_ref::<KernelError>(),
                        Some(KernelError::OverBudget { .. })
                    ) {
                        return Err(err);
                    }
                    // Over budget: ask the operator, and park on the question.
                    let q = self.kernel.ask_budget(g, reserve)?;
                    self.rep.budget_questions += 1;
                    if self.maybe_crash("after ask_budget")? {
                        return Ok(());
                    }
                    let g = self.guards.remove(&exec_id).unwrap();
                    self.kernel.end_turn(
                        g,
                        TurnEnd::Wait {
                            wake: Wake::Budget {
                                correlation_id: q.correlation_id,
                            },
                        },
                    )?;
                    return Ok(());
                }
            };
            self.rep.actions += 1;
            if one_frame {
                self.rep.one_frame_dispatches += 1;
                self.one_frame.insert(a.correlation_id.clone());
            } else {
                if self.maybe_crash("after plan")? {
                    return Ok(());
                }
                self.kernel.authorize(&a.correlation_id, &prop, None)?;
                if self.maybe_crash("after authorize")? {
                    return Ok(());
                }
                match self.kernel.dispatch(&a.correlation_id, None) {
                    Ok(_) => {}
                    Err(err) => {
                        if matches!(
                            err.downcast_ref::<KernelError>(),
                            Some(KernelError::NotRunnable { .. })
                        ) {
                            self.guards.remove(&exec_id);
                            return Ok(());
                        }
                        return Err(err);
                    }
                }
            }
            // The fake tool starts a job.
            let lost = self.chance(self.p.p_lost_job);
            if lost {
                self.rep.lost_jobs += 1;
            }
            let delay = if prop.tool == "fake.fast" {
                self.rng.random_range(0..500)
            } else {
                self.rng.random_range(500..40_000) // sometimes past the 20 s deadline
            };
            self.jobs.push(Job {
                corr: a.correlation_id.clone(),
                finish_at: if lost { None } else { Some(self.now() + delay) },
                outcome: if self.rng.random_bool(0.8) {
                    Outcome::Succeeded
                } else {
                    Outcome::Failed
                },
                spooled: false,
                settled: false,
                cancelled: false,
                reserved: reserve,
            });
            dispatched.push(a.correlation_id);
            if self.maybe_crash("after dispatch")? {
                return Ok(());
            }
        }
        // End the turn.
        let g = self.guards.remove(&exec_id).unwrap();
        let end = if !dispatched.is_empty() {
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: dispatched,
                },
            }
        } else {
            match self.rng.random_range(0..10) {
                0 => TurnEnd::Complete {
                    reason: "sim done".into(),
                },
                1..=3 => TurnEnd::Wait {
                    wake: Wake::DueAt {
                        at_ms: self.now() + self.rng.random_range(1_000..120_000),
                    },
                },
                4..=6 => TurnEnd::Wait { wake: Wake::Input },
                _ => TurnEnd::Requeue,
            }
        };
        let e = self.kernel.end_turn(g, end)?;
        if e.state.is_terminal() {
            self.kill_jobs(&e.outstanding)?;
        }
        Ok(())
    }

    /// Terminate the backends of cancelled actions: 70% die now (verified),
    /// 30% cannot be reached (unsupported) and may still finish late.
    fn kill_jobs(&mut self, corrs: &[String]) -> Result<()> {
        for corr in corrs {
            if let Some(j) = self.jobs.iter_mut().find(|j| &j.corr == corr) {
                j.cancelled = true;
                if self.rng.random_bool(0.7) {
                    j.finish_at = None;
                    self.kernel.cancel_acknowledged(corr)?;
                    self.kernel.cancel_verified(corr)?;
                } else {
                    self.kernel.cancel_unsupported(corr)?;
                }
            }
        }
        Ok(())
    }

    fn cancel_one(&mut self) -> Result<()> {
        let open: Vec<_> = self
            .kernel
            .open_executions()?
            .into_iter()
            .filter(|e| {
                !e.outstanding.is_empty()
                    || e.state == ExecState::Queued
                    || matches!(e.wake, Some(Wake::Budget { .. }))
            })
            .collect();
        if open.is_empty() {
            return Ok(());
        }
        let e = &open[self.rng.random_range(0..open.len())];
        let to_kill = self.kernel.cancel_execution(&e.id, "sim")?;
        self.rep.cancels += 1;
        if self.maybe_crash("after cancel")? {
            // Cancel already durable; the jobs get verified after restart below.
        }
        self.kill_jobs(&to_kill)?;
        Ok(())
    }

    /// Deliver every job whose virtual finish time has passed.
    fn deliver_due(&mut self) -> Result<()> {
        let now = self.now();
        let due: Vec<usize> = self
            .jobs
            .iter()
            .enumerate()
            .filter(|(_, j)| !j.spooled && j.finish_at.is_some_and(|t| t <= now))
            .map(|(i, _)| i)
            .collect();
        for i in due {
            let j = self.jobs[i].clone();
            let c = Completion {
                correlation_id: j.corr.clone(),
                outcome: j.outcome,
                result_ref: Some(format!("node_{}", j.corr)),
                external_op_id: None,
                started_at_ms: now.saturating_sub(100),
                finished_at_ms: now,
                producer: "sim-wrapper".into(),
                signature: None,
                cost_micros: Some(self.rng.random_range(0..=j.reserved)),
                detail: None,
            };
            // The wrapper always spools first (durable), then notifies.
            self.spool.write(&c)?;
            self.jobs[i].spooled = true;
            if self.chance(self.p.p_drop_notify) {
                // Notify lost: the heartbeat's drain finds it later.
                self.rep.notify_dropped += 1;
                continue;
            }
            let path = self.spool.completion_path(&j.corr);
            let acc = self.kernel.accept_completion(&c)?;
            self.rep.completions_delivered += 1;
            match &acc {
                Accepted::LateAfterCancel { .. } => self.rep.late_after_cancel += 1,
                Accepted::ResolvedUnknown { .. } => self.rep.resolved_unknowns += 1,
                Accepted::Quarantined { .. } => self.rep.quarantined += 1,
                _ => {}
            }
            if self.maybe_crash("after accept before spool remove")? {
                return Ok(()); // redelivery from the spool at startup is a no-op
            }
            self.spool.remove(&path)?;
            self.jobs[i].settled = true;
            if self.chance(self.p.p_dup) {
                let acc = self.kernel.accept_completion(&c)?;
                self.rep.duplicates += 1;
                if !matches!(
                    acc,
                    Accepted::DuplicateNoop { .. } | Accepted::LateAfterCancel { .. }
                ) {
                    bail!(
                        "duplicate completion for {} was not a no-op: {acc:?}",
                        j.corr
                    );
                }
            }
        }
        Ok(())
    }

    /// Finish the world: every finishable job finishes, everything drains and
    /// reconciles until nothing is open.
    fn quiesce(&mut self) -> Result<()> {
        // Release any held turn as the harness would at shutdown.
        let held: Vec<_> = self.guards.drain().map(|(_, g)| g).collect();
        for g in held {
            self.kernel.end_turn(g, TurnEnd::Requeue)?;
        }
        for _ in 0..10 {
            self.clock.advance(120_000);
            self.deliver_due()?;
            let drained = self.spool.drain()?;
            for (path, c) in drained.completions {
                self.kernel.accept_completion(&c)?;
                self.spool.remove(&path)?;
            }
            self.mark_settled_from_spool();
            let ev = WrapperEvidence {
                spool: self.spool.clone(),
            };
            let rep = self.kernel.reconcile(&ev)?;
            self.rep.reconciles += 1;
            self.rep.unknowns += rep.marked_unknown.len() as u64;
            self.rep.resolved_unknowns += rep.resolved_unknown.len() as u64;
        }
        Ok(())
    }

    fn check_invariants(&mut self, at: &str) -> Result<()> {
        self.rep.invariant_checks += 1;
        let execs = self.kernel.executions()?;
        let actions = self.kernel.actions()?;
        let by_corr: HashMap<_, _> = actions
            .iter()
            .map(|a| (a.correlation_id.clone(), a))
            .collect();
        let stats = self.kernel.stats()?;
        let running = execs
            .iter()
            .filter(|e| e.state == ExecState::Running)
            .count() as u32;
        if running != stats.turns_held {
            bail!(
                "{at}: {running} executions Running but {} turns held",
                stats.turns_held
            );
        }
        if running > self.p.ceiling {
            bail!("{at}: {running} running > ceiling {}", self.p.ceiling);
        }
        for e in &execs {
            match e.state {
                ExecState::Waiting if e.wake.is_none() => {
                    bail!("{at}: {} waiting with no wake", e.id)
                }
                ExecState::Queued | ExecState::Running if e.wake.is_some() => {
                    bail!("{at}: {} {:?} but has a wake", e.id, e.state)
                }
                _ => {}
            }
            let b = &e.budget;
            // Nothing is reserved past the limit (a job costs at most what it reserved).
            if b.spent_micros + b.reserved_micros + b.held_unknown_micros > b.limit_micros {
                bail!("{at}: {} budget over limit: {:?}", e.id, b);
            }
            let sum: u64 = b.reservations.values().sum();
            if sum != b.reserved_micros {
                bail!(
                    "{at}: {} reservations {sum} != reserved {}",
                    e.id,
                    b.reserved_micros
                );
            }
            if e.schema < SCHEMA {
                bail!("{at}: {} still has a unit budget after startup", e.id);
            }
            // Spend goes down only through a reset the sim approved.
            let approved = self.resets_done.get(&e.id).copied().unwrap_or(0);
            if b.resets != approved {
                bail!(
                    "{at}: {} shows {} resets, but {approved} were approved",
                    e.id,
                    b.resets
                );
            }
            if let Some(&(spent, resets)) = self.budgets.get(&e.id) {
                if b.spent_micros < spent && b.resets == resets {
                    bail!(
                        "{at}: {} spend went down from {spent} to {} with no reset",
                        e.id,
                        b.spent_micros
                    );
                }
            }
            self.budgets
                .insert(e.id.clone(), (b.spent_micros, b.resets));
            // Terminal stays terminal, and nothing new ends budget_exhausted.
            if let Some(st) = self.terminal.get(&e.id) {
                if e.state != *st {
                    bail!("{at}: {} was {st:?} and is now {:?}", e.id, e.state);
                }
            }
            if e.state.is_terminal() {
                self.terminal.insert(e.id.clone(), e.state);
            }
            if e.state == ExecState::BudgetExhausted && !LEGACY.iter().any(|(id, _, _)| *id == e.id)
            {
                bail!(
                    "{at}: {} ended budget_exhausted; a budget waits instead",
                    e.id
                );
            }
            // One question open at a time, and a budget wait names its own.
            let open_questions: Vec<&&Action> = by_corr
                .values()
                .filter(|a| {
                    a.execution_id == e.id
                        && a.tool == BUDGET_TOOL
                        && a.state == ActionState::Planned
                })
                .collect();
            match (&b.question, open_questions.as_slice()) {
                (None, []) => {}
                (Some(q), [a]) if &a.correlation_id == q && !e.state.is_terminal() => {}
                (q, open) => bail!(
                    "{at}: {} question {q:?} but open budget actions {:?}",
                    e.id,
                    open.iter().map(|a| &a.correlation_id).collect::<Vec<_>>()
                ),
            }
            if let (ExecState::Waiting, Some(Wake::Budget { correlation_id })) = (e.state, &e.wake)
            {
                let a = by_corr.get(correlation_id).ok_or_else(|| {
                    anyhow::anyhow!(
                        "{at}: {} waits on budget question {correlation_id}, which does not exist",
                        e.id
                    )
                })?;
                if a.tool != BUDGET_TOOL
                    || a.execution_id != e.id
                    || !matches!(a.state, ActionState::Planned | ActionState::Cancelled)
                {
                    bail!(
                        "{at}: {} waits on {correlation_id}, a {} {:?} of {}",
                        e.id,
                        a.tool,
                        a.state,
                        a.execution_id
                    );
                }
            }
            for c in &e.outstanding {
                let a = by_corr
                    .get(c)
                    .ok_or_else(|| anyhow::anyhow!("{at}: outstanding {c} has no action"))?;
                if a.execution_id != e.id {
                    bail!("{at}: outstanding {c} belongs to {}", a.execution_id);
                }
                if a.state.is_settled() || a.state == ActionState::OutcomeUnknown {
                    bail!("{at}: outstanding {c} is {:?}", a.state);
                }
            }
            let seen: BTreeSet<_> = e.queued_results.iter().collect();
            if seen.len() != e.queued_results.len() {
                bail!("{at}: {} has duplicate queued results", e.id);
            }
            for c in &e.queued_results {
                let a = by_corr
                    .get(c)
                    .ok_or_else(|| anyhow::anyhow!("{at}: queued result {c} has no action"))?;
                if matches!(a.state, ActionState::Planned | ActionState::Authorized) {
                    bail!("{at}: queued result {c} never dispatched ({:?})", a.state);
                }
            }
            if e.state.is_terminal() && e.state != ExecState::BudgetExhausted {
                // Terminal with outstanding work is only ever "cancel in flight".
                for c in &e.outstanding {
                    let a = by_corr[c];
                    if a.cancel.is_none() {
                        bail!(
                            "{at}: {} is {:?} with outstanding {c} and no cancel requested",
                            e.id,
                            e.state
                        );
                    }
                }
            }
        }
        for a in &actions {
            if a.state == ActionState::Dispatched {
                let e = execs
                    .iter()
                    .find(|e| e.id == a.execution_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!("{at}: action {} has no execution", a.correlation_id)
                    })?;
                if !e.outstanding.contains(&a.correlation_id) && !e.state.is_terminal() {
                    bail!(
                        "{at}: dispatched {} not in {}'s outstanding",
                        a.correlation_id,
                        e.id
                    );
                }
            }
            if a.state.is_settled() && a.settled_at_ms.is_none() {
                bail!("{at}: {} settled without settled_at", a.correlation_id);
            }
            // An action that waits for the operator carries the proposal its
            // confirm binds, and a kept proposal is the one planned (theseus-0g4).
            match &a.proposal {
                Some(p) if digest_proposal(p) != a.args_digest => {
                    bail!(
                        "{at}: {} keeps a proposal it was not planned under",
                        a.correlation_id
                    )
                }
                None if a.tool == BUDGET_TOOL => {
                    bail!(
                        "{at}: budget question {} keeps no proposal",
                        a.correlation_id
                    )
                }
                _ => {}
            }
        }
        // An action planned in one frame with its authorization and dispatch
        // (theseus-qa0) is there after any crash, and was never left planned
        // or authorized: its three transitions were one commit, in order.
        for c in &self.one_frame {
            let a = by_corr
                .get(c)
                .ok_or_else(|| anyhow::anyhow!("{at}: one-frame action {c} is gone"))?;
            if matches!(a.state, ActionState::Planned | ActionState::Authorized) {
                bail!("{at}: one-frame action {c} is {:?}", a.state);
            }
            match (a.authorized_at_ms, a.dispatched_at_ms) {
                (Some(au), Some(d)) if a.planned_at_ms <= au && au <= d => {}
                times => bail!(
                    "{at}: one-frame action {c} planned at {} with {times:?}",
                    a.planned_at_ms
                ),
            }
        }
        // Quarantine only ever holds ids we never minted.
        for q in self.kernel.quarantined()? {
            if by_corr.contains_key(&q.correlation_id) {
                bail!(
                    "{at}: quarantined a real correlation id {}",
                    q.correlation_id
                );
            }
        }
        Ok(())
    }

    fn check_terminal(&self) -> Result<()> {
        let actions = self.kernel.actions()?;
        let by_corr: HashMap<_, _> = actions
            .iter()
            .map(|a| (a.correlation_id.clone(), a))
            .collect();
        // Every completion the wrapper made durable settled its action, or the
        // action was cancelled first (and the late result is recorded).
        for j in &self.jobs {
            if !j.spooled {
                continue;
            }
            let a = by_corr[&j.corr];
            match a.state {
                ActionState::Succeeded | ActionState::Failed => {
                    if j.cancelled && a.resolution.is_none() && a.cancel.is_some() {
                        // settled before the cancel reached it; fine
                    }
                }
                ActionState::Cancelled => {
                    if !j.cancelled {
                        bail!("{} cancelled but the sim never cancelled it", j.corr);
                    }
                    if a.resolution.is_none() {
                        bail!(
                            "{} spooled a result after cancel but no late resolution recorded",
                            j.corr
                        );
                    }
                }
                other => bail!(
                    "committed completion for {} left action {:?} (spool file present: {})",
                    j.corr,
                    other,
                    self.spool.has_completion(&j.corr)
                ),
            }
        }
        // Lost jobs (never finished) must all be unknown or cancelled, never
        // silently succeeded; and nothing is left dispatched.
        for j in &self.jobs {
            if j.finish_at.is_none() && !j.spooled {
                let a = by_corr[&j.corr];
                match a.state {
                    ActionState::OutcomeUnknown | ActionState::Cancelled => {}
                    other => bail!("lost job {} ended as {:?}", j.corr, other),
                }
            }
        }
        for a in &actions {
            if a.state == ActionState::Dispatched {
                bail!("action {} still dispatched after quiesce", a.correlation_id);
            }
        }
        if self.kernel.stats()?.turns_held != 0 {
            bail!("turns still held after quiesce");
        }
        // Every reset is one `budget.reset` row, in the execution's session.
        for e in self.kernel.executions()? {
            let rows = self
                .kernel
                .store()
                .scan_scope(&e.session_id, 0, usize::MAX)?
                .into_iter()
                .filter(|r| r.kind == theseus_store::kinds::LEDGER)
                .filter_map(|r| r.decode::<LedgerRow>().ok())
                .filter(|r| r.kind == "budget.reset" && r.data["execution_id"] == e.id.as_str())
                .count() as u32;
            if rows != e.budget.resets {
                bail!(
                    "{} has {} resets but {rows} budget.reset rows",
                    e.id,
                    e.budget.resets
                );
            }
        }
        Ok(())
    }
}

/// A store that refuses everything; placeholder while the world is "down".
struct NullStore;
impl Store for NullStore {
    fn append(&self, _: &[theseus_store::NewRecord]) -> Result<Vec<u64>> {
        bail!("down")
    }
    fn get(&self, _: u64) -> Result<Option<theseus_store::Record>> {
        bail!("down")
    }
    fn scan(&self, _: u64, _: Option<u64>, _: usize) -> Result<Vec<theseus_store::Record>> {
        bail!("down")
    }
    fn latest_by_key(&self, _: u16, _: &str) -> Result<Option<theseus_store::Record>> {
        bail!("down")
    }
    fn latest_of_kind(&self, _: u16) -> Result<Vec<theseus_store::Record>> {
        bail!("down")
    }
    fn tail_of_kind(&self, _: u16, _: usize) -> Result<Vec<theseus_store::Record>> {
        bail!("down")
    }
    fn count_of_kind(&self, _: u16) -> Result<u64> {
        bail!("down")
    }
    fn scan_scope(&self, _: &str, _: u64, _: usize) -> Result<Vec<theseus_store::Record>> {
        bail!("down")
    }
    fn count_in_scope(&self, _: &str) -> Result<u64> {
        bail!("down")
    }
    fn last_position(&self) -> u64 {
        0
    }
    fn checkpoint(&self) -> Result<u64> {
        bail!("down")
    }
    fn stats(&self) -> Result<theseus_store::StoreStats> {
        bail!("down")
    }
}
