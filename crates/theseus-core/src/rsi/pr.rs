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

use super::rerun;
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
    /// The scratch of each review that is running, by its session (`own_removals`).
    pub(super) reviews: Mutex<std::collections::HashMap<String, PathBuf>>,
    /// Added to every git call and the gate's (a test's hermetic git).
    env: Mutex<Vec<(std::ffi::OsString, std::ffi::OsString)>>,
    /// A test's fixer and main (`fixing`); none in a daemon, which uses its own.
    fix: Mutex<Option<fixing::GateSeams>>,
    /// Each session's role, once read (`super::roles`): none for a session the loop did not open
    /// for one job.
    roles: Mutex<std::collections::HashMap<String, Option<super::roles::Role>>>,
    /// The branches whose fix runs now in this process: a fix row with no end is cut short only
    /// when its branch is not here (`fixing::resumed`).
    live_fixes: Mutex<std::collections::HashSet<String>>,
    /// A disk failure cannot authorize another attempt while this daemon is alive.
    failed_keel_holds: Mutex<std::collections::HashSet<String>>,
    /// A test's switch (`set_role_ceilings_off`); never set in a daemon.
    role_ceilings_off: std::sync::atomic::AtomicBool,
    /// A test's switch (`set_index_unread`): the fix rows read as a ledger index still being
    /// built reads; never set in a daemon.
    index_unread: std::sync::atomic::AtomicBool,
    /// A test's switch (`set_resets_fail`): how many resets of a self tree after a fix fail;
    /// never set in a daemon.
    resets_fail: std::sync::atomic::AtomicUsize,
    /// A test's place runs its stand-in gate at L0 unless it asks for L1
    /// (`set_jailed`); a daemon's never does (`Place::jail`).
    #[cfg(test)]
    jailed: std::sync::atomic::AtomicBool,
    #[cfg(test)]
    l1: Mutex<Option<Option<String>>>,
    /// The lock on the self loop's trees, once this daemon holds it (issue 41, `trees_lock`): taken at the first
    /// place asked for, and asked again while another daemon holds it.
    trees_owner: Mutex<Option<super::trees_lock::TreesOwner>>,
    /// Whether this daemon has said, once, that another holds the trees.
    trees_told: std::sync::atomic::AtomicBool,
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

    /// Record `role` for `session` (`super::roles`) in the store and in memory, after its session opened: a test's
    /// way. The loop's own producers write a role in the frame that opens its session.
    #[cfg(test)]
    pub(crate) fn set_role(&self, session: &str, role: super::roles::Role) -> anyhow::Result<()> {
        if let Some(core) = self.core() {
            core.store.append(&[role.record(session)?])?;
        }
        self.remember_role(session, role);
        Ok(())
    }

    /// Keep `role` as `session`'s in memory, over anything kept: its record is already in the store (written in
    /// the frame that opened the session, or by [`Self::set_role`]).
    pub(crate) fn remember_role(&self, session: &str, role: super::roles::Role) {
        self.roles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(session.to_string(), Some(role));
    }

    /// `session`'s role as [`Self::role_in`] reads it, a role that cannot be read as none: for tests.
    #[cfg(test)]
    pub(crate) fn role(&self, session: &str) -> Option<super::roles::Role> {
        let core = self.core()?;
        self.role_in(&core.store, session).ok().flatten()
    }

    /// `session`'s role, the one reader of it (its scope, its tools, the builder's verb and the review):
    /// what memory holds, else its own record in `store`, else, for a fork with no record of its own (one
    /// made before forks were given one), its source's, through every fork between; none when there is
    /// none, kept once read. An error, kept nowhere, when a record or a fork's origin cannot be read: a
    /// failed read is never "no role", and a builder's role is never inferred from none.
    pub(crate) fn role_in(
        &self,
        store: &crate::store::Store,
        session: &str,
    ) -> anyhow::Result<Option<super::roles::Role>> {
        if let Some(r) = self
            .roles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(session)
        {
            return Ok(*r);
        }
        let read = super::roles::effective(store, session)?;
        #[cfg(test)]
        tests_role_race::between();
        // What is kept stays small: the sessions with no role are forgotten past a bound (each is
        // read again from the store if it asks again).
        let mut kept = self.roles.lock().unwrap_or_else(PoisonError::into_inner);
        if kept.len() >= ROLES_KEPT {
            kept.retain(|_, r| r.is_some());
        }
        // A role kept while this read was out (`set_role`, `remember_role`) is newer than it: it stays, and is
        // the answer.
        Ok(*kept.entry(session.to_string()).or_insert(read))
    }

    pub(crate) fn failed_keel_hold(&self, item: &str) -> bool {
        self.failed_keel_holds
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(&tree::slug(item))
    }

    /// `branch`'s fix runs now in this process, until what this returns is dropped.
    pub(crate) fn live_fix(&self, branch: &str) -> LiveFix<'_> {
        self.live_fixes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(branch.to_string());
        LiveFix(self, branch.to_string())
    }

    /// Whether `branch`'s fix runs now in this process.
    pub(crate) fn fix_is_live(&self, branch: &str) -> bool {
        self.live_fixes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains(branch)
    }

    /// A test's switch: a role's session is offered what any self session is, so that
    /// `self.propose_change`'s own refusal is shown to hold without the role's ceiling.
    #[cfg(test)]
    pub(crate) fn set_role_ceilings_off(&self) {
        self.role_ceilings_off
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether a role's session has its role's ceiling (always, but in a test that switched it off).
    pub(crate) fn role_ceilings(&self) -> bool {
        !self
            .role_ceilings_off
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// A test's switch: the fix rows read as unreadable, as while the ledger's index is built.
    #[cfg(test)]
    pub(crate) fn set_index_unread(&self) {
        self.index_unread
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }

    /// A test's switch: the next `n` resets of a tree after a fix that did not hold fail.
    #[cfg(test)]
    pub(crate) fn set_resets_fail(&self, n: usize) {
        self.resets_fail
            .store(n, std::sync::atomic::Ordering::Relaxed);
    }

    /// Whether this reset fails (a test's switch; never in a daemon): one of the `n` is used.
    pub(crate) fn reset_fails(&self) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        self.resets_fail
            .fetch_update(Relaxed, Relaxed, |n| n.checked_sub(1))
            .is_ok()
    }

    /// Whether the fix rows are read as unreadable (a test's switch; never in a daemon).
    pub(crate) fn index_unread(&self) -> bool {
        self.index_unread.load(std::sync::atomic::Ordering::Relaxed)
    }

    /// How many sessions' roles this process keeps.
    #[cfg(test)]
    pub(crate) fn roles_kept(&self) -> usize {
        self.roles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// What this process remembers of the roles, gone: as after a restart.
    #[cfg(test)]
    pub(crate) fn forget_roles(&self) {
        self.roles
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clear();
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
            owner: self.trees_owner(),
        }
    }

    /// The lock on the trees, taken now if this daemon does not hold it yet; none while another daemon holds it
    /// (said once: this daemon then opens no phase tree and sweeps none).
    fn trees_owner(&self) -> Option<super::trees_lock::TreesOwner> {
        let mut held = self
            .trees_owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if held.is_none() {
            match super::trees_lock::take(&self.state.join("self")) {
                Ok(o) => *held = Some(o),
                Err(why) => {
                    if !self
                        .trees_told
                        .swap(true, std::sync::atomic::Ordering::Relaxed)
                    {
                        tracing::warn!(%why, "self: {why}: this daemon opens no phase tree and sweeps none");
                    }
                }
            }
        }
        held.clone()
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

/// How many sessions' roles `Board::role` keeps before it forgets those with none.
const ROLES_KEPT: usize = 4_096;

/// A fix that runs now (`Board::live_fix`): its branch is no longer live once this is dropped.
pub(crate) struct LiveFix<'a>(&'a Board, String);

impl Drop for LiveFix<'_> {
    fn drop(&mut self) {
        self.0
            .live_fixes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.1);
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
#[cfg(test)]
#[path = "tests_pr/legacy_fork.rs"]
mod tests_legacy_fork;

/// A seam between a role's read and its keeping, for a test that puts a write there.
#[cfg(test)]
pub(crate) mod tests_role_race {
    use std::cell::RefCell;

    thread_local! {
        static BETWEEN: RefCell<Option<Box<dyn FnOnce()>>> = const { RefCell::new(None) };
    }

    /// Run what was set, once, between the read and the keeping.
    pub(crate) fn between() {
        if let Some(f) = BETWEEN.with(|b| b.borrow_mut().take()) {
            f();
        }
    }

    /// What the next read on this thread runs between its read and its keeping.
    pub(crate) fn set(f: impl FnOnce() + 'static) {
        BETWEEN.with(|b| *b.borrow_mut() = Some(Box::new(f)));
    }
}

#[path = "pr_fix.rs"]
mod fixing;
pub use fixing::GateSeams;

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
    /// The suite's failures that were run once more and passed (`rerun`).
    reran: Vec<String>,
    /// A fix at the gate, when one was tried (issue 36, `fixing`).
    fix: Option<fixing::Note>,
    /// Why the daemon's git would not run in the tree (`tree_git`), when that stopped it.
    refused: Option<String>,
}

/// Run `self.propose_change` for the call of `tc`. An error is the result
/// the model reads.
pub async fn propose(
    board: &Board,
    tc: &TurnCtx<'_>,
    input: &Value,
) -> Result<(String, Value), String> {
    // A session the loop opened as a fixer, an approver or a reviewer builds nothing, whatever it
    // was offered (issue 36, round 2: `super::roles`).
    let i = fixing::builder_input(board, tc.session_id, input)?;
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
    if board.failed_keel_hold(&i.item) {
        return Err(format!(
            "Not run: {} waits for the owner; its keel hold could not be persisted.",
            i.item
        ));
    }
    // An item whose tries ran out builds no more until the owner's retry (issue 6).
    if let Ok(Some(t)) = crate::task_graph::get(&core.store, &i.item) {
        if super::backlog::attempts::capped(&t) {
            return Err(format!(
                "Not run: {} has used all its tries, so Theseus stopped building it. It waits \
                 for the owner's `{}`.",
                i.item,
                theseus_protocol::rsi::retry_command(&i.item)
            ));
        }
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
            "Not run: {} is held for the owner (a keel stop, or a push refused for what must not \
             leave) and waits for the owner's answer; it is never retried around.",
            i.item
        ));
    }
    // A branch that has its report is done: another attempt is a new branch; one whose fix a
    // restart cut short is put back to before the fix first (issue 36, round 2).
    let cut;
    (t, cut) = fixing::prepared(&core, board, &place, t, i.item.trim()).await?;
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
        reran: vec![],
        fix: cut,
        refused: None,
    };
    let said = attempt(&core, board, &place, tc, &mut a).await;
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

/// A tree the daemon's git would not run in (`tree_git`): the attempt stops before its commit,
/// and its row says why. Any other error is returned as it was.
fn refused(a: &mut Attempt<'_>, e: String) -> String {
    if e.contains(super::tree_git::TAMPERED) || e.contains(super::tree_git::NESTED) {
        a.stopped_at = Some(COMMIT);
        a.refused = Some(e.clone());
    }
    e
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
    core: &Arc<Core>,
    board: &Board,
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
    let (p, t, subject) = (place.clone(), a.tree.clone(), subject(a.i));
    // The model's words, with no line that reads as a fix commit's trailer (`fix::neutralised`).
    let body = super::join::fix::neutralised(&format!(
        "{}\n\nIssue: {}\nItem: {}\n\nCo-Authored-By: Theseus (self) <noreply@anthropic.com>",
        a.i.why.trim(),
        a.i.issue.trim(),
        a.i.item.trim()
    ));
    let (head, commits, diff) = blocking(move || committed(&p, &t, &subject, body))
        .await
        .map_err(|e| refused(a, e))?;
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
    // The gate, and a red in the change's own work once to a fixer (issue 36).
    let (run, commits) = match fixing::gated(core, board, place, tc, a, commits).await {
        Ok(r) => r,
        Err(out) => return out,
    };
    a.verdict = run.verdict.name();
    a.gate = Some(run.clone());
    a.report = Some(write_report(core, place, tc, a, &commits).map_err(|e| format!("{e:#}"))?);
    told(core, tc, a, &run);
    match run.verdict {
        Verdict::Keel => return keel(core, place, tc, a),
        Verdict::Red if run.aborted() => {
            return Ok(said(
                a,
                "The check kept aborting, so this attempt is red and uses one try. Something \
                 wrote in your tree while its check ran: the tail names what moved. The tree \
                 stays open for another attempt.",
            ))
        }
        Verdict::Red => return Ok(said(a, "The tree stays open for another attempt.")),
        Verdict::Green => {}
    }
    if let Some(why) = stopped(core) {
        return halted(a, PUSH, &why);
    }
    let mut after = String::new();
    if let Some(remote) = core.cfg.self_improve.remote.clone() {
        let (t, item) = (a.tree.clone(), a.i.item.trim().to_string());
        match push_branch(core, place, t, (item, tc.session_id.to_string()), remote).await? {
            Ok(to) => {
                after = format!("Pushed to {to}. ");
                a.pushed = Some(to);
            }
            // What must not leave holds the item for the owner, as a keel stop does: nothing goes on.
            Err(e) if e.holds() => return withheld(core, place, tc, a, &e),
            // Put off: no hold and no card. The branch is the join's to check again, as one never pushed.
            Err(tree::Refused::Deferred(d)) => {
                tracing::info!(branch = %a.tree.branch, "self.propose_change: the outgoing check is put off: {}", d.words());
                a.stopped_at = Some(PUSH);
                after = format!(
                    "Not pushed: the outgoing check is put off ({}); the join checks it again. ",
                    d.words()
                );
            }
            Err(e) => {
                a.stopped_at = Some(PUSH);
                after = format!("Not pushed: {e}. ");
            }
        }
    } else {
        after.push_str("[self] remote is unset, so the branch stays local. ");
    }
    // The item's other sessions are ended before the tree goes; this one is making the call.
    if let Err(e) = core
        .end_item_sessions(a.i.item.trim(), Some(tc.session_id))
        .await
    {
        tracing::warn!(error = %format!("{e:#}"), "self.propose_change: other sessions were not ended");
    }
    let (p, t) = (place.clone(), a.tree.clone());
    let closed = blocking(move || tree::close(&p, &t)).await;
    after.push_str(match closed {
        Ok(()) => "The tree and its build tree are closed; the branch stays.",
        Err(_) => "The tree could not be closed.",
    });
    Ok(said(a, &after))
}

/// Push tree `t`'s branch to `remote` (`tree::push`), with what the sessions working on `item` read, `session`
/// among them, for the outgoing check: where it went, or why it was refused.
async fn push_branch(
    core: &Arc<Core>,
    place: &Place,
    t: Tree,
    (item, session): (String, String),
    remote: String,
) -> Result<Result<String, tree::Refused>, String> {
    let (p, b) = (place.clone(), t.branch.clone());
    let (store, scrubber) = (core.store.clone(), core.tools.scrubber.clone());
    blocking(move || {
        let until = super::outgoing::clear::deadline();
        let heard = super::outgoing::Heard::of(&store, &item, &[&session], until);
        let heard = heard.as_ref().map_err(|u| *u);
        Ok(
            tree::push(&p, &t, &remote, &b, &|s| scrubber.scrub(s).1, heard)
                .map(|spec| format!("{remote} {spec}")),
        )
    })
    .await
}

/// What the attempt's commit leaves: its head (none when nothing was left to commit), the
/// branch's commits, and its size.
type Committed = (Option<String>, Vec<(String, String)>, (u64, u64, u64));

/// The attempt's commit, formatted first so the gate's formatting phase never fails an attempt.
fn committed(p: &Place, t: &Tree, subject: &str, body: String) -> anyhow::Result<Committed> {
    let body = match super::tree_fmt::format_tree(p, t).line() {
        Some(line) => format!("{body}\n{line}"),
        None => body,
    };
    let head = tree::commit(p, t, subject, &body)?;
    Ok((head, tree::commits(p, t)?, tree::diffstat(p, t)?))
}

/// The commit's subject: the change's first line, then the item.
fn subject(i: &Input) -> String {
    let why = i.why.trim();
    let first = why.lines().next().unwrap_or("").trim_end();
    let cuts = crate::scrub::Cuts::live();
    let (first, _) = crate::scrub::safe::head_with(&cuts, first, 64);
    // The trimmed end is checked again on the whole text, until it rests: a
    // known value may end in a period, or run past the first line.
    let mut at = first.len();
    loop {
        let end = cuts.before(why, why[..at].trim_end_matches('.').len());
        if end == at {
            break;
        }
        at = end;
    }
    format!("self: {} ({})", &why[..at], i.item.trim())
}

/// [`subject`] of an input with `item` and `why`, for the core's tests.
#[cfg(test)]
pub(crate) fn subject_for_tests(item: &str, why: &str) -> String {
    subject(&Input {
        item: item.into(),
        issue: String::new(),
        why: why.into(),
    })
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

/// The gate's verdict on the tree, read as a verdict (`rerun`): a check that aborted is run again,
/// up to [`rerun::ABORT_RERUNS`] more times, and one that kept aborting says so in its own tail; a
/// red suite whose only failures are tests the change cannot reach is run once more on those
/// tests, and when they all pass it is green, with the tests named in the report and the row.
async fn judged(
    a: &mut Attempt<'_>,
    place: &Place,
) -> Result<GateRun, Result<(String, Value), String>> {
    let mut run = gate_ran(a, tree::gate(place, &a.tree).await)?;
    let mut again = 0;
    while run.aborted() && again < rerun::ABORT_RERUNS {
        again += 1;
        run = gate_ran(a, tree::gate(place, &a.tree).await)?;
    }
    if run.aborted() {
        run.tail = format!(
            "gate: the check kept aborting ({} runs, each stopped because its tree moved): it \
             judged nothing about the code\n{}",
            again + 1,
            run.tail
        );
    } else if let Some(tests) = rerun::second_look(place, &a.tree, &run, None).await {
        a.reran = tests.iter().map(rerun::Failed::name).collect();
        run.verdict = Verdict::Green;
        run.tail = format!(
            "gate: the suite failed only on tests outside what this change reaches, and they \
             passed when run again alone: {}\n{}",
            a.reran.join(", "),
            run.tail
        );
    }
    Ok(run)
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

/// The phase the gate failed in, as its own last lines name it
/// (`gate: FAILED in <phase>`); `None` where it named none, as a green gate
/// does.
pub(crate) fn failed_phase(tail: &str) -> Option<String> {
    tail.lines()
        .rev()
        .find_map(|l| l.trim().strip_prefix("gate: FAILED in "))
        .map(|p| p.trim().to_string())
}

/// Every surface the owner looks at hears how the build ended (theseus-1eemp).
/// Until this, the loop pushed and posted only on green: 126 self reports since
/// 2026-10-20, none of them green, and the only thing ever said of them was the
/// backlog's line that work had started. Three places, none of them new:
///
/// - **where the item reports** - its own thread when its task has one, the
///   operator's DM when it has none - at once: the verdict, the phase it
///   failed in and the frozen report to read;
/// - **the item's task record**, so the forum post that stands for this build
///   carries the gate's verdict as evidence, and not only "its build's turn
///   ended with no green branch" hours later, when the session ends
///   (`rsi::pipeline::pipeline_ended`);
/// - **the night's or the morning's note**, through the fold the starts
///   already go through (`backlog::folded`), so the owner reads the end beside
///   the start and no note goes out on its own account.
///
/// A failure in any one of them is logged, never raised: the gate's verdict
/// stands whatever the telling did.
fn told(core: &Core, tc: &TurnCtx<'_>, a: &Attempt<'_>, run: &GateRun) {
    let task = crate::task_graph::get(&core.store, &a.i.item).unwrap_or_else(|e| {
        tracing::warn!(error = %format!("{e:#}"), "self-pr: the item's task was not read");
        None
    });
    let e = crate::rsi::backlog::Ended {
        item: a.i.item.clone(),
        title: task
            .as_ref()
            .map_or_else(|| a.i.item.clone(), |t| t.title.clone()),
        verdict: run.verdict.name().to_string(),
        phase: if run.aborted() {
            Some("a check that kept aborting".into())
        } else {
            failed_phase(&run.tail)
        },
        report: a.report.clone(),
    };
    // A red notice already went out for this item tonight: this one is only in the note.
    let quiet = task
        .as_ref()
        .is_some_and(crate::rsi::backlog::attempts::red_tonight);
    let fixed = a.fix.as_ref().map(|f| f.words.as_str());
    let capped = told_on_task(core, task, &e, run, fixed, tc.session_id);
    told_where_it_reports(core, tc, &e, run, quiet);
    if let Some(text) = capped {
        core.stage_self_digest(&text);
    }
    core.folded_add_ended(&e);
}

/// Where the item reports, the moment its gate ends: **its own thread** when
/// its task has one (`Outbox::target`), so the words land under the post the
/// owner already has open for it, and **the operator's DM** when it has none -
/// which on the live daemon is every self build.
///
/// A self item's session is in none of the three ways `Outbox::try_target`
/// resolves one - no place runs on it, no task record names it, no thread
/// shows at it - so the early return this opened with was taken every time:
/// **`self_build_red` appears 0 times in the owner's store**, index and WAL
/// both, across 126 self builds since 2026-10-20. The owner, 2026-11-10 08:35:
/// *"theseus said it started work on something, but there were no new task
/// records in the forum or in the task thread"*. The note and the task record
/// were the other two surfaces and both reached him; this is the third.
///
/// The operator's lane is where the rest of the self loop already speaks
/// (`backlog::start::stage_self_digest`, and `keel`'s card below), and this
/// takes it on the same terms: guarded by the same `owner_dm_bound`, so a
/// headless daemon says nothing, as before. **It binds no session to a
/// place**, so the build stays the `Cli`-kind turn it is, with its own tool
/// budget and its own ledger bucket, and nothing else that reads
/// `Outbox::target` changes meaning. `Outbox::refuse_unbound` exempts
/// `OPERATOR_TARGET` by name, so the post is never refused out from under the
/// loop. When a self item does get a post of its own, this `match` is the one
/// thing that changes.
fn told_where_it_reports(
    core: &Core,
    tc: &TurnCtx<'_>,
    e: &crate::rsi::backlog::Ended,
    run: &GateRun,
    quiet: bool,
) {
    if quiet && run.verdict == Verdict::Red {
        return;
    }
    let target = core.outbox.target(tc.session_id);
    if target.is_none() && !core.runner.place_rule.owner_dm_bound() {
        return;
    }
    let text = format!(
        "{} (exit {}, {} s). {}",
        e.line(),
        run.exit.map_or_else(|| "none".into(), |c| c.to_string()),
        run.secs,
        match run.verdict {
            Verdict::Green => "It is pushed from here.",
            Verdict::Keel => "It waits for the owner's word; nothing retries around it.",
            Verdict::Red => "The tree stays open for another attempt.",
        }
    );
    let kind = match run.verdict {
        Verdict::Green => "self_build",
        _ => "self_build_red",
    };
    let staged = (|| -> anyhow::Result<()> {
        let body = json!({"kind": kind, "item": e.item, "verdict": e.verdict, "text": text});
        let (post, frame) = match &target {
            Some(t) => core.outbox.stage(tc.session_id, tc.execution_id, t, body)?,
            None => core.outbox.stage_to_operator(Some(tc.session_id), body)?,
        };
        core.store.append(&frame)?;
        core.outbox.posted(&post);
        Ok(())
    })();
    if let Err(err) = staged {
        tracing::warn!(error = %format!("{err:#}"), "self-pr: where the item reports was not told");
    }
}

/// The item's task record: the gate's verdict as evidence, at the moment it is
/// known. The state is left alone - a red build's item is still in progress,
/// and the pipeline closes it when its turn ends - so this adds the record the
/// forum shows and takes nothing away.
fn told_on_task(
    core: &Core,
    _task: Option<theseus_protocol::tasks::TaskRecord>,
    e: &crate::rsi::backlog::Ended,
    run: &GateRun,
    fixed: Option<&str>,
    session: &str,
) -> Option<String> {
    // Never publish the snapshot taken before the owner could drop or retry it.
    let _one = core
        .rsi
        .backlog
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let mut t = match crate::task_graph::get(&core.store, &e.item) {
        Ok(task) => task?,
        Err(err) => {
            tracing::warn!(error = %format!("{err:#}"), "self-pr: the item's task was not reread");
            return None;
        }
    };
    if t.state != theseus_protocol::tasks::TaskState::InProgress
        || t.evidence
            .iter()
            .rev()
            .find_map(|e| e.identity.strip_prefix("session:"))
            .is_some_and(|started| started != session)
    {
        return None;
    }
    let now = theseus_protocol::now_unix_ms();
    let unjudged = super::infra::of_attempt(&e.verdict, Some(run))
        == Some(super::infra::Outcome::Infrastructure);
    t.version += 1;
    t.updated_at_ms = now;
    let kept_aborting = run.aborted().then_some("its check kept aborting; ");
    t.evidence.push(theseus_protocol::tasks::TaskEvidence {
        node: None,
        identity: format!("gate:{}", e.verdict),
        note: Some(format!(
            "{}{} (exit {}, {} s){}",
            kept_aborting.unwrap_or_default(),
            e.line(),
            run.exit.map_or_else(|| "none".into(), |c| c.to_string()),
            run.secs,
            fixed.map(|f| format!("; {f}")).unwrap_or_default()
        )),
        by: "self-pr".into(),
        at_ms: now,
    });
    // A gate that never judged the code (it could not run, a command it needs is missing, it ran
    // for no time at all: `infra::of_attempt`) says nothing about the item, so it is no try
    // (`backlog::attempts`): its evidence is followed by one that says so.
    if unjudged {
        t.evidence.push(theseus_protocol::tasks::TaskEvidence {
            node: None,
            identity: crate::rsi::backlog::attempts::UNJUDGED.into(),
            note: Some("the gate did not judge the code, so it is no try".into()),
            by: "self-pr".into(),
            at_ms: now,
        });
    }
    // The try that reaches the cap marks the item and is the one notice for it.
    let capped = (run.verdict == Verdict::Red && !unjudged)
        .then(|| core.cap_check(&mut t, now))
        .flatten();
    let written = crate::task_graph::record(&t).and_then(|r| core.store.append(&[r]));
    if let Err(err) = written {
        tracing::warn!(error = %format!("{err:#}"), "self-pr: the item's task record was not written");
        return None;
    }
    capped
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
///
/// **The mark is unconditional and the card is guarded** (theseus-1eemp). The
/// mark is what stops the next attempt, so it is written whatever the daemon
/// looks like; the card is a post on the operator's lane, and that lane is the
/// owner's DM, so with no DM bound it has nowhere to be delivered. It would
/// not be refused either: `Outbox::refuse_unbound` exempts `OPERATOR_TARGET`
/// by name (`outbox.rs:1017`), so an unguarded card **pends** against a place
/// that will never resolve - a post that is neither delivered nor failed, and
/// so counted in nothing. The same guard the rest of the self loop already
/// takes (`backlog::start::stage_self_digest`, `told_where_it_reports`, and
/// `folded_add_ended` on the note) is the one taken here.
fn keel(
    core: &Core,
    place: &Place,
    tc: &TurnCtx<'_>,
    a: &mut Attempt<'_>,
) -> Result<(String, Value), String> {
    let text = keel_text(
        &a.tree.branch,
        &a.i.item,
        a.report.as_deref().unwrap_or("none"),
    );
    held_for_owner(core, place, tc, a, (text, "It stopped at the keel"))
}

/// A push refused for what must not leave (`tree::Refused::holds`): held for the owner as a keel stop is, by its
/// mark (every later call is refused), its held stop row (the join and the pipeline skip the head:
/// `join::Seen`), written before the attempt's own row, and one card. The card, the row and what the model is
/// told name the refusal's fixed category and safe ids, never a word of what matched; when the branch's own name
/// held the words, they name neither it nor the item.
fn withheld(
    core: &Core,
    place: &Place,
    tc: &TurnCtx<'_>,
    a: &mut Attempt<'_>,
    why: &tree::Refused,
) -> Result<(String, Value), String> {
    use super::outgoing::{Part, Refusal};
    let head = a.head.clone().unwrap_or_default();
    tc.record(&crate::fact::rsi_join::SelfJoinStopped {
        branch: a.tree.branch.clone(),
        head: head.clone(),
        phase: "push".into(),
        why: why.to_string(),
        numbers: json!({"held": true, "outgoing": true, "at": "branch"}),
    });
    let named = !matches!(
        why,
        tree::Refused::Outgoing(Refusal::Words {
            part: Part::Branch,
            ..
        })
    );
    let words = matches!(why, tree::Refused::Outgoing(Refusal::Words { .. }));
    let lift = match named {
        true => format!("remove its hold, {}.keel among the self trees", a.tree.slug),
        false => format!(
            "remove its hold, the `.keel` mark beside the self tree whose branch holds commit {}",
            super::outgoing::short(&head)
        ),
    };
    let names = named.then_some((a.tree.branch.as_str(), a.i.item.as_str()));
    let held = Held {
        exit: Exit::Branch,
        names,
        head: &head,
        repo: &place.repo,
        words,
    };
    let text = withheld_text(&held, why, &lift);
    let stopped = format!("It was not pushed ({why}); it is held");
    let (mut said, mut meta) = held_for_owner(core, place, tc, a, (text, &stopped))?;
    // The branch's own name held the words: what the model is told names neither it nor its tree's slug.
    if !named {
        for name in [a.tree.branch.as_str(), a.tree.slug.as_str()] {
            said = said.replace(name, "[a name withheld]");
        }
        meta = json!({"head": a.head, "gate": a.verdict, "withheld": true});
    }
    Ok((said, meta))
}

/// Where a self push was stopped, for the owner's card.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Exit {
    /// The builder's branch, from its tree (the tree stays).
    Branch,
    /// A fix made at review (its temporary tree is closed).
    Review,
    /// The join to main (the merge's tree is closed).
    Join,
}

/// A held push, as its card names it: never a word of what matched.
pub(crate) struct Held<'a> {
    pub exit: Exit,
    /// The branch and the item, unless the branch's own name held the words.
    pub names: Option<(&'a str, &'a str)>,
    /// The commit that was not sent.
    pub head: &'a str,
    /// The self repository.
    pub repo: &'a std::path::Path,
    /// It was refused for words of a conversation (else: it could not be checked).
    pub words: bool,
}

/// The owner's card for a push refused for what must not leave, true at the exit it was refused at; `lift` says
/// how its hold is lifted.
pub(crate) fn withheld_text(h: &Held<'_>, why: &dyn std::fmt::Display, lift: &str) -> String {
    let what = match h.words {
        true => "a commit holds words from one of your conversations, which a session working on it read",
        false => "what it would send could not be checked",
    };
    let head = super::outgoing::short(h.head);
    let (title, item) = match (h.names, h.exit) {
        (Some((b, _)), Exit::Branch) => (format!("Self branch {b} was not pushed"), true),
        (Some((b, _)), Exit::Review) => (format!("A fix of self branch {b} was not pushed"), true),
        (Some((b, _)), Exit::Join) => (format!("Self branch {b} was not joined to main"), true),
        (None, _) => (
            format!("A self branch at commit {head} was not pushed"),
            false,
        ),
    };
    let item = match (item, h.names) {
        (true, Some((_, i))) => format!(" ({i})"),
        _ => String::new(),
    };
    let on = match h.names {
        Some((b, _)) => format!("commit {head} on {b}"),
        None => format!("commit {head}"),
    };
    let repo = h.repo.display();
    let then = match h.exit {
        Exit::Branch => format!(
            "Nothing of it left this machine, and it is never retried around. The held head is {on}, in its \
             tree among the self trees: drop the item, or take the words out of its commits, then {lift}."
        ),
        Exit::Review => format!(
            "This push was stopped: the new fix was not sent (the branch as it stood before the fix may \
             already be at the remote). The fix is {on} in the self repository; its tree is closed. See it with \
             `git -C {repo} show {head}`, drop the item or change the branch there, then {lift}."
        ),
        Exit::Join => format!(
            "This push was stopped: the merge was not sent, and main at the remote is as it was (the branch \
             itself may already be at the remote). The held head is {on} in the self repository; the merge's \
             tree is closed. See it with `git -C {repo} log -p main..{head}`, drop the item or change the \
             branch there, then {lift}."
        ),
    };
    crate::owner_note::note(
        crate::owner_note::Kind::Decision,
        &title,
        &format!("{what}{item}: {why}"),
        &then,
    )
}

/// Hold the attempt for the owner: the mark that stops every later attempt, and the owner's card `text`;
/// `stopped` begins what the model is told.
fn held_for_owner(
    core: &Core,
    place: &Place,
    tc: &TurnCtx<'_>,
    a: &mut Attempt<'_>,
    (text, stopped): (String, &str),
) -> Result<(String, Value), String> {
    a.stopped_at = Some(PUSH);
    let mut notice = String::new();
    if let Err(e) = std::fs::write(keel_mark(place, &a.tree), &a.tree.branch) {
        core.tools
            .self_pr
            .failed_keel_holds
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(tree::slug(&a.i.item));
        tracing::warn!(error = %e, "self-pr: could not persist the keel hold");
        notice = format!(
            " It could not persist the keel hold ({e}); it holds only until the daemon restarts."
        );
    }
    if !core.runner.place_rule.owner_dm_bound() {
        // Nothing retries around it either way: the mark above is written
        // first and is what holds the branch.
        return Ok(said(
            a,
            &format!(
                "{stopped} and waits for the owner's word; no DM is bound, so no card was \
                 posted.{notice}"
            ),
        ));
    }
    let mut text = text;
    text.push_str(&notice);
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
        &format!("{stopped}: the owner's card asks for an answer.{notice}"),
    ))
}

/// The result the model reads: the gate's verdict and where to look.
fn said(a: &Attempt<'_>, after: &str) -> (String, Value) {
    let g = a.gate.as_ref();
    let fixed = a
        .fix
        .as_ref()
        .map(|f| format!("A fix was tried at the gate: {}. ", f.words))
        .unwrap_or_default();
    let text = format!(
        "{}: the gate is {} (exit {}, {} s); report {}. {fixed}{after}",
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
        numbers: {
            let mut n = built_numbers(g, a.diff, a.pushed.is_some());
            if !a.reran.is_empty() {
                n["reran"] = json!(a.reran);
            }
            if g.is_some_and(GateRun::aborted) {
                n["kept_aborting"] = json!(true);
            }
            if let Some(why) = &a.refused {
                n["refused"] = json!(why);
            }
            n
        },
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
         {fix}## The gate's last lines\n\n```text\n{tail}\n```\n\n{COMPLETE}\n",
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
        fix = a
            .fix
            .as_ref()
            .map(fixing::Note::section)
            .unwrap_or_default(),
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

#[cfg(test)]
mod task_write_tests {
    use super::*;
    #[test]
    fn gate_evidence_keeps_a_newer_owner_state_and_evidence() {
        use theseus_protocol::tasks::{TaskRecord, TaskState};
        let dir = tempfile::tempdir().unwrap();
        let store = crate::store::Store::open(dir.path()).unwrap();
        let mut cfg = crate::Config::example();
        cfg.server.state_dir = dir.path().display().to_string();
        let core = Core::build(crate::rpc::Parts::for_tests(
            cfg,
            std::sync::Arc::new(crate::provider::FakeProvider::default()),
            store,
        ))
        .unwrap();
        let old = TaskRecord {
            id: "self-test".into(),
            state: TaskState::InProgress,
            version: 1,
            ..Default::default()
        };
        let mut dropped = old.clone();
        dropped.version += 1;
        dropped.state = TaskState::Abandoned;
        dropped
            .evidence
            .push(theseus_protocol::tasks::TaskEvidence {
                identity: "owner:drop".into(),
                ..Default::default()
            });
        core.store
            .append(&[crate::task_graph::record(&dropped).unwrap()])
            .unwrap();
        let ended = crate::rsi::backlog::Ended {
            item: old.id.clone(),
            title: "test".into(),
            verdict: "red".into(),
            phase: None,
            report: None,
        };
        let run = GateRun {
            verdict: Verdict::Red,
            exit: Some(1),
            blob: String::new(),
            log: PathBuf::new(),
            secs: 1,
            tail: String::new(),
            unrunnable: false,
        };
        told_on_task(&core, Some(old), &ended, &run, None, "old-build");
        let now = crate::task_graph::get(&core.store, &ended.item)
            .unwrap()
            .unwrap();
        assert_eq!(now.state, TaskState::Abandoned);
        assert!(now.evidence.iter().any(|e| e.identity == "owner:drop"));
    }
}
