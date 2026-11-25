//! The self-join (theseus-pw1q.9; the owner's decision of 2026-10-09: a
//! self-join is a notice after, not a yes before). What stands in for the
//! owner's yes is an independent review on another model with a planted
//! revert, the join lock, one gate on the merge, a plain signed merge, the
//! install and a notice that carries the one-click revert. The hard keel
//! still stops at the owner's card.
//!
//! [`Core::self_join_pass`] runs from the tender ([`tender`], only with
//! `[self] mode = "act"`), and asks [`super::gate`] before it starts and
//! before every phase, stopping on anything but `Allowed` with a
//! `self.join.stopped` row that names the phase. For each green
//! `self.branch.built` row (seam S4), oldest first:
//!
//! 0. **Main's move** ([`moved`], issue 20): a branch not yet reviewed whose dry run on main's
//!    tip conflicts is not reviewed. It goes back to its builder with main merged into a
//!    working copy of it, and none of that is a try.
//! 1. **The review**, once per branch, head and reviewer model. The
//!    reviewer's model must differ from the builder's (the builder's
//!    session's last target): the same model, an unknown builder or an
//!    unknown profile is `refused`, with its reason. The planted revert
//!    ([`planted`], code, not the reviewer) runs first: unproved is a
//!    reject, and no reviewer is paid. Then the check turn ([`review`]).
//!    One `self.review.verdict` row; on a reject the branch stays and the
//!    item goes back to the backlog (`proposed`, the reasons in its
//!    evidence) in the row's frame.
//! 2. **The keel check** ([`keel`]): a class posts the owner's card with the
//!    diff's summary, writes `self.join.stopped` (phase `keel`) and waits;
//!    that head is not tried again until the owner's `theseus self retry`
//!    (`self.join.reopened`, [`joining`]): then the pass runs it as any
//!    accepted branch, every check from this one on, and a head still in the
//!    keel is held again with a new card. Nothing is remembered from the hold.
//! 3. **The join**, one a pass, under the lock ([`lock`]): fetch, the dry
//!    run on the remote's main (`merge-tree`), a plain `--no-ff` merge in a
//!    tree, main's count folded into it ([`stats`], issue 45; a failed count
//!    goes on uncounted, unless its tree cannot be shown put back),
//!    main's gate on it through the door with the benches, the
//!    signature (no `user.signingkey` in `[self] repo`: the owner's card, and
//!    nothing is pushed), the push of main, never forced; main moved before
//!    the push redoes the dry run, the merge and the gate, at most
//!    [`TRIES`] times. A dry run that conflicts with main returns the item to
//!    the backlog with a notice, as step 0 does (where the working copy cannot
//!    be made, its tree is closed and the next build cuts a fresh branch from
//!    the new main), and drops it at the third conflict. The
//!    lock is released with its `done:` line. One `self.joined` row.
//!    **The sealed check** ([`sealing`], theseus-pw1q.21.7) runs on each try's
//!    merge between main's gate and the signature, when the branch's diff
//!    touches a behaviour path (`sealed::hold::BEHAVIOUR`) or `[self.sealed]
//!    every_branch` is set: the merge's build, which main's gate just made,
//!    is scored on every sealed set beside main's cached score, and a family
//!    or total that falls holds the head (`self.join.stopped`, phase
//!    `sealed`, with the numbers) and returns the item to the backlog. With
//!    no sealed set the phase is skipped, saying so in its row.
//! 4. **The install** ([`install`]), only with `[self] install = true`: the
//!    build and the restart go to a transient `systemd-run --user` unit, and
//!    the new daemon writes `self.installed` and the notice
//!    ([`Core::self_installed_after_serving`]).
//!
//! The branch is `[self] remote`'s (`origin` when unset): its main is the
//! base, and main is pushed there.

mod contains;
mod dead;
pub mod dependencies;
pub(crate) mod fix;
pub mod git;
pub mod guard;
pub mod install;
pub(crate) mod joining;
pub mod keel;
pub mod layout;
pub mod lock;
mod moved;
pub mod planted;
mod proof_check;
pub mod review;
mod reviewing;
pub mod root_install;
pub mod scratch;
mod sealing;
mod stats;
mod stats_tree;
mod stopped_planted;
mod tender;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tests_contains;
#[cfg(test)]
mod tests_dead;
#[cfg(test)]
mod tests_dependencies;
#[cfg(test)]
mod tests_install_truth;
#[cfg(test)]
mod tests_installing;
#[cfg(test)]
mod tests_moved;
#[cfg(test)]
mod tests_notify;
#[cfg(test)]
mod tests_planted;
#[cfg(test)]
mod tests_planted_real;
#[cfg(test)]
mod tests_proof_kind;
#[cfg(test)]
mod tests_reap;
#[cfg(test)]
mod tests_retry;
#[cfg(test)]
mod tests_scratch;
#[cfg(test)]
mod tests_sealed;
#[cfg(test)]
mod tests_second_look;
#[cfg(test)]
mod tests_stats;
#[cfg(test)]
mod tests_stats_contain;
#[cfg(test)]
mod tests_unreviewable;
mod unreviewable;
mod verify;
pub mod words;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use self::planted::Tests;
use self::review::Reviewer;
/// The count's `--range` run, as a join runs it (issue 45): public for the real-jail test.
pub use self::stats::range as count_range;
use super::tree::Place;
use super::Gate;
use crate::fact::rsi_join::SelfJoinStopped;
use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// How many times a join redoes its dry run, merge and gate when main moves.
pub const TRIES: u32 = 3;

/// A test's hook, called with a phase's name.
pub type PhaseHook = Arc<dyn Fn(&str) + Send + Sync>;

/// A test's hook, called with the join tree's path once the count is folded into the merge.
pub type FoldHook = Arc<dyn Fn(&std::path::Path) + Send + Sync>;

/// What the pass reaches outside the core: the reviewer, the test runner,
/// the lock's pace, the install, and a test's hooks.
#[derive(Clone)]
pub struct Seams {
    pub reviewer: Arc<dyn Reviewer>,
    pub tests: Arc<dyn Tests>,
    pub installer: Arc<dyn install::Installer>,
    /// Scores a build on the sealed sets (theseus-pw1q.21.7).
    pub sealed: Arc<dyn super::sealed::hold::Scorer>,
    /// The commit of the daemon's own build, which the sealed check scores as main's (`crate::build`).
    pub installed: Arc<dyn Fn() -> Option<String> + Send + Sync>,
    /// Fixes what a review asked to repair, and has the builder's model approve it (issue 36).
    pub fixer: Arc<dyn fix::Fixer>,
    pub lock_wait: Duration,
    pub lock_every: Duration,
    /// Called with each phase's name before its gate is asked.
    pub on_phase: Option<PhaseHook>,
    /// Called with the try's number before each push of main.
    pub before_push: Option<Arc<dyn Fn(u32) + Send + Sync>>,
    /// How long main's counter may run in a join's merge, its two runs together, before it is stopped (issue 45).
    pub stats_limit: Duration,
    /// Called with the join tree's path once the count is folded into the merge, before the fold is checked: a
    /// test's way to break it.
    pub after_fold: Option<FoldHook>,
}

impl Default for Seams {
    fn default() -> Self {
        Self {
            reviewer: Arc::new(review::CheckTurn),
            tests: Arc::new(planted::Cargo),
            installer: Arc::new(install::Systemd),
            sealed: Arc::new(super::sealed::hold::Scratch),
            installed: Arc::new(|| crate::build().commit),
            fixer: Arc::new(fix::turns::Turns),
            lock_wait: Duration::from_secs(2 * 3600),
            lock_every: Duration::from_secs(30),
            on_phase: None,
            before_push: None,
            stats_limit: Duration::from_secs(120),
            after_fold: None,
        }
    }
}

/// What one pass did.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Pass {
    /// What the gate said at the start.
    pub gate: String,
    /// `(branch, verdict)` for each review written.
    pub reviewed: Vec<(String, String)>,
    /// `(branch, merge)` for each join.
    pub joined: Vec<(String, String)>,
    /// `(branch, phase)` for each stop.
    pub stopped: Vec<(String, String)>,
}

/// A green branch, as its `self.branch.built` row says it.
#[derive(Debug, Clone)]
struct Built {
    item: String,
    branch: String,
    head: String,
    report: Option<String>,
    cost_usd: f64,
    why: String,
    session: Option<String>,
    at_ms: u64,
    /// This head is a fix's, made at review and approved (issue 36): what the fresh review is
    /// told of it. A second `fix` at it is a reject.
    fixed: Option<String>,
}

/// An `accept` row with no `asked` is from before the review recorded the model that answered: it
/// was written from the setting while the turn could run elsewhere (the three reviews of 2026-11-14
/// and 15 said Fable and ran on Opus), so it is no proof its head was reviewed by another model than
/// the builder's. Such a row is not seen, and its head is reviewed again under the rule that checks
/// the model that answered. A row that is not an accept is as it was: a reject or a refusal keeps
/// its head from being asked again, and it never joined anything. Every accept written since says
/// what it was asked, because it can only be written for a review that had a target.
pub(crate) fn unproven_accept(verdict: &str, asked: &str) -> bool {
    verdict == "accept" && asked.is_empty()
}

/// A verdict written before: its branch, head, the model that answered, the one asked, and verdict.
struct Prior {
    branch: String,
    head: String,
    model: String,
    asked: String,
    verdict: String,
    /// What it said: for a `fix`, what to repair.
    reasons: Vec<String>,
}

/// What the ledger holds of the joins so far.
#[derive(Default)]
struct Seen {
    built: Vec<Built>,
    verdicts: Vec<Prior>,
    joined: Vec<String>,
    /// `(branch, head)` held at the keel.
    keel: Vec<(String, String)>,
    /// `(branch, head)` the owner's retry lifted a hold from (`self.join.reopened`).
    reopened: Vec<(String, String)>,
    /// Each head's stops since its last verdict or retry, oldest first (`unreviewable`).
    stops: Vec<(String, String, unreviewable::Stop)>,
}

/// The rows of `kinds` oldest first; none while the index's shape is built.
pub(crate) fn rows_of(
    store: &crate::store::Store,
    names: &[&str],
) -> Result<Option<Vec<LedgerRow>>> {
    let tags: Vec<String> = names.iter().map(|n| ledger_kind(n)).collect();
    let mut out = Vec::new();
    let mut before = None;
    loop {
        let page = Page {
            kind: kinds::LEDGER,
            tags: tags.clone(),
            after: None,
            before,
            since_ms: None,
            until_ms: None,
            limit: 256,
        };
        let Some(got) = store.ledger_page(&page)? else {
            return Ok(None);
        };
        for r in got.records.iter().rev() {
            if let Ok(row) = r.decode::<LedgerRow>() {
                out.push(row);
            }
        }
        match (got.more, got.first) {
            (true, Some(first)) => before = Some(first),
            _ => break,
        }
    }
    out.reverse();
    Ok(Some(out))
}

fn s(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_string()
}

fn seen(store: &crate::store::Store) -> Result<Option<Seen>> {
    let Some(rows) = rows_of(
        store,
        &[
            "self.branch.built",
            "self.review.verdict",
            "self.joined",
            "self.join.stopped",
            "self.join.reopened",
            "self.fix.verdict",
        ],
    )?
    else {
        return Ok(None);
    };
    let mut out = Seen::default();
    for r in rows {
        let d = &r.data;
        let head = (s(d, "branch"), s(d, "head"));
        match r.kind.as_str() {
            "self.join.stopped" => out.stops.push((
                head.0.clone(),
                head.1.clone(),
                unreviewable::stop_of(&s(d, "phase"), &d["numbers"], r.at_unix_ms),
            )),
            "self.review.verdict" | "self.join.reopened" => {
                out.stops.retain(|(b, h, _)| (b, h) != (&head.0, &head.1));
            }
            _ => {}
        }
        match r.kind.as_str() {
            "self.branch.built" if d["gate"] == "green" && d["head"].is_string() => {
                out.built.push(Built {
                    item: s(d, "item"),
                    branch: s(d, "branch"),
                    head: s(d, "head"),
                    report: d["report"].as_str().map(str::to_string),
                    cost_usd: d["cost_usd"].as_f64().unwrap_or(0.0),
                    why: s(d, "why"),
                    session: r.session_id.clone(),
                    at_ms: r.at_unix_ms,
                    fixed: None,
                });
            }
            // A fix made at review and accepted by the builder's model is the branch's new
            // head: its proof step and a fresh review come next (issue 36).
            "self.fix.verdict" if fix::accepted_at_review(d) => {
                if let Some(b) = out.built.iter().rev().find(|b| b.branch == s(d, "branch")) {
                    let mut b = b.clone();
                    b.head = s(d, "head");
                    b.at_ms = r.at_unix_ms;
                    b.fixed = Some(fix::told(d));
                    out.built.push(b);
                }
            }
            "self.review.verdict" => out.verdicts.push(Prior {
                branch: s(d, "branch"),
                head: s(d, "head"),
                model: s(d, "model"),
                asked: s(d, "asked"),
                verdict: s(d, "verdict"),
                reasons: d["reasons"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect(),
            }),
            "self.joined" => out.joined.push(s(d, "branch")),
            // A head already on main, closed as joined by hand on the owner's retry
            // (`joining`): done, so it is not joined, reviewed or held again.
            "self.join.stopped" if d["numbers"]["by_hand"] == true => {
                out.joined.push(s(d, "branch"));
            }
            // A head held at the keel, with no key or red on its merge waits
            // for the owner: never tried again. A conflict with main is held
            // the same (this head is never joined) but the item is built
            // again from the new main (theseus-o1h3f.6).
            "self.join.stopped" if d["numbers"]["held"] == true => {
                out.keel.push((s(d, "branch"), s(d, "head")));
            }
            // The owner's retry lifts the hold as if it had never been made: the head is a
            // branch with its review, and the pass runs every check of the join on it. A stop
            // after this row holds it again.
            "self.join.reopened" => {
                let key = (s(d, "branch"), s(d, "head"));
                out.keel.retain(|k| *k != key);
                out.reopened.push(key);
            }
            _ => {}
        }
    }
    Ok(Some(out))
}

/// Why a self step may not run now; none when it may.
pub(crate) fn refused(core: &Core) -> Option<String> {
    match super::gate(&core.self_ctx()) {
        Gate::Allowed => None,
        Gate::Off => Some("[self] mode is \"off\"".into()),
        Gate::Halted { why, .. } => Some(format!("self-improvement is halted ({why})")),
    }
}

/// The remote the join fetches and pushes: `[self] remote`, else `origin`.
pub fn remote_of(cfg: &crate::Config) -> String {
    cfg.self_improve
        .remote
        .clone()
        .unwrap_or_else(|| "origin".into())
}

/// The join lock's directory.
pub fn lock_dir(cfg: &crate::Config) -> std::path::PathBuf {
    cfg.self_improve.join_lock.as_deref().map_or_else(
        || cfg.state_dir().join("self").join("join.lock"),
        crate::config::expand,
    )
}

/// Blocking work on the blocking pool.
pub(crate) async fn blocking<T: Send + 'static>(
    f: impl FnOnce() -> Result<T> + Send + 'static,
) -> Result<T> {
    tokio::task::spawn_blocking(f).await?
}

/// A verdict, as its row says it.
struct Said {
    reviewer: String,
    /// The model that answered; for a refusal before any turn, the one it would have run on.
    model: String,
    /// The model the review was asked to run on.
    asked: String,
    verdict: &'static str,
    planted: &'static str,
    /// How the reviewer checked a build-broke proof (`proof_check`), else none.
    checked: Option<&'static str>,
    reasons: Vec<String>,
    cost_usd: f64,
}

/// One branch's join, as it goes.
struct Run<'a> {
    core: &'a Arc<Core>,
    seams: &'a Seams,
    place: Place,
    b: Built,
    /// The owner lifted a hold from this head: a head found on main is then closed as joined by
    /// hand (`joining`); nothing else about its run differs.
    reopened: bool,
    /// This head's stops since its last verdict or retry, oldest first (`unreviewable`).
    earlier: Vec<unreviewable::Stop>,
    /// What a review that answered `fix` asked to repair (`fix::at_review`).
    asked: Vec<String>,
    pass: &'a mut Pass,
}

impl Run<'_> {
    /// Before a phase: the test's hook, then the gate. A refusal writes the
    /// stop's row and says so.
    fn phase(&mut self, name: &str) -> bool {
        // The phase before this one closed what it opened (`guard`).
        guard::assert_closed(&self.place, &format!("before the {name} phase"), false);
        if let Some(f) = &self.seams.on_phase {
            f(name);
        }
        match refused(self.core) {
            None => true,
            Some(why) => {
                self.stop(name, &why, Value::Null);
                false
            }
        }
    }

    fn stop(&mut self, phase: &str, why: &str, numbers: Value) {
        self.core.rec(None).record(&SelfJoinStopped {
            branch: self.b.branch.clone(),
            head: self.b.head.clone(),
            phase: phase.into(),
            why: why.into(),
            numbers,
        });
        self.pass
            .stopped
            .push((self.b.branch.clone(), phase.into()));
    }

    /// A stop that holds the head for the owner (`numbers.held`): the next
    /// pass skips it. A halt and the lock are tried again; so are the planted
    /// revert's and the review's stops, until the same one has repeated enough
    /// times to be held (`stopped_planted`, `stopped_review`).
    fn hold(&mut self, phase: &str, why: &str, numbers: Value) {
        let mut n = match numbers {
            Value::Object(m) => Value::Object(m),
            _ => json!({}),
        };
        n["held"] = json!(true);
        self.stop(phase, why, n);
    }

    /// The owner's card, to the DM alone.
    fn card(&self, text: String) {
        let card = (|| -> Result<()> {
            let (post, frame) = self
                .core
                .outbox
                .stage_to_operator(None, json!({"kind": "self_join", "text": text}))?;
            self.core.store.append(&frame)?;
            self.core.outbox.posted(&post);
            Ok(())
        })();
        if let Err(e) = card {
            tracing::warn!(error = %format!("{e:#}"), "self-join: the owner's card was not posted");
        }
    }
}

impl Core {
    /// One pass of the self-join: the module's doc.
    pub async fn self_join_pass(self: &Arc<Self>, seams: &Seams) -> Result<Pass> {
        // One pass at a time, the join tender's and the pipeline's
        // (theseus-pw1q.16): never two reviews of one head, never two joins.
        let _one = self.rsi.pipeline.joins.lock().await;
        let mut pass = Pass::default();
        let g = super::gate(&self.self_ctx());
        pass.gate = g.name().into();
        if g != Gate::Allowed {
            return Ok(pass);
        }
        let Some(repo) = self.cfg.self_improve.repo.clone() else {
            return Ok(pass);
        };
        let store = self.store.clone();
        let Some(seen) = blocking(move || seen(&store)).await? else {
            return Ok(pass);
        };
        let place = self.tools.self_pr.place(&repo);
        let model = self
            .runner
            .resolve_target(
                &self.live_profile().0,
                self.cfg.self_improve.reviewer_profile.as_deref(),
                None,
                None,
            )
            .map(|t| t.model)
            .unwrap_or_default();
        let mut joined_one = false;
        for b in &seen.built {
            if seen.joined.contains(&b.branch)
                || seen.keel.contains(&(b.branch.clone(), b.head.clone()))
            {
                continue;
            }
            let theirs = |v: &&Prior| v.branch == b.branch && v.head == b.head;
            let mine = seen
                .verdicts
                .iter()
                .filter(theirs)
                .filter(|v| !unproven_accept(&v.verdict, &v.asked))
                .find(|v| {
                    v.model == model
                        || v.asked == model
                        || v.verdict == "accept"
                        || v.verdict == "reject"
                        || v.verdict == "fix"
                });
            let mut run = Run {
                core: self,
                seams,
                place: place.clone(),
                b: b.clone(),
                reopened: seen.reopened.contains(&(b.branch.clone(), b.head.clone())),
                earlier: seen
                    .stops
                    .iter()
                    .filter(|(sb, sh, _)| (sb, sh) == (&b.branch, &b.head))
                    .map(|(_, _, st)| st.clone())
                    .collect(),
                asked: vec![],
                pass: &mut pass,
            };
            let (verdict, asked) = match mine {
                Some(v) => (v.verdict.clone(), v.reasons.clone()),
                None => {
                    // Before its review is spent: a change main has moved under goes back to
                    // its builder (`moved`).
                    if run.moved_under(&remote_of(&self.cfg)).await? {
                        continue;
                    }
                    // A review that keeps failing for a reason that clears waits longer each time.
                    if unreviewable::waiting(&run.earlier, theseus_protocol::now_unix_ms()) {
                        continue;
                    }
                    match run.review().await? {
                        Some(v) => (v, run.asked_fix()),
                        None => break,
                    }
                }
            };
            // A review that asked for a fix: the fix runs, once (issue 36; `fix`).
            if verdict == "fix" {
                if !run.fix_at_review(&asked).await? {
                    break;
                }
                continue;
            }
            if verdict != "accept" || joined_one {
                continue;
            }
            joined_one = true;
            if !run.join(&remote_of(&self.cfg)).await? {
                break;
            }
        }
        guard::assert_closed(&place, "at the end of the pass", true);
        Ok(pass)
    }
}
