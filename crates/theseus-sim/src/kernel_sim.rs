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
//! was planned under (theseus-0g4).
//!
//! The limit moves (theseus-3pj). Half the executions open with the config's
//! limit and follow it; the rest name their own, which never changes. Now and
//! then the operator changes the config's limit and the process restarts onto
//! it; a third of all starts serve from a copy the vault has not confirmed,
//! and the vault's word then applies the limit (`follow_spend_limit`). Every
//! open execution that follows the config has the config's limit after every
//! step, and each change of an execution's limit is a `budget.limit_changed`
//! row from the old limit to the new. A raise leaves none of them waiting on
//! its budget. A lower limit may put a budget over it, but only a lower limit
//! does, and nothing new is reserved while it is over. Half the actions are planned, authorized,
//! and dispatched in one frame (`plan_and_dispatch`, theseus-qa0); such an
//! action is never found planned or authorized, crash or no crash, and its
//! three transitions carry their times in order.
//!
//! A quarter of the turns run a batch, as a response's calls run together
//! (theseus-a60): 3 to 6 actions dispatched back to back, one frame each,
//! before any completes. The in-process ones then complete in the turn in a
//! random order, and the rest are jobs. A crash may come after the batch is
//! dispatched or between two of its completions; an in-process call it
//! interrupts has no evidence, so it must end unknown or cancelled, never
//! lost or left dispatched, and every other invariant holds for each action.
//!
//! A share of the turns is raced (theseus-id9): a second OS thread drives the
//! same execution while the turn commits, with a cancel, a job's completion
//! arriving, the turn's own calls finishing on that thread, a wake, input, and
//! the heartbeat's reconcile, in an order the seed picks and a timing the OS
//! picks. A crash may stop the turn anywhere in it. Every invariant holds
//! after each race, and one more, read from the ledger in WAL order: an
//! execution that was cancelled never runs again. After its
//! `execution.cancelled` row, no turn of it starts and no action of it is
//! planned, and it is `cancelled` from then on. The seed fixes what each
//! thread does, not how they interleave, so a run with races reproduces from
//! its seed only up to its first race; `--p-race 0` is the sim as it was, one
//! thread, reproducible throughout.
//!
//! Some turns ask the operator about a call and park on the answer
//! (theseus-w98); the answer is a decline or new input in its place, and a
//! cancel may come first. One more invariant: an execution that has ended
//! has no action planned or authorized. A cancel settles everything its
//! execution planned and never sent, and so does a turn that ends it, so
//! nothing of it counts as waiting, and nothing of it can still run.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Barrier};

use anyhow::{bail, Result};
use rand::rngs::StdRng;
use rand::seq::SliceRandom;
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
    /// Share of the turns raced by a second OS thread on the same execution.
    pub p_race: f64,
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
    /// theseus-w98: calls that waited for the operator, those declined (or
    /// superseded), and the actions a cancel ended before they were sent.
    pub asked: u64,
    pub asked_declined: u64,
    pub ended_unsent: u64,
    /// Actions planned, authorized, and dispatched in one frame.
    pub one_frame_dispatches: u64,
    /// theseus-l6y: actions authorized and dispatched in one frame; turns an
    /// input woke and admitted in one frame; results a turn read itself (a
    /// batch's in-process calls, never queued); and batch turns that faulted
    /// after settling some, each of which must have woken its execution.
    pub authorized_and_dispatched: u64,
    pub input_admits: u64,
    pub own_results: u64,
    pub faults: u64,
    pub fault_wakes: u64,
    /// Turns that dispatched several actions at once (theseus-a60), their
    /// actions, and the crashes that came inside one.
    pub batches: u64,
    pub batch_actions: u64,
    pub batch_crashes: u64,
    /// Turns raced by a second OS thread (theseus-id9), the operations that
    /// thread ran, and the crashes that stopped a raced turn.
    pub races: u64,
    pub race_ops: u64,
    pub race_crashes: u64,
    /// Kernel transactions the racing thread ran, two transitions in one
    /// frame each (theseus-0owd), and answers the operator gave, each one
    /// frame with its wake (theseus-jj9f).
    pub race_frames: u64,
    pub one_frame_answers: u64,
    pub legacy_migrated: u64,
    /// Restarts onto a changed spend limit (theseus-3pj), and how many
    /// raised it; the open executions that took a new limit; the budget waits
    /// a raise let proceed; and the starts from an unconfirmed copy, whose
    /// limit the vault's word applied after startup.
    pub limit_changes: u64,
    pub limit_raises: u64,
    pub limits_followed: u64,
    pub limit_proceeds: u64,
    pub confirmed_after_startup: u64,
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
    /// Actions `authorize_and_dispatch` returned (theseus-l6y): authorized
    /// and dispatched in one frame, so never found authorized.
    two_in_one: HashSet<String>,
    /// Results a turn's view settled for its own execution (theseus-l6y):
    /// the turn read them, so none is ever queued.
    own: HashSet<String>,
    /// Executions with an `execution.cancelled` row, and the last position
    /// the invariant check has read the ledger to.
    cancelled: HashSet<String>,
    ledger_read_to: u64,
    /// The config's spend limit, which executions opened without one of
    /// their own follow (theseus-3pj). It changes only across a restart.
    limit: Micros,
    /// Executions opened with a limit of their own, and that limit.
    pinned: HashMap<String, Micros>,
    /// Each execution's limit at the last check.
    limits: HashMap<String, Micros>,
    /// Executions over their limit at the last check (a lower limit came
    /// after their spend), and the reservations they held then.
    over: HashMap<String, BTreeSet<String>>,
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

/// The store as the daemon opens it: its index keeps the kernel's terms
/// (theseus-lv2), which `check_terms` holds to a full read.
fn open_store(dir: &Path, p: &SimParams) -> Result<Arc<dyn Store>> {
    Ok(Arc::new(
        WalStore::open_projected(
            &dir.join("store"),
            WalConfig {
                fsync: p.fsync,
                ..Default::default()
            },
            &theseus_kernel::terms::PROJECTION,
        )?
        .with_checkpoint_every(500),
    ))
}

/// The config's limit when a run starts: small, as the executions' own
/// limits are, so that a few calls reach it.
const FIRST_LIMIT: Micros = 4_000;

fn cfg(p: &SimParams, spend_limit_micros: Micros) -> KernelConfig {
    KernelConfig {
        admission_ceiling: p.ceiling,
        default_deadline_ms: 30_000,
        spend_limit_micros,
        confirm_ttl_ms: 60_000,
        heartbeat_ms: 60_000,
        fault_after_startup_step: None,
        unconfirmed_config: false,
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
    let kernel = new_kernel(store, clock.clone(), cfg(&p, FIRST_LIMIT));
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
        two_in_one: HashSet::new(),
        own: HashSet::new(),
        cancelled: HashSet::new(),
        ledger_read_to: 0,
        limit: FIRST_LIMIT,
        pinned: HashMap::new(),
        limits: HashMap::new(),
        over: HashMap::new(),
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

    // The first check reads every execution as startup left it: the limits
    // a later change is measured from.
    w.check_invariants("after the first startup")?;
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
        self.restart()
    }

    /// The process is gone, and a new one starts under `self.limit`. A start
    /// may die inside a startup step, and is tried again. A third of the
    /// starts serve from a copy of the config the vault has not confirmed
    /// (theseus-2fo): their startup leaves every limit as it was, and the
    /// vault's word then gives the open executions the config's
    /// (`follow_spend_limit`).
    fn restart(&mut self) -> Result<()> {
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
            let mut c = cfg(&self.p, self.limit);
            c.unconfirmed_config = self.chance(0.3);
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
                    let followed = if c.unconfirmed_config {
                        if !rep.limits_followed.is_empty() {
                            bail!(
                                "startup under an unconfirmed copy changed {} limits",
                                rep.limits_followed.len()
                            );
                        }
                        self.rep.confirmed_after_startup += 1;
                        k.follow_spend_limit()?
                    } else {
                        rep.limits_followed
                    };
                    self.rep.limits_followed += followed.len() as u64;
                    self.rep.limit_proceeds +=
                        followed.iter().filter(|f| f.proceeds).count() as u64;
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
            // Small enough that a few calls reach it, so the budget wait
            // runs. Half the executions follow the config's limit
            // (theseus-3pj); the rest name a limit of their own.
            let own = self
                .chance(0.5)
                .then(|| self.rng.random_range(1_000..15_000));
            // The product's path: the core keeps the session's own record,
            // and the kernel opens the session's one execution.
            let e = self
                .kernel
                .open_execution(&format!("ses_{step}"), kind, auth(), own, None)?;
            if let Some(own) = own {
                self.pinned.insert(e.id.clone(), own);
            }
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
        if roll >= 96 {
            return self.change_limit();
        }
        if roll < 70 + (self.p.p_cancel * 100.0) as i32 {
            return self.cancel_one();
        }
        if roll % 2 == 0 {
            return if self.chance(0.5) {
                self.answer_budget()
            } else {
                self.answer_confirm()
            };
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

    /// The operator changes `[kernel] spend_limit_usd`, and the daemon
    /// restarts onto it (theseus-3pj): every open execution that follows the
    /// config takes the new limit. A raise lets each one waiting at its old
    /// limit proceed, so none still waits on its budget; a lower limit
    /// refuses the next reservation that does not fit.
    fn change_limit(&mut self) -> Result<()> {
        // Most often a raise when one of them waits at its limit, so the
        // raise's wake runs often; otherwise anywhere in the range.
        let at_limit = self.kernel.open_executions()?.iter().any(|e| {
            !e.budget.pinned
                && (matches!(e.wake, Some(Wake::Budget { .. })) || e.budget.question.is_some())
        });
        let mut to = if at_limit && self.chance(0.7) {
            self.limit + self.rng.random_range(1_000..6_000)
        } else {
            self.rng.random_range(1_500..8_000)
        };
        if to == self.limit {
            to += 1_000;
        }
        let raised = to > self.limit;
        self.limit = to;
        self.rep.limit_changes += 1;
        self.rep.limit_raises += u64::from(raised);
        if self.p.verbose {
            eprintln!("  restart onto a spend limit of {to} at t={}", self.now());
        }
        self.restart()?;
        if raised {
            for e in self.kernel.open_executions()? {
                if !e.budget.pinned
                    && (matches!(e.wake, Some(Wake::Budget { .. })) || e.budget.question.is_some())
                {
                    bail!(
                        "a raised limit left {} waiting on its budget ({:?}, question {:?})",
                        e.id,
                        e.wake,
                        e.budget.question
                    );
                }
            }
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
        let open = self.kernel.open_executions()?;
        // An input's turn, a third of the time (theseus-l6y): an execution
        // waiting on input is woken and admitted in one frame.
        let input: Vec<String> = open
            .iter()
            .filter(|e| e.state == ExecState::Waiting && matches!(e.wake, Some(Wake::Input)))
            .map(|e| e.id.clone())
            .collect();
        let queued: Vec<String> = open
            .iter()
            .filter(|e| e.state == ExecState::Queued)
            .map(|e| e.id.clone())
            .collect();
        let by_input = !input.is_empty() && (queued.is_empty() || self.chance(0.3));
        let exec_id = if by_input {
            input[self.rng.random_range(0..input.len())].clone()
        } else if queued.is_empty() {
            return Ok(());
        } else {
            queued[self.rng.random_range(0..queued.len())].clone()
        };
        if !by_input && self.chance(self.p.p_race) {
            return self.race_a_turn(&exec_id);
        }
        let admitted = if by_input {
            self.kernel.admit_input(&exec_id)
        } else {
            self.kernel.admit(&exec_id)
        };
        let g = match admitted {
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
        self.rep.input_admits += u64::from(by_input);
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
        if self.chance(0.25) {
            return self.take_a_batch(&exec_id);
        }
        // A tenth of the rest ask the operator about a call and park on the
        // answer (theseus-w98): a decline, new input, or a cancel ends it.
        if self.chance(0.1) {
            return self.ask_the_operator(&exec_id);
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
                // Half the time authorized and dispatched in one frame, as
                // a call the operator confirmed is (theseus-l6y).
                let dispatched = if self.rng.random_bool(0.5) {
                    match self
                        .kernel
                        .authorize_and_dispatch(&a.correlation_id, &prop, None, None)
                    {
                        Ok(Ok(d)) => {
                            self.rep.authorized_and_dispatched += 1;
                            self.two_in_one.insert(a.correlation_id.clone());
                            Ok(d)
                        }
                        Ok(Err(why)) => bail!("the sim's authorization was refused: {why:#}"),
                        // A cancel that landed first, as `dispatch` says it.
                        Err(err) => Err(err),
                    }
                } else {
                    self.kernel.authorize(&a.correlation_id, &prop, None)?;
                    if self.maybe_crash("after authorize")? {
                        return Ok(());
                    }
                    self.kernel.dispatch(&a.correlation_id, None)
                };
                match dispatched {
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

    /// A turn that asks the operator about a call and parks on the answer
    /// (theseus-w98): the call keeps its proposal and reserves nothing, and
    /// the execution waits on the confirm. A crash may come between the two.
    fn ask_the_operator(&mut self, exec_id: &str) -> Result<()> {
        let prop = Proposal {
            tool: "fake.ask".into(),
            args: json!({"n": self.rng.random_range(0..1000)}),
            resource: None,
            policy_context: json!({}),
        };
        let g = self.guards.get(exec_id).unwrap();
        let a = self.kernel.plan_confirm_with(
            g,
            &prop,
            RetryClass::NonRepeatable,
            Some(20_000),
            |_| Ok(vec![]),
        )?;
        self.rep.actions += 1;
        self.rep.asked += 1;
        if self.maybe_crash("after plan_confirm")? {
            return Ok(());
        }
        let g = self.guards.remove(exec_id).unwrap();
        self.kernel.end_turn(
            g,
            TurnEnd::Wait {
                wake: Wake::Confirm {
                    confirm_id: a.correlation_id,
                },
            },
        )?;
        Ok(())
    }

    /// The operator answers a call that waits (theseus-w98): no, or new
    /// input in its place, and the execution wakes to read it. Approvals are
    /// the core's (`confirm_action`); a cancel is `cancel_one`'s.
    fn answer_confirm(&mut self) -> Result<()> {
        let waiting: Vec<(String, String)> = self
            .kernel
            .open_executions()?
            .into_iter()
            .filter_map(|e| match (e.state, &e.wake) {
                (ExecState::Waiting, Some(Wake::Confirm { confirm_id })) => {
                    Some((e.id.clone(), confirm_id.clone()))
                }
                _ => None,
            })
            .collect();
        if waiting.is_empty() {
            return Ok(());
        }
        let (exec_id, corr) = waiting[self.rng.random_range(0..waiting.len())].clone();
        let note = if self.chance(0.5) {
            "not now"
        } else {
            "superseded: new input came instead of an answer"
        };
        // An older build's answer, two frames, may have left it declined and
        // still waiting: then only the wake.
        let open = self
            .kernel
            .action(&corr)?
            .is_some_and(|a| a.state == ActionState::Planned);
        // The answer and its wake are one frame, a kernel transaction, as the
        // core answers (theseus-jj9f): a crash comes before it or after it.
        self.kernel.frame(&[&exec_id], |k| {
            if open {
                k.decline_action(&corr, "sim", note)?;
            }
            k.wake(&exec_id, "confirm")
        })?;
        if open {
            self.rep.asked_declined += 1;
        }
        self.rep.one_frame_answers += 1;
        self.maybe_crash("after the answer")?;
        Ok(())
    }

    /// A turn whose actions run together (theseus-a60): 3 to 6 dispatched
    /// back to back, one frame each, as the tool runtime plans a group. Like
    /// its tool calls, they reserve nothing. Half are in-process calls, which
    /// complete in the turn in a random order; the rest are jobs. A crash
    /// after the dispatch, or between two completions, leaves each in-process
    /// call it interrupts with no evidence: a lost job, which must end unknown
    /// or cancelled.
    fn take_a_batch(&mut self, exec_id: &str) -> Result<()> {
        let n = self.rng.random_range(3..=6);
        let mut batch: Vec<(String, bool)> = Vec::new();
        for _ in 0..n {
            let g = self.guards.get(exec_id).unwrap();
            let prop = Proposal {
                tool: "fake.read".into(),
                args: json!({"n": self.rng.random_range(0..1000)}),
                resource: None,
                policy_context: json!({}),
            };
            let a = self.kernel.plan_and_dispatch(
                g,
                &prop,
                RetryClass::SafeToRepeat,
                Some(20_000),
                0,
                |_| Ok(vec![]),
            )?;
            self.rep.actions += 1;
            self.rep.one_frame_dispatches += 1;
            self.one_frame.insert(a.correlation_id.clone());
            let in_process = self.chance(0.5);
            batch.push((a.correlation_id, in_process));
        }
        self.rep.batches += 1;
        self.rep.batch_actions += batch.len() as u64;
        let now = self.now();
        for (corr, in_process) in &batch {
            let lost = !in_process && self.chance(self.p.p_lost_job);
            if lost {
                self.rep.lost_jobs += 1;
            }
            let delay = self.rng.random_range(0..40_000);
            self.jobs.push(Job {
                corr: corr.clone(),
                // An in-process call finishes in the turn, or never.
                finish_at: if *in_process || lost {
                    None
                } else {
                    Some(now + delay)
                },
                outcome: if self.rng.random_bool(0.8) {
                    Outcome::Succeeded
                } else {
                    Outcome::Failed
                },
                spooled: false,
                settled: false,
                cancelled: false,
                reserved: 0,
            });
        }
        if self.maybe_crash("after a batch dispatch")? {
            self.rep.batch_crashes += 1;
            return Ok(());
        }
        let mut in_process: Vec<String> = batch
            .iter()
            .filter(|(_, p)| *p)
            .map(|(c, _)| c.clone())
            .collect();
        in_process.shuffle(&mut self.rng);
        // The turn reads its in-process results itself: each settles through
        // a view that is the turn's own, which never queues it (theseus-l6y).
        // A view holds the store, so none lives across a crash point, where
        // the store is opened again.
        let mut settled_own = 0;
        for corr in in_process {
            let i = self.jobs.iter().position(|j| j.corr == corr).unwrap();
            let c = Completion {
                correlation_id: corr.clone(),
                outcome: self.jobs[i].outcome,
                result_ref: Some(format!("node_{corr}")),
                external_op_id: None,
                started_at_ms: now,
                finished_at_ms: self.now(),
                producer: "inproc:fake.read".into(),
                signature: None,
                cost_micros: None,
                detail: None,
            };
            let own = self
                .kernel
                .view(self.kernel.store().clone())
                .turn_of(exec_id);
            own.accept_completion(&c)?;
            settled_own += own.own_settled();
            drop(own);
            self.own.insert(corr.clone());
            self.rep.own_results += 1;
            // Committed: it must stay settled.
            self.jobs[i].spooled = true;
            self.jobs[i].settled = true;
            self.rep.completions_delivered += 1;
            if self.maybe_crash("between a batch's completions")? {
                self.rep.batch_crashes += 1;
                return Ok(());
            }
        }
        let g = self.guards.remove(exec_id).unwrap();
        // A fault after the results settled (a store error, say), as the
        // core meets it: the turn parks on input, and, since its model never
        // read what it settled, `run` wakes it. The queue entry that once
        // requeued it is not written now, so the wake must.
        if self.chance(0.2) {
            self.rep.faults += 1;
            let e = self
                .kernel
                .end_turn(g, TurnEnd::Wait { wake: Wake::Input })?;
            if e.state.is_terminal() {
                self.kill_jobs(&e.outstanding)?;
                return Ok(());
            }
            if settled_own > 0 {
                self.kernel.wake(exec_id, "fault")?;
                self.rep.fault_wakes += 1;
                let e = self.kernel.execution(exec_id)?.unwrap();
                if e.state != ExecState::Queued || !e.resume_pending {
                    bail!(
                        "a faulted turn with its own results settled left {exec_id} {:?} (resume pending: {})",
                        e.state,
                        e.resume_pending
                    );
                }
            }
            return Ok(());
        }
        let jobs: Vec<String> = batch
            .iter()
            .filter(|(_, p)| !p)
            .map(|(c, _)| c.clone())
            .collect();
        let end = if jobs.is_empty() {
            TurnEnd::Wait { wake: Wake::Input }
        } else {
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: jobs,
                },
            }
        };
        let e = self.kernel.end_turn(g, end)?;
        if e.state.is_terminal() {
            self.kill_jobs(&e.outstanding)?;
        }
        Ok(())
    }

    /// A turn raced by a second OS thread on the same execution (theseus-id9).
    /// The seed picks what that thread does and what the turn does; the OS
    /// picks how they interleave. The turn's calls reserve nothing, and each
    /// finishes as a job, in the turn, or on the racing thread as it arrives.
    /// A crash may stop the turn after any of its frames: the racing thread
    /// runs to its end, and then the process dies.
    fn race_a_turn(&mut self, exec_id: &str) -> Result<()> {
        self.rep.races += 1;
        let mut ops = Vec::new();
        for _ in 0..self.rng.random_range(1..=3) {
            let op = match self.rng.random_range(0..10) {
                0..=2 => RaceOp::Cancel,
                3..=4 => match self.finish_a_job_of(exec_id)? {
                    Some(c) => RaceOp::Complete(c),
                    None => RaceOp::Wake,
                },
                5..=6 => RaceOp::Wake,
                7 => RaceOp::Input,
                8 => RaceOp::Frame,
                _ => RaceOp::Reconcile,
            };
            ops.push(op);
        }
        self.rep.cancels += ops.iter().filter(|o| matches!(o, RaceOp::Cancel)).count() as u64;
        let mut calls = Vec::new();
        for _ in 0..self.rng.random_range(0..=4) {
            let finish = match self.rng.random_range(0..3) {
                0 => Finish::Job(if self.chance(self.p.p_lost_job) {
                    None
                } else {
                    Some(self.rng.random_range(0..40_000))
                }),
                1 => Finish::InTurn,
                _ => Finish::Beside,
            };
            let outcome = if self.rng.random_bool(0.8) {
                Outcome::Succeeded
            } else {
                Outcome::Failed
            };
            calls.push((finish, self.rng.random_bool(0.5), outcome));
        }
        let plan = RacedPlan {
            crash_after: self
                .chance(self.p.p_crash * 5.0)
                .then(|| self.rng.random_range(1..=calls.len() as u32 * 3 + 3)),
            end_pick: self.rng.random_range(0..10),
            due_in: self.rng.random_range(1_000..120_000),
            calls,
        };
        let racer = self.kernel.view(self.kernel.store().clone());
        let spool = self.spool.clone();
        let (beside, handed) = mpsc::channel();
        let start = Barrier::new(2);
        let (turn, raced) = std::thread::scope(|s| {
            let h = {
                let (racer, spool, start) = (&racer, &spool, &start);
                s.spawn(move || {
                    start.wait();
                    run_racer(racer, spool, exec_id, ops, handed)
                })
            };
            start.wait();
            let turn = self.raced_turn(exec_id, &plan, beside);
            (turn, h.join().expect("the racing thread panicked"))
        });
        drop(racer);
        let turn = turn?;
        let raced = raced?;
        self.rep.race_ops += raced.ops;
        self.rep.race_frames += raced.frames;
        self.rep.unknowns += raced.unknowns;
        self.rep.resolved_unknowns += raced.resolved;
        for (corr, acc) in &raced.accepted {
            self.rep.completions_delivered += 1;
            match acc {
                Accepted::LateAfterCancel { .. } => self.rep.late_after_cancel += 1,
                Accepted::ResolvedUnknown { .. } => self.rep.resolved_unknowns += 1,
                _ => {}
            }
            // Committed: it must stay settled.
            if let Some(j) = self.jobs.iter_mut().find(|j| &j.corr == corr) {
                j.spooled = true;
                j.settled = true;
            }
        }
        self.mark_settled_from_spool();
        let mut to_kill = raced.to_kill;
        if let Some(e) = turn.ended.as_ref().filter(|e| e.state.is_terminal()) {
            to_kill.extend(e.outstanding.iter().cloned());
        }
        to_kill.sort();
        to_kill.dedup();
        if turn.crashed {
            self.rep.race_crashes += 1;
            self.crash("inside a raced turn")?;
        }
        self.kill_jobs(&to_kill)
    }

    /// The raced turn's own commits, on this thread.
    fn raced_turn(
        &mut self,
        exec_id: &str,
        plan: &RacedPlan,
        beside: mpsc::Sender<(String, Outcome)>,
    ) -> Result<RacedTurn> {
        let mut out = RacedTurn::default();
        let mut frames = 0;
        let mut stop = || {
            frames += 1;
            plan.crash_after == Some(frames)
        };
        let g = match self.kernel.admit(exec_id) {
            Ok(g) => g,
            // Cancelled beside it, or no room: no turn.
            Err(e)
                if matches!(
                    e.downcast_ref::<KernelError>(),
                    Some(
                        KernelError::NotRunnable { .. }
                            | KernelError::AdmissionFull { .. }
                            | KernelError::TurnHeld { .. }
                    )
                ) =>
            {
                return Ok(out)
            }
            Err(e) => return Err(e),
        };
        self.rep.turns += 1;
        if stop() {
            std::mem::forget(g);
            out.crashed = true;
            return Ok(out);
        }
        self.kernel.take_results(&g)?;
        if stop() {
            std::mem::forget(g);
            out.crashed = true;
            return Ok(out);
        }
        let now = self.now();
        let mut waits_on = Vec::new();
        for (i, &(finish, one_frame, outcome)) in plan.calls.iter().enumerate() {
            let prop = Proposal {
                tool: "fake.race".into(),
                args: json!({"i": i}),
                resource: None,
                policy_context: json!({}),
            };
            let planned = if one_frame {
                self.kernel.plan_and_dispatch(
                    &g,
                    &prop,
                    RetryClass::SafeToRepeat,
                    Some(20_000),
                    0,
                    |_| Ok(vec![]),
                )
            } else {
                self.kernel
                    .plan_action(&g, &prop, RetryClass::SafeToRepeat, Some(20_000), 0)
            };
            let a = match planned {
                Ok(a) => a,
                // Cancelled beside the turn: nothing more is planned.
                Err(e)
                    if matches!(
                        e.downcast_ref::<KernelError>(),
                        Some(KernelError::NoTurn {
                            state: "cancelled",
                            ..
                        })
                    ) =>
                {
                    break
                }
                Err(e) => return Err(e),
            };
            self.rep.actions += 1;
            if one_frame {
                self.rep.one_frame_dispatches += 1;
                self.one_frame.insert(a.correlation_id.clone());
            } else {
                if stop() {
                    std::mem::forget(g);
                    out.crashed = true;
                    return Ok(out);
                }
                // Cancelled between the plan and the dispatch: it never
                // leaves. The cancel settled it (theseus-w98), so the
                // authorize or the dispatch, whichever follows, is refused.
                let cancelled = |e: &anyhow::Error| {
                    matches!(
                        e.downcast_ref::<KernelError>(),
                        Some(KernelError::NotRunnable { .. })
                    )
                };
                match self.kernel.authorize(&a.correlation_id, &prop, None) {
                    Ok(_) => {}
                    Err(e) if cancelled(&e) => break,
                    Err(e) => return Err(e),
                }
                if stop() {
                    std::mem::forget(g);
                    out.crashed = true;
                    return Ok(out);
                }
                match self.kernel.dispatch(&a.correlation_id, None) {
                    Ok(_) => {}
                    Err(e) if cancelled(&e) => break,
                    Err(e) => return Err(e),
                }
            }
            if let Finish::Job(None) = finish {
                self.rep.lost_jobs += 1;
            }
            self.jobs.push(Job {
                corr: a.correlation_id.clone(),
                finish_at: match finish {
                    Finish::Job(delay) => delay.map(|d| now + d),
                    // In process: it finishes in the turn or beside it, or never.
                    Finish::InTurn | Finish::Beside => None,
                },
                outcome,
                spooled: false,
                settled: false,
                cancelled: false,
                reserved: 0,
            });
            if stop() {
                std::mem::forget(g);
                out.crashed = true;
                return Ok(out);
            }
            match finish {
                Finish::Job(_) => waits_on.push(a.correlation_id),
                Finish::InTurn => {
                    self.kernel.accept_completion(&Completion {
                        correlation_id: a.correlation_id.clone(),
                        outcome,
                        result_ref: Some(format!("node_{}", a.correlation_id)),
                        external_op_id: None,
                        started_at_ms: now,
                        finished_at_ms: self.now(),
                        producer: "inproc:fake.race".into(),
                        signature: None,
                        cost_micros: None,
                        detail: None,
                    })?;
                    let j = self.jobs.last_mut().unwrap();
                    j.spooled = true;
                    j.settled = true;
                    self.rep.completions_delivered += 1;
                    if stop() {
                        std::mem::forget(g);
                        out.crashed = true;
                        return Ok(out);
                    }
                }
                Finish::Beside => {
                    let _ = beside.send((a.correlation_id, outcome));
                }
            }
        }
        drop(beside);
        let end = if !waits_on.is_empty() {
            TurnEnd::Wait {
                wake: Wake::Actions {
                    correlation_ids: waits_on,
                },
            }
        } else {
            match plan.end_pick {
                0 => TurnEnd::Complete {
                    reason: "sim done".into(),
                },
                1..=3 => TurnEnd::Wait {
                    wake: Wake::DueAt {
                        at_ms: now + plan.due_in,
                    },
                },
                4..=6 => TurnEnd::Wait { wake: Wake::Input },
                _ => TurnEnd::Requeue,
            }
        };
        out.ended = Some(self.kernel.end_turn(g, end)?);
        Ok(out)
    }

    /// A job of this execution finishes now: its completion is spooled, as
    /// its wrapper would, for the racing thread to accept.
    fn finish_a_job_of(&mut self, exec_id: &str) -> Result<Option<Completion>> {
        let mut mine = Vec::new();
        for (i, j) in self.jobs.iter().enumerate() {
            if j.spooled || j.finish_at.is_none() {
                continue;
            }
            if self
                .kernel
                .action(&j.corr)?
                .is_some_and(|a| a.execution_id == exec_id)
            {
                mine.push(i);
            }
        }
        if mine.is_empty() {
            return Ok(None);
        }
        let i = mine[self.rng.random_range(0..mine.len())];
        let j = self.jobs[i].clone();
        let now = self.now();
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
        self.spool.write(&c)?;
        self.jobs[i].spooled = true;
        Ok(Some(c))
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
                    || matches!(
                        e.wake,
                        Some(Wake::Budget { .. }) | Some(Wake::Confirm { .. })
                    )
            })
            .collect();
        if open.is_empty() {
            return Ok(());
        }
        let e = &open[self.rng.random_range(0..open.len())];
        let cancel = self
            .kernel
            .cancel_execution_with(&e.id, "sim", |_| Ok(vec![]))?;
        let to_kill = cancel.to_kill;
        self.rep.cancels += 1;
        self.rep.ended_unsent += cancel.not_run.len() as u64;
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

    /// The kernel's reads by state agree with a read of every record
    /// (theseus-lv2): the index's terms are a projection of the WAL, through
    /// every crash, reopen, and race the run makes.
    fn check_terms(
        &self,
        at: &str,
        execs: &[Execution],
        actions: &[Action],
        stats: &theseus_kernel::KernelStats,
    ) -> Result<()> {
        fn ids<'a>(v: impl Iterator<Item = &'a str>) -> Vec<String> {
            let mut v: Vec<String> = v.map(str::to_string).collect();
            v.sort();
            v
        }
        let k = &self.kernel;
        let want = ids(execs
            .iter()
            .filter(|e| !e.state.is_terminal())
            .map(|e| e.id.as_str()));
        let got = ids(k.open_executions()?.iter().map(|e| e.id.as_str()));
        if got != want {
            bail!("{at}: open executions by their terms {got:?}, by a full read {want:?}");
        }
        let runnable = |e: &&Execution| {
            e.state == ExecState::Queued
                || (e.state == ExecState::Waiting
                    && (matches!(e.wake, Some(Wake::DueAt { .. }))
                        || !e.wakes.is_empty()
                        || !e.report_wakes.is_empty()))
        };
        let want = ids(execs.iter().filter(runnable).map(|e| e.id.as_str()));
        let got = ids(k.maybe_runnable()?.iter().map(|e| e.id.as_str()));
        if got != want {
            bail!("{at}: maybe runnable by their terms {got:?}, by a full read {want:?}");
        }
        let want = ids(actions
            .iter()
            .filter(|a| !a.state.is_settled())
            .map(|a| a.correlation_id.as_str()));
        let got = ids(k.open_actions()?.iter().map(|a| a.correlation_id.as_str()));
        if got != want {
            bail!("{at}: open actions by their terms {got:?}, by a full read {want:?}");
        }
        for e in execs {
            let want = ids(actions
                .iter()
                .filter(|a| a.execution_id == e.id && !a.state.is_settled())
                .map(|a| a.correlation_id.as_str()));
            let got = ids(k
                .unsettled_actions(&e.id)?
                .iter()
                .map(|a| a.correlation_id.as_str()));
            if got != want {
                bail!(
                    "{at}: {}'s unsettled actions by their terms {got:?}, by a full read {want:?}",
                    e.id
                );
            }
        }
        let mut by_state: BTreeMap<String, u64> = BTreeMap::new();
        for e in execs {
            *by_state.entry(e.state.as_str().to_string()).or_default() += 1;
        }
        if stats.executions_by_state != by_state {
            bail!(
                "{at}: executions by state from their terms {:?}, by a full read {by_state:?}",
                stats.executions_by_state
            );
        }
        let mut by_state: BTreeMap<String, u64> = BTreeMap::new();
        for a in actions {
            *by_state.entry(a.state.as_str().to_string()).or_default() += 1;
        }
        if stats.actions_by_state != by_state {
            bail!(
                "{at}: actions by state from their terms {:?}, by a full read {by_state:?}",
                stats.actions_by_state
            );
        }
        let want = ids(execs
            .iter()
            .filter(|e| e.parent.is_some())
            .map(|e| e.id.as_str()));
        let got = ids(k.tasks(None)?.iter().map(|e| e.id.as_str()));
        if got != want {
            bail!("{at}: tasks by their terms {got:?}, by a full read {want:?}");
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
        self.check_terms(at, &execs, &actions, &stats)?;
        // An execution that was cancelled never runs again (theseus-id9). Read
        // in WAL order, as far as the store has gone: after an execution's
        // `execution.cancelled` row, no turn of it starts and no action of it
        // is planned, and it is `cancelled` from then on. Its rows are
        // evidence a later write cannot take back, so a cancel another thread
        // overwrote between two checks is caught too.
        // Each execution's `budget.limit_changed` since the last check: (from, to).
        let mut changed: HashMap<String, (Micros, Micros)> = HashMap::new();
        let last = self.kernel.store().last_position();
        if last > self.ledger_read_to {
            for r in self
                .kernel
                .store()
                .scan(self.ledger_read_to + 1, Some(last), usize::MAX)?
            {
                if r.kind != theseus_store::kinds::LEDGER {
                    continue;
                }
                let row: LedgerRow = r.decode()?;
                let Some(id) = row.data["execution_id"].as_str() else {
                    continue;
                };
                match row.kind.as_str() {
                    "execution.cancelled" => {
                        self.cancelled.insert(id.to_string());
                    }
                    "budget.limit_changed" => {
                        let micros = |k: &str| usd_to_micros(row.data[k].as_f64().unwrap_or(-1.0));
                        changed.insert(id.to_string(), (micros("from_usd"), micros("to_usd")));
                    }
                    "execution.running" | "action.planned" if self.cancelled.contains(id) => {
                        bail!(
                            "{at}: {id} was cancelled, and then wrote {} at {}",
                            row.kind,
                            r.position
                        )
                    }
                    _ => {}
                }
            }
            self.ledger_read_to = last;
        }
        for e in execs.iter().filter(|e| self.cancelled.contains(&e.id)) {
            if e.state != ExecState::Cancelled {
                bail!("{at}: {} was cancelled and is now {:?}", e.id, e.state);
            }
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
            // Nothing is reserved past the limit (a job costs at most what it
            // reserved). A lower limit may come after the spend (theseus-3pj),
            // and a unit budget read in dollars may start over it (its spend is
            // its session's recorded cost): the budget is then over the limit,
            // and nothing new is reserved until it fits again.
            let total = b.spent_micros + b.reserved_micros + b.held_unknown_micros;
            let holds: BTreeSet<String> = b.reservations.keys().cloned().collect();
            if total > b.limit_micros {
                let lowered = match self.limits.get(&e.id) {
                    Some(&was) => b.limit_micros < was,
                    None => b.units_before.is_some(),
                };
                let still = self
                    .over
                    .get(&e.id)
                    .is_some_and(|before| holds.is_subset(before));
                if !(lowered || still) {
                    bail!("{at}: {} budget over limit: {:?}", e.id, b);
                }
                self.over.insert(e.id.clone(), holds);
            } else {
                self.over.remove(&e.id);
            }
            // A limit of its own never changes; an open execution that
            // follows the config has the config's; and each change is a
            // `budget.limit_changed` row from the limit before to this one.
            match self.pinned.get(&e.id) {
                Some(&own) if b.limit_micros != own || !b.pinned => {
                    bail!("{at}: {} opened with its own limit {own}: {:?}", e.id, b)
                }
                None if !e.state.is_terminal() && (b.pinned || b.limit_micros != self.limit) => {
                    bail!(
                        "{at}: {} follows the config's limit {}, and has {} (pinned: {})",
                        e.id,
                        self.limit,
                        b.limit_micros,
                        b.pinned
                    )
                }
                _ => {}
            }
            let was = self.limits.insert(e.id.clone(), b.limit_micros);
            match (was, changed.get(&e.id)) {
                (Some(was), None) if was != b.limit_micros => bail!(
                    "{at}: {}'s limit went from {was} to {} with no budget.limit_changed row",
                    e.id,
                    b.limit_micros
                ),
                (Some(was), Some(&row)) if row != (was, b.limit_micros) => bail!(
                    "{at}: {}'s limit went from {was} to {}, and its row says {row:?}",
                    e.id,
                    b.limit_micros
                ),
                (None, Some(row)) => bail!(
                    "{at}: {} is new, and already has a budget.limit_changed row {row:?}",
                    e.id
                ),
                _ => {}
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
                // Nothing it planned outlives it (theseus-w98): an ended
                // execution has no action planned or authorized, so nothing
                // counts as waiting on it, and nothing of it can still run.
                if let Some(a) = by_corr.values().find(|a| {
                    a.execution_id == e.id
                        && matches!(a.state, ActionState::Planned | ActionState::Authorized)
                }) {
                    bail!(
                        "{at}: {} is {:?}, and its action {} ({}) is still {}",
                        e.id,
                        e.state,
                        a.correlation_id,
                        a.tool,
                        a.state.as_str()
                    );
                }
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
                // A turn read its own results itself (theseus-l6y).
                if self.own.contains(c) {
                    bail!("{at}: {c}, a result its turn read itself, is queued");
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
        // Authorized and dispatched in one frame (theseus-l6y): after any
        // crash, never found authorized, and authorized no later than
        // dispatched.
        for c in &self.two_in_one {
            let a = by_corr
                .get(c)
                .ok_or_else(|| anyhow::anyhow!("{at}: two-in-one action {c} is gone"))?;
            if matches!(a.state, ActionState::Planned | ActionState::Authorized) {
                bail!("{at}: two-in-one action {c} is {:?}", a.state);
            }
            match (a.authorized_at_ms, a.dispatched_at_ms) {
                (Some(au), Some(d)) if au <= d => {}
                times => bail!("{at}: two-in-one action {c} with {times:?}"),
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

/// What the racing thread does to a raced turn's execution (theseus-id9).
enum RaceOp {
    Cancel,
    /// A job of the execution finished: its completion is spooled, and the
    /// racing thread accepts it and removes the file, as the driver's drain does.
    Complete(Completion),
    Wake,
    Input,
    /// The heartbeat's reconcile, over every execution.
    Reconcile,
    /// Input and a nudge in one kernel transaction (theseus-0owd): one
    /// frame, or nothing when the execution ended.
    Frame,
}

/// Where a raced turn's call finishes.
#[derive(Debug, Clone, Copy)]
enum Finish {
    /// A job, after this many virtual ms, or never.
    Job(Option<u64>),
    /// In process, in the turn.
    InTurn,
    /// In process, on the racing thread, as soon as it is dispatched.
    Beside,
}

/// A raced turn, as the seed chose it: its calls (where each finishes, whether
/// it is planned in one frame, its outcome), the frame after which a crash
/// stops it, and how it ends.
struct RacedPlan {
    calls: Vec<(Finish, bool, Outcome)>,
    crash_after: Option<u32>,
    end_pick: u32,
    due_in: u64,
}

#[derive(Default)]
struct RacedTurn {
    crashed: bool,
    /// The execution as `end_turn` left it.
    ended: Option<Execution>,
}

/// What the racing thread did.
#[derive(Default)]
struct Raced {
    ops: u64,
    /// Its kernel transactions.
    frames: u64,
    /// The calls each cancel said to stop.
    to_kill: Vec<String>,
    /// Each completion it delivered, and what accepting it did.
    accepted: Vec<(String, Accepted)>,
    unknowns: u64,
    resolved: u64,
}

/// The racing thread: its operations in order, and each call the turn hands
/// it completed as it arrives, until the turn is done.
fn run_racer(
    k: &Kernel,
    spool: &Spool,
    exec_id: &str,
    ops: Vec<RaceOp>,
    handed: mpsc::Receiver<(String, Outcome)>,
) -> Result<Raced> {
    let mut out = Raced::default();
    let finish = |corr: String, outcome: Outcome, out: &mut Raced| -> Result<()> {
        let acc = k.accept_completion(&Completion {
            correlation_id: corr.clone(),
            outcome,
            result_ref: Some(format!("node_{corr}")),
            external_op_id: None,
            started_at_ms: k.now_ms(),
            finished_at_ms: k.now_ms(),
            producer: "inproc:beside".into(),
            signature: None,
            cost_micros: None,
            detail: None,
        })?;
        out.accepted.push((corr, acc));
        Ok(())
    };
    // A wake of an execution that ended meanwhile is refused, which is right.
    let ended = |r: Result<Execution>| match r {
        Err(e)
            if !matches!(
                e.downcast_ref::<KernelError>(),
                Some(KernelError::NotRunnable { .. })
            ) =>
        {
            Err(e)
        }
        _ => Ok(()),
    };
    for op in ops {
        out.ops += 1;
        match op {
            RaceOp::Cancel => out.to_kill.extend(k.cancel_execution(exec_id, "sim-race")?),
            RaceOp::Complete(c) => {
                let acc = k.accept_completion(&c)?;
                spool.remove(&spool.completion_path(&c.correlation_id))?;
                out.accepted.push((c.correlation_id, acc));
            }
            RaceOp::Wake => ended(k.wake(exec_id, "sim-race"))?,
            RaceOp::Input => ended(k.wake_input(exec_id))?,
            RaceOp::Frame => {
                out.frames += 1;
                ended(k.frame(&[exec_id], |k| {
                    k.wake_input(exec_id)?;
                    k.wake(exec_id, "sim-race")
                }))?
            }
            RaceOp::Reconcile => {
                let rep = k.reconcile(&WrapperEvidence {
                    spool: spool.clone(),
                })?;
                out.unknowns += rep.marked_unknown.len() as u64;
                out.resolved += rep.resolved_unknown.len() as u64;
            }
        }
        while let Ok((corr, outcome)) = handed.try_recv() {
            finish(corr, outcome, &mut out)?;
        }
    }
    for (corr, outcome) in handed {
        finish(corr, outcome, &mut out)?;
    }
    Ok(out)
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
