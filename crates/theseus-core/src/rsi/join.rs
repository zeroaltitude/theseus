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
//!    that head is never tried again.
//! 3. **The join**, one a pass, under the lock ([`lock`]): fetch, the dry
//!    run on the remote's main (`merge-tree`), a plain `--no-ff` merge in a
//!    tree, main's gate on it through the door with the benches, the
//!    signature (no `user.signingkey` in `[self] repo`: the owner's card, and
//!    nothing is pushed), the push of main, never forced; main moved before
//!    the push redoes the dry run, the merge and the gate, at most
//!    [`TRIES`] times. A dry run that conflicts with main returns the item to
//!    the backlog with a notice (its tree closed, so the next build cuts a
//!    fresh branch from the new main) and drops it at the third conflict. The
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

pub mod git;
pub mod install;
pub(crate) mod joining;
pub mod keel;
pub mod lock;
pub mod planted;
pub mod review;
mod reviewing;
mod sealing;
mod tender;
#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tests_notify;
#[cfg(test)]
mod tests_sealed;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_store::pages::ledger_kind;
use theseus_store::{kinds, Page};

use self::planted::Tests;
use self::review::Reviewer;
use super::tree::Place;
use super::Gate;
use crate::fact::rsi_join::SelfJoinStopped;
use crate::ledger::LedgerRow;
use crate::rpc::Core;

/// How many times a join redoes its dry run, merge and gate when main moves.
pub const TRIES: u32 = 3;

/// A test's hook, called with a phase's name.
pub type PhaseHook = Arc<dyn Fn(&str) + Send + Sync>;

/// What the pass reaches outside the core: the reviewer, the test runner,
/// the lock's pace, the install, and a test's hooks.
#[derive(Clone)]
pub struct Seams {
    pub reviewer: Arc<dyn Reviewer>,
    pub tests: Arc<dyn Tests>,
    pub installer: Arc<dyn install::Installer>,
    /// Scores a build on the sealed sets (theseus-pw1q.21.7).
    pub sealed: Arc<dyn super::sealed::hold::Scorer>,
    pub lock_wait: Duration,
    pub lock_every: Duration,
    /// Called with each phase's name before its gate is asked.
    pub on_phase: Option<PhaseHook>,
    /// Called with the try's number before each push of main.
    pub before_push: Option<Arc<dyn Fn(u32) + Send + Sync>>,
}

impl Default for Seams {
    fn default() -> Self {
        Self {
            reviewer: Arc::new(review::CheckTurn),
            tests: Arc::new(planted::Cargo),
            installer: Arc::new(install::Systemd),
            sealed: Arc::new(super::sealed::hold::Scratch),
            lock_wait: Duration::from_secs(2 * 3600),
            lock_every: Duration::from_secs(30),
            on_phase: None,
            before_push: None,
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
}

/// A verdict written before: its branch, head, reviewer model and verdict.
struct Prior {
    branch: String,
    head: String,
    model: String,
    verdict: String,
}

/// What the ledger holds of the joins so far.
#[derive(Default)]
struct Seen {
    built: Vec<Built>,
    verdicts: Vec<Prior>,
    joined: Vec<String>,
    /// `(branch, head)` held at the keel.
    keel: Vec<(String, String)>,
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
        ],
    )?
    else {
        return Ok(None);
    };
    let mut out = Seen::default();
    for r in rows {
        let d = &r.data;
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
                });
            }
            "self.review.verdict" => out.verdicts.push(Prior {
                branch: s(d, "branch"),
                head: s(d, "head"),
                model: s(d, "model"),
                verdict: s(d, "verdict"),
            }),
            "self.joined" => out.joined.push(s(d, "branch")),
            // A head held at the keel, with no key or red on its merge waits
            // for the owner: never tried again. A conflict with main is held
            // the same (this head is never joined) but the item is built
            // again from the new main (theseus-o1h3f.6).
            "self.join.stopped" if d["numbers"]["held"] == true => {
                out.keel.push((s(d, "branch"), s(d, "head")));
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
    model: String,
    verdict: &'static str,
    planted: &'static str,
    reasons: Vec<String>,
    cost_usd: f64,
}

/// One branch's join, as it goes.
struct Run<'a> {
    core: &'a Arc<Core>,
    seams: &'a Seams,
    place: Place,
    b: Built,
    pass: &'a mut Pass,
}

impl Run<'_> {
    /// Before a phase: the test's hook, then the gate. A refusal writes the
    /// stop's row and says so.
    fn phase(&mut self, name: &str) -> bool {
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
    /// pass skips it. A halt, the lock, the planted revert and the review's
    /// stops are tried again.
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
                .find(|v| v.model == model || v.verdict == "accept" || v.verdict == "reject");
            let mut run = Run {
                core: self,
                seams,
                place: place.clone(),
                b: b.clone(),
                pass: &mut pass,
            };
            let verdict = match mine {
                Some(v) => v.verdict.clone(),
                None => match run.review().await? {
                    Some(v) => v,
                    None => break,
                },
            };
            if verdict != "accept" || joined_one {
                continue;
            }
            joined_one = true;
            if !run.join(&remote_of(&self.cfg)).await? {
                break;
            }
        }
        Ok(pass)
    }
}
