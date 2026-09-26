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

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{bail, Result};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use serde_json::json;
use theseus_kernel::job::WrapperEvidence;
use theseus_kernel::*;
use theseus_store::{Engine, Store, WalConfig, WalStore};

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
    pub engine: Engine,
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
}

fn open_store(dir: &Path, p: &SimParams) -> Result<Arc<dyn Store>> {
    Ok(Arc::new(
        WalStore::open(
            &dir.join("store"),
            p.engine,
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
        default_budget: 100_000,
        control_reserve: 1_000,
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
    let kernel = Kernel::new(open_store(&dir, &p)?, clock.clone(), cfg(&p));
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
    };

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
            let k = Kernel::new(
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
            let budget = self.rng.random_range(5_000..200_000);
            let (_, e) = self.kernel.open_session(
                kind,
                vec![format!("root_{step}")],
                auth(),
                Some(budget),
                None,
            )?;
            self.execs.push(e.id.clone());
            if self.maybe_crash("after open_session")? {
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
        // Wake a waiting conversation with input.
        let waiting: Vec<_> = self
            .kernel
            .open_executions()?
            .into_iter()
            .filter(|e| e.state == ExecState::Waiting && matches!(e.wake, Some(Wake::Input)))
            .collect();
        if !waiting.is_empty() {
            let i = self.rng.random_range(0..waiting.len());
            self.kernel.wake_input(&waiting[i].id)?;
        }
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
            let mut prop = Proposal {
                tool: if self.rng.random_bool(0.5) {
                    "fake.slow".into()
                } else {
                    "fake.fast".into()
                },
                args: json!({"n": self.rng.random_range(0..1000)}),
                resource: None,
                policy_context: json!({}),
            };
            let (gate, _) = run_gate(&AllowAll, &mut prop, &auth());
            assert_eq!(gate, GateResult::Allow);
            let reserve = self.rng.random_range(0..3_000);
            let a = match self.kernel.plan_action(
                g,
                &prop,
                RetryClass::SafeToRepeat,
                Some(20_000),
                reserve,
            ) {
                Ok(a) => a,
                Err(err) => {
                    if matches!(
                        err.downcast_ref::<KernelError>(),
                        Some(KernelError::BudgetExhausted { .. })
                    ) {
                        // Terminal; the turn is over.
                        self.guards.remove(&exec_id);
                        return Ok(());
                    }
                    return Err(err);
                }
            };
            self.rep.actions += 1;
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
            let leftover = e.outstanding.clone();
            self.kill_jobs(&leftover)?;
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
            .filter(|e| !e.outstanding.is_empty() || e.state == ExecState::Queued)
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
                usage_units: Some(self.rng.random_range(0..1_000)),
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
            if b.spent + b.reserved + b.held_unknown > b.limit {
                bail!("{at}: {} budget over limit: {:?}", e.id, b);
            }
            let sum: u64 = b.reservations.values().sum();
            if sum != b.reserved {
                bail!(
                    "{at}: {} reservations {sum} != reserved {}",
                    e.id,
                    b.reserved
                );
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
