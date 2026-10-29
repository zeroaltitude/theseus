//! The branch builder (theseus-pw1q.8; seams S1, S4, S6): the tool
//! `self.propose_change { item, issue, why }`, which Theseus calls in a
//! session whose principal is `"self"` to change its own source on a branch
//! nobody else uses, and `self.branches`, the list of what it built.
//!
//! The tool is registered only while `[self] mode = "act"`; with the mode
//! off there is no tool. It runs only in a `"self"`-principal session (any
//! other is refused, and nothing is written), and asks [`super::gate`]
//! before it starts and between its phases. Two calls make one attempt:
//!
//! 1. The first call for an item opens its tree ([`super::tree::open`]) and
//!    hands it over: the result names the tree, whose directory joins the
//!    workspace roots while `[self]` builds (`toolrun/guarded.rs`), so the
//!    existing file tools edit it by absolute path.
//! 2. The next call commits what the tree holds (the item's id in the
//!    subject), runs main's gate on it through the door, in L1 (the branch's
//!    code runs nowhere else, theseus-pw1q.21.1: `super::jail`), writes the frozen
//!    report at `<state>/self/reports/<branch with / as ->.md` (what, why,
//!    the numbers, the verdict, the cost; it ends `<!-- REPORT COMPLETE -->`
//!    and is never written again), then pushes a green branch when
//!    `[self] remote` is set, and closes the tree. A red one keeps its tree
//!    for another attempt, on a new branch.
//!
//! Every attempt ends with one `self.branch.built` row, red or green, and a
//! halt between phases ends it there, its row naming the phase it stopped
//! before. A keel-guard finding stops the branch at an owner card in the
//! owner's DM and never retries around it: the item's later calls are
//! refused until the owner clears its mark (`<tree>.keel`). It never merges:
//! self-join does, after an independent review.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError, Weak};

use serde::Deserialize;
use serde_json::{json, Value};
use theseus_protocol::rsi::{SelfBranch, SelfBranchesParams, SelfBranchesResult};
use theseus_tools::{Backend, Plan, Retry, Tool, ToolClass, ToolCtx};

use super::tree::{self, GateRun, Place, Tree, Verdict};
use super::Gate;
use crate::fact::rsi_pr::SelfBranchBuilt;
use crate::rpc::Core;
use crate::toolrun::TurnCtx;

pub const PROPOSE: &str = "self.propose_change";
pub const NAMES: [&str; 1] = [PROPOSE];
/// The principal a self step's session runs as (seam S1).
pub const PRINCIPAL: &str = "self";
/// What a call cut by a restart answers when it runs again: the attempt
/// may have run, so nothing is done twice.
pub const INTERRUPTED: &str = "A restart cut this self step short, so it may have run part of \
     the way: call self.propose_change again with the item to see where its tree stands.";
/// How a frozen report ends.
pub const COMPLETE: &str = "<!-- REPORT COMPLETE -->";

/// What the tool reaches the core through, attached once the core is built,
/// and where the self trees live.
#[derive(Default)]
pub struct Board {
    pub(super) core: OnceLock<Weak<Core>>,
    pub(super) state: PathBuf,
    /// The root task of a self session's tree, once found (`own_tree`).
    pub(super) roots: Mutex<std::collections::HashMap<String, String>>,
    /// Added to every git call and the gate's (a test's hermetic git).
    env: Mutex<Vec<(std::ffi::OsString, std::ffi::OsString)>>,
    /// A test's place runs its stand-in gate at L0 unless it asks for L1
    /// (`set_jailed`); a daemon's never does (`Place::jail`).
    #[cfg(test)]
    jailed: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    l1: Mutex<Option<Option<String>>>,
}

impl Board {
    pub fn new(state: PathBuf) -> Self {
        Self {
            state,
            ..Self::default()
        }
    }

    /// The core, once attached (theseus-cudar: what a card and the
    /// evidence tool read the record through).
    pub(crate) fn core(&self) -> Option<Arc<Core>> {
        self.core.get().and_then(Weak::upgrade)
    }

    /// The state dir the self trees, reports and gates live under.
    pub(crate) fn state_dir(&self) -> &Path {
        &self.state
    }

    pub fn attach(&self, core: &Arc<Core>) {
        let _ = self.core.set(Arc::downgrade(core));
    }

    #[cfg(test)]
    pub fn set_env(&self, env: Vec<(std::ffi::OsString, std::ffi::OsString)>) {
        *self.env.lock().unwrap_or_else(PoisonError::into_inner) = env;
    }

    /// Why L1 cannot run a self build here, or `None`: the refusal health
    /// shows (a daemon that runs as root), else `theseusd check`'s own probe,
    /// `/bin/true` started as a job starts.
    #[cfg(not(test))]
    async fn l1_unavailable(&self, core: &Core) -> Option<String> {
        let sandbox = &core.tools.sandbox;
        if let Some(why) = sandbox.refusal() {
            return Some(why.to_string());
        }
        sandbox.self_test().await.why
    }

    /// A test's L1 works unless it says it does not: the unit tests' binary
    /// cannot start a real one (the daemon's tests do).
    #[cfg(test)]
    #[allow(clippy::unused_async, reason = "the daemon's version awaits")]
    async fn l1_unavailable(&self, _: &Core) -> Option<String> {
        self.l1
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .flatten()
    }

    /// A test's answer to "can L1 run here", in place of the probe.
    #[cfg(test)]
    pub fn set_l1(&self, unavailable: Option<&str>) {
        *self.l1.lock().unwrap_or_else(PoisonError::into_inner) =
            Some(unavailable.map(str::to_string));
    }

    #[cfg(test)]
    pub fn set_jailed(&self) {
        self.jailed
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// L1 for a self build: the daemon's sandbox (its floor and its sealed
    /// sets hidden). A test's place has none unless it asks.
    fn jail(&self) -> Option<super::jail::Jail> {
        #[cfg(test)]
        if !self.jailed.load(std::sync::atomic::Ordering::Relaxed) {
            return None;
        }
        let core = self.core.get()?.upgrade()?;
        Some(super::jail::Jail::new(core.tools.sandbox.clone()))
    }

    pub(crate) fn place(&self, repo: &str) -> Place {
        Place {
            repo: crate::config::expand(repo),
            state: self.state.clone(),
            env: self
                .env
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .clone(),
            jail: self.jail(),
            notify: self.notify(),
            stop: None,
        }
    }

    /// The gate's inform notices go to the operator's DM through the core's
    /// outbox, quietly (a badge and the digest, never a ping); the halt's
    /// notice alone is its own kind, which pings (theseus-6s7ll).
    fn notify(&self) -> Option<tree::Notify> {
        let core = self.core.get()?.clone();
        let stage = move |halt: bool, text: String| {
            let Some(core) = core.upgrade() else { return };
            let sent = (|| -> anyhow::Result<()> {
                let (post, frame) = core
                    .outbox
                    .stage_to_operator(None, notice_body(halt, text))?;
                core.store.append(&frame)?;
                core.outbox.posted(&post);
                Ok(())
            })();
            if let Err(e) = sent {
                tracing::warn!(error = %format!("{e:#}"), "self build: the notice was not posted");
            }
        };
        let halt = stage.clone();
        Some(
            tree::Notify::new(move |text| stage(false, text))
                .with_halt(move |text| halt(true, text)),
        )
    }
}

/// The body of a self build's notice: quiet, or for the halt a `self_halt` post that pings.
pub(super) fn notice_body(halt: bool, text: String) -> serde_json::Value {
    match halt {
        true => json!({"kind": "self_halt", "text": text}),
        false => json!({"kind": "notice", "text": text, "quiet": true}),
    }
}

/// Where the self trees live, which join the workspace roots while
/// `[self] mode = "act"` and a repo is set: what the file tools edit.
pub fn trees_root(cfg: &crate::Config, state: &Path) -> Option<PathBuf> {
    let s = &cfg.self_improve;
    (s.mode == theseus_protocol::rsi::SelfMode::Act && s.repo.is_some())
        .then(|| state.join("self").join("trees"))
}

/// Register `self.propose_change`, only while `[self] mode = "act"`.
pub fn register(cfg: &crate::Config, r: &mut theseus_tools::Registry) {
    if cfg.self_improve.mode == theseus_protocol::rsi::SelfMode::Act {
        r.register(Arc::new(ProposeChange));
    }
}

/// `self.propose_change`.
pub struct ProposeChange;

impl Tool for ProposeChange {
    fn name(&self) -> &str {
        PROPOSE
    }

    fn description(&self) -> &str {
        "Change Theseus's own source on a branch of its own. The first call for an item opens a \
         git worktree off origin/main, outside every other tree, and names its directory: edit \
         it there with the file tools, by absolute path. The next call for the item commits what \
         the tree holds, runs main's gate on it, writes a frozen report, and pushes the branch \
         to self/* when a remote is set; a red gate keeps the tree for another attempt. The \
         gate's verdict is the result, never this call's words. A keel finding stops the item \
         until the owner answers. Nothing here merges. Only a self session may call it; the \
         kill switch stops it between phases."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "item": {"type": "string", "description": "The backlog item's id: the tree's name, and in every commit's subject."},
                "issue": {"type": "string", "description": "The issue it answers."},
                "why": {"type": "string", "description": "What the change does and why, in a sentence or two: the commit's subject and the report's why."}
            },
            "required": ["item", "issue", "why"],
            "additionalProperties": false
        })
    }

    fn class(&self) -> ToolClass {
        ToolClass::Run
    }

    fn backend(&self) -> Backend {
        Backend::Harness
    }

    fn retry(&self) -> Retry {
        Retry::NonRepeatable
    }

    fn plan(&self, input: &Value, _ctx: &ToolCtx) -> Result<Plan, String> {
        let i = input_of(input)?;
        Ok(Plan {
            summary: format!("self branch for {} ({})", i.item, i.issue),
            ..Default::default()
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    item: String,
    issue: String,
    why: String,
}

#[cfg(test)]
#[path = "tests_pr/gated.rs"]
mod tests_gated;

fn input_of(v: &Value) -> Result<Input, String> {
    let i: Input = serde_json::from_value(v.clone()).map_err(|e| format!("invalid input: {e}"))?;
    if i.item.trim().is_empty() || i.why.trim().is_empty() {
        return Err("item and why must not be empty".into());
    }
    Ok(i)
}

/// The phases, as a stopped row names the one it stopped before.
const COMMIT: &str = "commit";
const GATE: &str = "gate";
const PUSH: &str = "push";

/// One attempt's state, as its row and report say it.
struct Attempt<'a> {
    i: &'a Input,
    tree: Tree,
    head: Option<String>,
    gate: Option<GateRun>,
    report: Option<String>,
    pushed: Option<String>,
    stopped_at: Option<&'static str>,
    verdict: &'static str,
    diff: (u64, u64, u64),
}

/// Run `self.propose_change` for the call of `tc`. An error is the result
/// the model reads.
pub async fn propose(
    board: &Board,
    tc: &TurnCtx<'_>,
    input: &Value,
) -> Result<(String, Value), String> {
    let i = input_of(input)?;
    let core = board
        .core
        .get()
        .and_then(Weak::upgrade)
        .ok_or("the core is not attached")?;
    let principal = crate::task_graph::principal_of(tc.kernel, tc.execution_id);
    if principal != PRINCIPAL {
        return Err(format!(
            "Not run: {PROPOSE} runs only in a self session (principal \"self\"), and this \
             session's is \"{principal}\"."
        ));
    }
    // Off: no tool at all; halted, before a tree is open: nothing begins.
    let halted_now = stopped(&core);
    let Some(repo) = core.cfg.self_improve.repo.clone() else {
        return Err("Not run: [self] repo is unset, so there are no self branches.".into());
    };
    // A self build's code (its gate, its tests, its build) runs in L1 and
    // nowhere else (theseus-pw1q.21.1), so a sealed set is out of its reach:
    // it builds whether or not one is set. A host that cannot run L1 builds
    // nothing.
    if halted_now.is_none() {
        if let Some(why) = board.l1_unavailable(&core).await {
            return Err(format!(
                "Not run: a self build runs its code in L1 and nowhere else, and L1 cannot run on \
             this host: {why}"
            ));
        }
    }
    let mut place = board.place(&repo);
    place.stop = Some(stop_of(&core, tc.execution_id));
    let (p, item) = (place.clone(), i.item.clone());
    let found = blocking(move || Ok(tree::find(&p, &item))).await?;
    let Some(mut t) = found else {
        if let Some(why) = halted_now {
            return Err(format!("Not run: {why}."));
        }
        let (p, item) = (place.clone(), i.item.clone());
        let now = theseus_protocol::now_unix_ms();
        let t = blocking(move || tree::open(&p, &item, now)).await?;
        return Ok((
            format!(
                "Tree open for {} at {} on branch {}, off origin/main, with its own build tree. \
                 Edit it there with the file tools, by absolute path; then call {PROPOSE} \
                 again with item {:?} to commit, gate, report and push.",
                i.item,
                t.dir.display(),
                t.branch,
                i.item
            ),
            json!({"tree": t.dir, "branch": t.branch, "target": t.target, "phase": "open"}),
        ));
    };
    if keel_mark(&place, &t).exists() {
        return Err(format!(
            "Not run: {} stopped at the keel and waits for the owner's answer; it is never \
             retried around.",
            i.item
        ));
    }
    // A branch that has its report is done: another attempt is a new branch.
    if report_path(&place, &t.branch).exists() {
        let p = place.clone();
        t = blocking(move || {
            tree::next_branch(&p, &mut t)?;
            Ok(t)
        })
        .await?;
    }
    let mut a = Attempt {
        i: &i,
        tree: t,
        head: None,
        gate: None,
        report: None,
        pushed: None,
        stopped_at: None,
        verdict: "halted",
        diff: (0, 0, 0),
    };
    let said = attempt(&core, &place, tc, &mut a).await;
    super::infra::watch(
        &core,
        &place,
        &a.tree.branch,
        super::infra::of_attempt(a.verdict, a.gate.as_ref()),
        a.gate.as_ref().and_then(|g| g.exit),
    );
    built(tc, &a);
    said
}

/// Why a self step may not run now; none when it may.
fn stopped(core: &Core) -> Option<String> {
    match super::gate(&core.self_ctx()) {
        Gate::Allowed => None,
        Gate::Off => Some("[self] mode is \"off\"".into()),
        Gate::Halted { why, .. } => Some(format!("self-improvement is halted ({why})")),
    }
}

/// The stop check of a self build's wait for may-build: the kill switch (the
/// same one asked between phases) or a `/stop` of this call's execution.
pub(super) fn stop_of(core: &Arc<Core>, execution_id: &str) -> tree::Stop {
    let (core, exec) = (Arc::downgrade(core), execution_id.to_string());
    tree::Stop::new(move || {
        let core = core.upgrade()?;
        stopped(&core).or_else(|| {
            let e = core.kernel.execution(&exec).ok()??;
            e.stopped.map(|s| format!("stopped by {}", s.by))
        })
    })
}

/// The attempt's phases after the tree is open: commit, gate, report, push.
async fn attempt(
    core: &Core,
    place: &Place,
    tc: &TurnCtx<'_>,
    a: &mut Attempt<'_>,
) -> Result<(String, Value), String> {
    // The stop check reaches the wait through one line of wiring in
    // `propose` (`place.stop = Some(stop_of(..))`), and both halves of the
    // mechanism are tested apart from it. This is the seam: the place handed
    // on to `tree::gate` below, and from there to the wait, must carry the
    // check (theseus-rexxp join fix).
    #[cfg(test)]
    assert!(
        place.stop.is_some(),
        "propose hands the gate a place with no stop check: the owner's stop \
         would end no wait"
    );
    if let Some(why) = stopped(core) {
        return halted(a, COMMIT, &why);
    }
    let (p, t) = (place.clone(), a.tree.clone());
    let subject = subject(a.i);
    let body = format!(
        "{}\n\nIssue: {}\nItem: {}\n\nCo-Authored-By: Theseus (self) <noreply@anthropic.com>",
        a.i.why.trim(),
        a.i.issue.trim(),
        a.i.item.trim()
    );
    let (head, commits, diff) = blocking(move || {
        let head = tree::commit(&p, &t, &subject, &body)?;
        Ok((head, tree::commits(&p, &t)?, tree::diffstat(&p, &t)?))
    })
    .await?;
    if commits.is_empty() {
        a.verdict = "empty";
        return Err(format!(
            "Nothing to build: the tree at {} holds no change against origin/main. Edit it \
             first.",
            a.tree.dir.display()
        ));
    }
    a.head = head.or_else(|| commits.first().map(|c| c.0.clone()));
    a.diff = diff;
    if let Some(why) = stopped(core) {
        return halted(a, GATE, &why);
    }
    let run = match gate_ran(a, tree::gate(place, &a.tree).await) {
        Ok(run) => run,
        Err(out) => return out,
    };
    a.verdict = run.verdict.name();
    a.gate = Some(run.clone());
    a.report = Some(write_report(core, place, tc, a, &commits).map_err(|e| format!("{e:#}"))?);
    match run.verdict {
        Verdict::Keel => return keel(core, place, tc, a),
        Verdict::Red => return Ok(said(a, "The tree stays open for another attempt.")),
        Verdict::Green => {}
    }
    if let Some(why) = stopped(core) {
        return halted(a, PUSH, &why);
    }
    let mut after = String::new();
    if let Some(remote) = core.cfg.self_improve.remote.clone() {
        let (p, t, b) = (place.clone(), a.tree.clone(), a.tree.branch.clone());
        let scrubber = core.tools.scrubber.clone();
        let pushed = blocking(move || {
            Ok(tree::push(&p, &t, &remote, &b, &|s| scrubber.scrub(s).1)
                .map(|spec| format!("{remote} {spec}")))
        })
        .await?;
        match pushed {
            Ok(to) => {
                after = format!("Pushed to {to}. ");
                a.pushed = Some(to);
            }
            Err(e) => {
                a.stopped_at = Some(PUSH);
                after = format!("Not pushed: {e}. ");
            }
        }
    } else {
        after.push_str("[self] remote is unset, so the branch stays local. ");
    }
    let (p, t) = (place.clone(), a.tree.clone());
    let closed = blocking(move || tree::close(&p, &t)).await;
    after.push_str(match closed {
        Ok(()) => "The tree and its build tree are closed; the branch stays.",
        Err(_) => "The tree could not be closed.",
    });
    Ok(said(a, &after))
}

/// The commit's subject: the change's first line, then the item.
fn subject(i: &Input) -> String {
    let first = i.why.trim().lines().next().unwrap_or("").trim();
    let first: String = first.chars().take(64).collect();
    format!("self: {} ({})", first.trim_end_matches('.'), i.item.trim())
}

/// What the attempt does with the gate's outcome: go on with the run, or end
/// with the way out. A stop is a value of its own, `Gated::Stopped`, so no
/// error layer on the way out of the gate can turn a stopped build into a
/// failed one (theseus-tsfkm); the compiler holds this arm.
fn gate_ran(
    a: &mut Attempt<'_>,
    gated: anyhow::Result<tree::Gated>,
) -> Result<GateRun, Result<(String, Value), String>> {
    match gated {
        Ok(tree::Gated::Ran(run)) => Ok(run),
        Ok(tree::Gated::Stopped(why)) => Err(halted(a, GATE, &why)),
        Err(e) => {
            a.verdict = "failed";
            Err(Err(format!("The gate did not run: {e:#}")))
        }
    }
}

/// A halt between phases: the row says where.
fn halted(a: &mut Attempt<'_>, before: &'static str, why: &str) -> Result<(String, Value), String> {
    a.verdict = "halted";
    a.stopped_at = Some(before);
    Err(format!(
        "Stopped before the {before}: {why}. The tree stays at {}.",
        a.tree.dir.display()
    ))
}

/// The keel card's words (theseus-x8b9k).
pub(crate) fn keel_text(branch: &str, item: &str, report: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Decision,
        &format!("Ack or drop self branch {branch}"),
        &format!("it stopped at the keel ({item}): main's keel guard found what needs your yes"),
        &format!(
            "It waits for you and is never retried around. Its report: {report}. Read it, then ack \
             the finding with a signed Keel: trailer, or drop the branch."
        ),
    )
}

/// The keel stopped it: its mark, so nothing retries around it, and the
/// owner's card.
fn keel(
    core: &Core,
    place: &Place,
    tc: &TurnCtx<'_>,
    a: &mut Attempt<'_>,
) -> Result<(String, Value), String> {
    a.stopped_at = Some(PUSH);
    let _ = std::fs::write(keel_mark(place, &a.tree), &a.tree.branch);
    let text = keel_text(
        &a.tree.branch,
        &a.i.item,
        a.report.as_deref().unwrap_or("none"),
    );
    let card = (|| -> anyhow::Result<()> {
        let body = json!({"kind": "self_keel", "text": text});
        let (post, frame) = core.outbox.stage_to_operator(Some(tc.session_id), body)?;
        core.store.append(&frame)?;
        core.outbox.posted(&post);
        Ok(())
    })();
    if let Err(e) = card {
        tracing::warn!(error = %format!("{e:#}"), "self-pr: the keel card was not posted");
    }
    Ok(said(
        a,
        "It stopped at the keel: the owner's card asks for an answer.",
    ))
}

/// The result the model reads: the gate's verdict and where to look.
fn said(a: &Attempt<'_>, after: &str) -> (String, Value) {
    let g = a.gate.as_ref();
    let text = format!(
        "{}: the gate is {} (exit {}, {} s); report {}. {after}",
        a.tree.branch,
        a.verdict,
        g.and_then(|g| g.exit)
            .map_or_else(|| "none".to_string(), |e| e.to_string()),
        g.map_or(0, |g| g.secs),
        a.report.as_deref().unwrap_or("none"),
    );
    let meta = json!({
        "branch": a.tree.branch, "head": a.head, "gate": a.verdict,
        "report": a.report, "log": g.map(|g| g.log.clone()), "pushed": a.pushed,
        "tail": g.filter(|g| g.verdict != Verdict::Green).map(|g| g.tail.clone()),
    });
    (text, meta)
}

/// Every attempt's one row.
fn built(tc: &TurnCtx<'_>, a: &Attempt<'_>) {
    if a.verdict == "empty" {
        return;
    }
    let g = a.gate.as_ref();
    tc.record(&SelfBranchBuilt {
        item: &a.i.item,
        issue: &a.i.issue,
        branch: &a.tree.branch,
        head: a.head.as_deref(),
        gate: a.verdict,
        report: a.report.as_deref(),
        cost_usd: cost_usd(tc),
        stopped_at: a.stopped_at,
        why: &a.i.why,
        numbers: built_numbers(g, a.diff, a.pushed.is_some()),
    });
}

/// A `self.branch.built` row's numbers. `infra::of_row` reads `exit`,
/// `gate_secs` and `unrunnable` back out of them by these key names.
pub(super) fn built_numbers(g: Option<&GateRun>, diff: (u64, u64, u64), pushed: bool) -> Value {
    json!({
        "gate_secs": g.map(|g| g.secs), "exit": g.and_then(|g| g.exit),
        "unrunnable": g.is_some_and(|g| g.unrunnable),
        "files": diff.0, "added": diff.1, "removed": diff.2,
        "pushed": pushed,
    })
}

/// The self session's model spend so far.
fn cost_usd(tc: &TurnCtx<'_>) -> f64 {
    tc.kernel
        .execution(tc.execution_id)
        .ok()
        .flatten()
        .map_or(0.0, |e| e.budget.spent_micros as f64 / 1_000_000.0)
}

fn report_path(p: &Place, branch: &str) -> PathBuf {
    p.reports().join(format!("{}.md", branch.replace('/', "-")))
}

fn keel_mark(p: &Place, t: &Tree) -> PathBuf {
    p.trees().join(format!("{}.keel", t.slug))
}

/// The frozen report (seam S4): written once, read-only, scrubbed.
fn write_report(
    core: &Core,
    place: &Place,
    tc: &TurnCtx<'_>,
    a: &Attempt<'_>,
    commits: &[(String, String)],
) -> anyhow::Result<String> {
    let g = a
        .gate
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no gate run"))?;
    let path = report_path(place, &a.tree.branch);
    if path.exists() {
        anyhow::bail!("{} is frozen already", path.display());
    }
    let commits: String = commits
        .iter()
        .map(|(h, s)| format!("- `{}` {s}\n", &h[..h.len().min(12)]))
        .collect();
    let remote = core.cfg.self_improve.remote.as_deref().map_or_else(
        || "unset: the branch stays local".to_string(),
        |r| format!("{r}, pushed after this report when the gate is green"),
    );
    let text = format!(
        "# Self branch {branch}\n\n\
         - Item: {item}\n- Issue: {issue}\n- Branch: {branch}\n- Head: {head}\n\
         - Gate: {verdict}\n- Exit: {exit}\n- Gate seconds: {secs}\n\
         - Judged by: main's scripts/gate.sh, blob {blob}, run through the door\n\
         - Gate log: {log}\n- Change: {files} files, +{added} -{removed}\n\
         - Cost: ${cost:.4} (the self session's model spend so far)\n- Remote: {remote}\n\n\
         ## What and why\n\n{why}\n\n## Commits\n\n{commits}\n\
         ## The gate's last lines\n\n```text\n{tail}\n```\n\n{COMPLETE}\n",
        branch = a.tree.branch,
        item = a.i.item.trim(),
        issue = a.i.issue.trim(),
        head = a.head.as_deref().unwrap_or("none"),
        verdict = g.verdict.name(),
        exit = g.exit.map_or_else(|| "none".into(), |e| e.to_string()),
        secs = g.secs,
        blob = g.blob,
        log = g.log.display(),
        files = a.diff.0,
        added = a.diff.1,
        removed = a.diff.2,
        cost = cost_usd(tc),
        why = a.i.why.trim(),
        tail = g.tail,
    );
    let (text, _) = core.tools.scrubber.scrub(&text);
    std::fs::create_dir_all(place.reports())?;
    let tmp = path.with_extension("md.tmp");
    std::fs::write(&tmp, text)?;
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o444))?;
    std::fs::rename(&tmp, &path)?;
    Ok(path.display().to_string())
}

/// Blocking work (git) on the blocking pool; an error is the model's words.
async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> anyhow::Result<T> + Send + 'static,
) -> Result<T, String> {
    tokio::task::spawn_blocking(f)
        .await
        .map_err(|e| format!("the self step's work stopped: {e}"))?
        .map_err(|e| format!("{e:#}"))
}

/// A report's `- <key>: <value>` line.
fn report_field(text: &str, key: &str) -> Option<String> {
    let p = format!("- {key}: ");
    text.lines()
        .find_map(|l| l.strip_prefix(&p))
        .map(|v| v.trim().to_string())
}

impl Core {
    /// `self.branches`: the repo's `self/*` branches, each with its open
    /// tree, its frozen report and the gate's verdict there.
    pub fn self_branches(&self, _p: &SelfBranchesParams) -> anyhow::Result<SelfBranchesResult> {
        let s = &self.cfg.self_improve;
        let mut out = SelfBranchesResult {
            repo: s.repo.clone(),
            remote: s.remote.clone(),
            branches: Vec::new(),
            state: self.self_state(),
        };
        let Some(repo) = &s.repo else {
            return Ok(out);
        };
        let place = self.tools.self_pr.place(repo);
        let mut trees = std::collections::BTreeMap::new();
        for e in std::fs::read_dir(place.trees())
            .into_iter()
            .flatten()
            .flatten()
        {
            if e.path().join(".git").exists() {
                if let Ok(b) = place.git_ok(&e.path(), &["rev-parse", "--abbrev-ref", "HEAD"]) {
                    trees.insert(b, e.path().display().to_string());
                }
            }
        }
        for (branch, head) in tree::branches(&place)? {
            let report = report_path(&place, &branch);
            let text = std::fs::read_to_string(&report).ok();
            out.branches.push(SelfBranch {
                item: text.as_deref().and_then(|t| report_field(t, "Item")),
                gate: text.as_deref().and_then(|t| report_field(t, "Gate")),
                report: text.is_some().then(|| report.display().to_string()),
                tree: trees.get(&branch).cloned(),
                head: Some(head),
                branch,
            });
        }
        Ok(out)
    }
}
