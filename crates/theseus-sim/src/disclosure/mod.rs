//! The disclosure simulator (M4 19b; design m4-boundaries §2.7; P6: "private
//! material never reaches a public audience's context").
//!
//! `theseus-sim disclosure --seed N --steps M` builds a world from its seed
//! (people, guild channels whose viewers change, mid-turn too, a private and a
//! public tree of files, context files of both kinds) and drives a whole core
//! through it, as a library: operator messages from the CLI, DMs, and
//! channels, with attachments; the owner's file reads, public ones, fetches,
//! and jobs that connected out; tasks with briefs and reports; graduations by
//! the operator; held posts and their answers; trusts; and a test-only
//! "foreign node", a node of another session written into this one with its
//! label, standing in for M6's recall so the filter meets nodes labeled for
//! other places and people before M6 exists.
//!
//! The sim plays the Discord binding (it binds places, tells the core who can
//! view a channel, and delivers posts with the binding's own check at post
//! time) and the harness driver (one continuation at a time, so tasks and woken
//! parents run in an order the seed fixes). Its model repeats every atom its
//! request carried (`model.rs`), and its oracle judges every atom that leaves by
//! its own copy of §2.5's rules (`atoms.rs`), never by the core's labels.
//!
//! The invariants, checked after every compile, every streamed edit, every
//! post, and every step (`check.rs`, `courier.rs`):
//! - **admitted**: every atom a request carries may be read by the audience the
//!   compile was for, and that audience is the session's;
//! - **posted**: every post's atoms may be read by who can view its place when
//!   it is posted, or the post was held and the owner released it;
//! - **quiet**: no text streams into a guild channel unless every atom in it
//!   fits any audience the channel can have (19c's quiet loops);
//! - **paired**: every `tool_use` in a request has its `tool_result` next;
//! - **latch (I1)**: a session holds external text exactly when, since its
//!   last trust, it was written a result from outside (a fetch, a job that
//!   connected out), a brief from a session that held it, or a report from a
//!   task that held it (T1's sites, which 20a moves to admission).
//!
//! A failure names its seed, its step, the invariant, and the atoms and nodes
//! involved; the same seed reproduces it.

mod atoms;
mod check;
mod courier;
mod model;
mod rig;
mod steps;
mod tools;
mod world;

#[cfg(test)]
mod tests;

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use anyhow::{anyhow, Result};
use theseus_core::Core;

use world::{Push, Shared};

/// A run's knobs.
#[derive(Debug, Clone)]
pub struct Params {
    pub seed: u64,
    pub steps: u32,
    /// The share of the model's calls that change a channel's viewers first.
    pub p_mid_turn: f64,
    /// Fail on the known gaps too (`atoms::KNOWN_GAPS`), instead of counting
    /// them.
    pub strict: bool,
    pub verbose: bool,
}

#[cfg(test)]
impl Params {
    pub fn new(seed: u64, steps: u32) -> Self {
        Self {
            seed,
            steps,
            p_mid_turn: 0.15,
            strict: false,
            verbose: false,
        }
    }
}

/// What a run did, counted. Two runs of one seed report the same, its time
/// aside (`trace` digests every step's decisions).
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Report {
    pub seed: u64,
    pub steps: u32,
    pub sessions: u64,
    pub turns: u64,
    pub continuations: u64,
    /// Requests the model answered, each checked: one per compile.
    pub compiled: u64,
    pub recompiled_for_audience: u64,
    /// Nodes and context files the requests carried as placeholders, and the
    /// requests that withheld any.
    pub withheld: u64,
    pub withholding: u64,
    /// Files that came with a withheld message, left out with it.
    pub withheld_files: u64,
    pub tool_calls: u64,
    pub fetches: u64,
    pub egress: u64,
    pub tasks: u64,
    pub posts: u64,
    pub reports: u64,
    /// Posts that took a fresh read of who views their channel.
    pub read_at_post: u64,
    pub held: u64,
    pub released: u64,
    pub held_back: u64,
    pub graduated: u64,
    pub graduation_refused: u64,
    pub foreign: u64,
    pub trusts: u64,
    pub viewer_changes: u64,
    pub unheard_changes: u64,
    pub mid_turn_changes: u64,
    pub quiet_loops: u64,
    pub quiet_deltas: u64,
    pub streamed_deltas: u64,
    pub latch_checks: u64,
    pub atoms: u64,
    pub grants: u64,
    pub invariant_checks: u64,
    /// Atoms that reached an audience by a known gap (`atoms::KNOWN_GAPS`),
    /// counted instead of failing.
    pub known_gaps: u64,
    pub trace: String,
    #[serde(skip)]
    pub wall_ms: u64,
}

/// An invariant that did not hold: the run stops on the first.
#[derive(Debug)]
pub struct Violation {
    pub seed: u64,
    pub step: u32,
    pub invariant: &'static str,
    pub detail: String,
}

impl std::fmt::Display for Violation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "seed {}, step {}: the {} invariant does not hold: {}\n  reproduce: theseus-sim disclosure --seed {} --steps {}",
            self.seed,
            self.step,
            self.invariant,
            self.detail,
            self.seed,
            self.step + 1
        )
    }
}

impl std::error::Error for Violation {}

/// Where a session posts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlaceKind {
    Cli,
    Dm(u64),
    Channel(u64),
}

impl PlaceKind {
    /// The outbox's place, for `bind_place`.
    fn bind(self) -> Option<String> {
        match self {
            PlaceKind::Cli => None,
            PlaceKind::Dm(u) => Some(format!("dm:{u}")),
            PlaceKind::Channel(c) => Some(format!("channel:{c}")),
        }
    }
}

/// A session the sim knows: one it opened for a place, or a task the core
/// opened for one of them.
#[derive(Debug, Clone)]
pub(crate) struct Sess {
    pub id: String,
    pub name: String,
    pub place: Option<PlaceKind>,
    /// For a task: the session that started it, and its short id.
    pub parent: Option<usize>,
    pub short: Option<String>,
    /// Where it posts (`outbox.target`), for its audience.
    pub target: Option<String>,
    pub live: bool,
    pub turns: u32,
    /// The model of its latch, and the node that set it.
    pub latched: Option<String>,
    /// The WAL position its nodes were read to.
    pub read_to: u64,
}

/// A post the courier saw held: its question, which the owner answers.
#[derive(Debug, Clone)]
pub(crate) struct HeldPost {
    pub question: String,
}

pub(crate) struct Sim {
    pub p: Params,
    pub shared: Arc<Mutex<Shared>>,
    pub core: Arc<Core>,
    /// Keeps the core's temp dir until the run ends.
    _rig: rig::Rig,
    pub sessions: Vec<Sess>,
    pub by_id: BTreeMap<String, usize>,
    /// Each place, and its session now.
    pub places: Vec<(PlaceKind, usize)>,
    pub held: Vec<HeldPost>,
    /// The held posts' questions the owner approved: only those may go.
    pub approved: BTreeSet<String>,
    /// Sessions written to since the last latch check.
    pub dirty: BTreeSet<usize>,
    pub rep: Report,
    pub step: u32,
    trace: u64,
    /// Where a run's time went, by phase (`--verbose` prints it).
    pub spent: BTreeMap<&'static str, std::time::Duration>,
}

/// Tell the core who can view each pushed channel, as the binding does.
pub(crate) fn push_all(core: &Core, pushes: &[Push]) {
    for p in pushes {
        core.place_viewers(
            p.channel,
            Some(p.name.clone()),
            p.viewers.as_ref().map(|v| v.iter().copied().collect()),
            p.viewers
                .is_none()
                .then_some("the bot cannot read who views it"),
        );
    }
}

/// One seed's counts on a line.
pub fn line(r: &Report) -> String {
    format!(
        "seed {}: {} steps in {} ms · {} sessions · {} turns, {} continuations · {} compiles checked ({} withheld nodes in {}, {} files with them; {} recompiled for an audience) · {} quiet loops ({} deltas kept back, {} streamed) · {} posts ({} read at post time; {} held: {} released, {} held back) · {} graduated ({} refused) · {} foreign nodes · {} tasks, {} reports · {} calls ({} fetches, {} egress) · {} trusts · {} viewer changes ({} unheard, {} mid-turn) · {} latch checks · {} invariant checks · {} atoms, {} graduated · {} by known gaps · trace {}",
        r.seed, r.steps, r.wall_ms, r.sessions, r.turns, r.continuations, r.compiled, r.withheld,
        r.withholding, r.withheld_files, r.recompiled_for_audience, r.quiet_loops, r.quiet_deltas, r.streamed_deltas,
        r.posts, r.read_at_post, r.held, r.released, r.held_back, r.graduated,
        r.graduation_refused, r.foreign, r.tasks, r.reports, r.tool_calls, r.fetches, r.egress,
        r.trusts, r.viewer_changes, r.unheard_changes, r.mid_turn_changes, r.latch_checks,
        r.invariant_checks, r.atoms, r.grants, r.known_gaps, r.trace
    )
}

/// The known gaps a run counted, in words.
fn known_line(n: u64) -> String {
    let gaps: Vec<String> = atoms::KNOWN_GAPS
        .iter()
        .map(|(id, what)| format!("{id} ({what})"))
        .collect();
    format!(
        "{n} atoms reached an audience by a known gap, counted, not failed: {}",
        gaps.join("; ")
    )
}

/// `theseus-sim disclosure`: each seed from `seed`, a line each, and the
/// totals; the first invariant that does not hold stops it.
pub fn cli(p: &Params, seeds: u32, json: bool) -> Result<()> {
    let t0 = std::time::Instant::now();
    let mut total = Report::default();
    for s in p.seed..p.seed + u64::from(seeds) {
        let rep = match run(&Params {
            seed: s,
            ..p.clone()
        }) {
            Ok(r) => r,
            Err(e) => {
                if let Some(v) = e.downcast_ref::<Violation>() {
                    println!("DISCLOSURE FAILED: {v}");
                }
                return Err(e.context(format!("seed {s}")));
            }
        };
        if json {
            println!("{}", serde_json::to_string(&rep)?);
        } else {
            println!("{}", line(&rep));
        }
        add(&mut total, &rep);
    }
    total.seed = p.seed;
    total.wall_ms = t0.elapsed().as_millis() as u64;
    println!(
        "DISCLOSURE OK: {seeds} seeds of {} steps in {} ms · {} compiles ({} withheld nodes, {} withheld files) · {} held posts ({} released, {} held back) · {} graduated · {} quiet loops ({} deltas kept back) · {} posts · {} tasks · {} invariant checks",
        p.steps, total.wall_ms, total.compiled, total.withheld, total.withheld_files, total.held, total.released,
        total.held_back, total.graduated, total.quiet_loops, total.quiet_deltas, total.posts,
        total.tasks, total.invariant_checks
    );
    if total.known_gaps > 0 {
        println!("{}", known_line(total.known_gaps));
    }
    Ok(())
}

fn add(t: &mut Report, r: &Report) {
    t.steps += r.steps;
    t.sessions += r.sessions;
    t.turns += r.turns;
    t.continuations += r.continuations;
    t.compiled += r.compiled;
    t.recompiled_for_audience += r.recompiled_for_audience;
    t.withheld += r.withheld;
    t.withholding += r.withholding;
    t.withheld_files += r.withheld_files;
    t.tool_calls += r.tool_calls;
    t.fetches += r.fetches;
    t.egress += r.egress;
    t.tasks += r.tasks;
    t.posts += r.posts;
    t.reports += r.reports;
    t.read_at_post += r.read_at_post;
    t.held += r.held;
    t.released += r.released;
    t.held_back += r.held_back;
    t.graduated += r.graduated;
    t.graduation_refused += r.graduation_refused;
    t.foreign += r.foreign;
    t.trusts += r.trusts;
    t.viewer_changes += r.viewer_changes;
    t.unheard_changes += r.unheard_changes;
    t.mid_turn_changes += r.mid_turn_changes;
    t.quiet_loops += r.quiet_loops;
    t.quiet_deltas += r.quiet_deltas;
    t.streamed_deltas += r.streamed_deltas;
    t.latch_checks += r.latch_checks;
    t.atoms += r.atoms;
    t.grants += r.grants;
    t.invariant_checks += r.invariant_checks;
    t.known_gaps += r.known_gaps;
}

/// Run one seed.
pub fn run(p: &Params) -> Result<Report> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let t0 = std::time::Instant::now();
    let mut rep = rt.block_on(async {
        let mut sim = Sim::new(p.clone())?;
        if p.verbose {
            eprintln!("  the core was built in {} ms", t0.elapsed().as_millis());
        }
        sim.go().await?;
        Ok::<Report, anyhow::Error>(sim.finish())
    })?;
    rep.wall_ms = t0.elapsed().as_millis() as u64;
    Ok(rep)
}

impl Sim {
    fn new(p: Params) -> Result<Self> {
        let shared = Arc::new(Mutex::new(Shared::generate(p.seed, p.p_mid_turn)));
        let rig = rig::build(&shared)?;
        Ok(Self {
            core: rig.core.clone(),
            _rig: rig,
            shared,
            sessions: Vec::new(),
            by_id: BTreeMap::new(),
            places: Vec::new(),
            held: Vec::new(),
            approved: BTreeSet::new(),
            dirty: BTreeSet::new(),
            rep: Report {
                seed: p.seed,
                ..Default::default()
            },
            step: 0,
            trace: 0xcbf2_9ce4_8422_2325,
            spent: BTreeMap::new(),
            p,
        })
    }

    /// Count `t0`'s time to now against `phase`.
    pub fn spent(&mut self, phase: &'static str, t0: std::time::Instant) {
        *self.spent.entry(phase).or_default() += t0.elapsed();
    }

    pub fn lock(&self) -> MutexGuard<'_, Shared> {
        self.shared
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Fold a step's decision into the run's trace (FNV-1a).
    pub fn note(&mut self, what: &str) {
        for b in what.bytes().chain(*b"\n") {
            self.trace ^= u64::from(b);
            self.trace = self.trace.wrapping_mul(0x0100_0000_01b3);
        }
        if self.p.verbose {
            eprintln!("  step {}: {what}", self.step);
        }
    }

    pub fn violation(&self, invariant: &'static str, detail: String) -> anyhow::Error {
        anyhow!(Violation {
            seed: self.p.seed,
            step: self.step,
            invariant,
            detail,
        })
    }

    async fn go(&mut self) -> Result<()> {
        self.connect()?;
        for step in 0..self.p.steps {
            self.step = step;
            self.lock().step = step;
            let t0 = std::time::Instant::now();
            self.one_step().await?;
            self.spent("step", t0);
            let t0 = std::time::Instant::now();
            self.drive().await?;
            self.spent("drive", t0);
            let t0 = std::time::Instant::now();
            self.deliver()?;
            self.spent("deliver", t0);
            let t0 = std::time::Instant::now();
            self.check_latches()?;
            self.spent("latches", t0);
            let t0 = std::time::Instant::now();
            self.nothing_else_waits()?;
            self.spent("waits", t0);
        }
        self.quiesce().await?;
        if self.p.verbose {
            for (phase, d) in &self.spent {
                eprintln!("  time in {phase}: {} ms", d.as_millis());
            }
        }
        Ok(())
    }

    /// Nothing the sim does asks the operator anything but a held post: a
    /// call or a budget that waits would stop its session, and the run would
    /// stop exercising it. Such a question is the sim's own error.
    fn nothing_else_waits(&self) -> Result<()> {
        let waiting = self.core.confirm_list()?;
        match waiting
            .iter()
            .find(|c| c.tool != theseus_protocol::HELD_POST_TOOL)
        {
            Some(c) => Err(anyhow!(
                "step {}: {} waits for the operator in session {} ({}): the sim never asks that",
                self.step,
                c.tool,
                c.session_id,
                c.reason
            )),
            None => Ok(()),
        }
    }

    /// The binding connects: it binds a session to each place, and reads who
    /// views every channel.
    fn connect(&mut self) -> Result<()> {
        let (people, channels): (Vec<u64>, Vec<u64>) = {
            let s = self.lock();
            (s.people.clone(), s.channels.iter().map(|c| c.id).collect())
        };
        let mut kinds = vec![PlaceKind::Cli];
        // The owner's DM, and DMs with two others.
        kinds.extend(people.iter().take(3).map(|u| PlaceKind::Dm(*u)));
        kinds.extend(channels.iter().map(|c| PlaceKind::Channel(*c)));
        for kind in kinds {
            let sess = self.open_session(kind)?;
            self.places.push((kind, sess));
        }
        let pushes = self.lock().read_all();
        push_all(&self.core, &pushes);
        self.note("connect");
        Ok(())
    }

    /// Answer every held post, run what is runnable, deliver every post, and
    /// check every session's latch.
    async fn quiesce(&mut self) -> Result<()> {
        while !self.held.is_empty() {
            self.answer_held(true)?;
            self.drive().await?;
            self.deliver()?;
        }
        self.drive().await?;
        self.deliver()?;
        self.dirty.extend(0..self.sessions.len());
        self.check_latches()
    }

    fn finish(mut self) -> Report {
        let s = self.lock();
        let (tool_calls, fetches, egress, mid) =
            (s.tool_calls, s.fetches, s.egress, s.mid_turn_changes);
        let (atoms, grants) = (s.atoms.len() as u64, s.atoms.grants() as u64);
        drop(s);
        self.rep.steps = self.p.steps;
        self.rep.sessions = self.sessions.len() as u64;
        self.rep.tool_calls = tool_calls;
        self.rep.fetches = fetches;
        self.rep.egress = egress;
        self.rep.mid_turn_changes = mid;
        self.rep.atoms = atoms;
        self.rep.grants = grants;
        self.rep.trace = format!("{:016x}", self.trace);
        self.rep.clone()
    }
}
