//! The join's phases of one accepted branch (theseus-pw1q.9): the keel
//! check, the lock, then each try's dry run and merge, main's gate on it,
//! the signature and the push; the install after.

use std::sync::PoisonError;

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::tasks::{TaskEvidence, TaskState};

use super::install::Started;
use super::{blocking, dependencies, git, keel, lock, lock_dir, moved, refused, stats, Run, TRIES};
use crate::fact::rsi_join::{SelfJoinAgain, SelfJoined};
use crate::rsi::rerun;
use crate::rsi::tree::{self, GateRun, Tree, Verdict};

/// How a try ended.
enum Try {
    Joined(String),
    Moved,
    Stopped,
}

/// A merge made in its tree: the tree, the main it is on, its commit (with the count folded in, issue 45),
/// the commit before the count, and what the count did.
pub(super) struct Merged {
    pub(super) t: Tree,
    pub(super) base: String,
    pub(super) merge: String,
    pub(super) uncounted: String,
    pub(super) stats: String,
    pub(super) lines: Option<String>,
    /// Why the count was skipped, in the code's own words (`stats::Skip`), when it was.
    pub(super) skip: Option<stats::Skip>,
}

impl Run<'_> {
    /// The keel check and the join. False when it stopped.
    pub(super) async fn join(&mut self, remote: &str) -> Result<bool> {
        if !self.phase("keel") || !self.keel_clear(remote).await? || !self.phase("lock") {
            return Ok(false);
        }
        let core = self.core.clone();
        let holder = format!(
            "theseusd self-join {} (pid {})",
            self.b.branch,
            std::process::id()
        );
        let held = lock::take(
            &lock_dir(&core.cfg),
            &holder,
            self.seams.lock_wait,
            self.seams.lock_every,
            || refused(&core),
        )
        .await?;
        let held = match held {
            Ok(h) => h,
            Err(why) => {
                self.stop("lock", &why, Value::Null);
                return Ok(false);
            }
        };
        let done = self.joined(remote).await;
        let said = match &done {
            Ok(Some(m)) => format!("joined {} as {m}", self.b.branch),
            Ok(None) => format!("stopped {}", self.b.branch),
            Err(e) => format!("failed {}: {e:#}", self.b.branch),
        };
        if let Err(e) = held.release(&said) {
            tracing::warn!(error = %format!("{e:#}"), "self-join: the join lock was not released");
        }
        let Some(merge) = done? else {
            return Ok(false);
        };
        // A root-owned layout is the helper's (issue 22): a card for `"ask"`, none for `"auto"`. Any other layout
        // installs only on the owner's standing word, as before.
        let standing = self.core.cfg.self_improve.install.standing_ok();
        let root = self.core.root_install_applies().await;
        if !((root || standing) && self.phase("install")) {
            return Ok(true);
        }
        // Another install under way, or not known yet whether one is (issue 41): this one does not start.
        if let Some(why) = self.core.install_blocked() {
            self.stop("install", &why, Value::Null);
            return Ok(true);
        }
        if root {
            let (core, place) = (self.core.clone(), self.place.clone());
            let (branch, item) = (self.b.branch.clone(), self.b.item.clone());
            if !core
                .root_install_after_join(&place, &merge, &branch, &item)
                .await
                && standing
            {
                // No helper: the loop's own install, which says the one step (issue 20).
                self.install(merge).await;
            }
        } else {
            self.install(merge).await;
        }
        Ok(true)
    }

    /// The change's keel classes: true when there are none; else the
    /// owner's card and a held stop.
    async fn keel_clear(&mut self, remote: &str) -> Result<bool> {
        let (p, r, h, b) = (
            self.place.clone(),
            remote.to_string(),
            self.b.head.clone(),
            self.b.branch.clone(),
        );
        let (hits, stat) = blocking(move || {
            git::fetch(&p, &r)?;
            let main = git::main_of(&p, &r)?;
            // Every reading here is of what the branch itself changed, from where it left
            // main: the paths and the diff by git's own three dots, the guard by that commit.
            let from = git::merge_base(&p, &main, &h)?;
            let paths: Vec<String> = git::paths(&p, &main, &h)?
                .into_iter()
                .map(|(_, p)| p)
                .collect();
            let diff = git::diff(&p, &main, &h, 0)?;
            let mut hits = keel::classify(&paths, &diff);
            hits.extend(dependencies::hits(&p, &from, &h)?);
            let t = git::detached(&p, "keel", &b, &h)?;
            let guard = keel::keel_guard(&p, &t.dir, &main, &from, &h);
            let _ = tree::close(&p, &t);
            hits.extend(guard?);
            let stat = p.git_ok(&p.repo, &["diff", "--stat", &format!("{main}...{h}")])?;
            Ok((hits, stat))
        })
        .await?;
        if hits.is_empty() {
            return Ok(true);
        }
        let mut classes: Vec<&str> = hits.iter().map(|h| h.class.as_str()).collect();
        classes.dedup();
        // Each package a change of dependencies names is a line of the card: the owner reads
        // all of them, however many, where the rest of the hits are the first twenty.
        let (deps, others): (Vec<&keel::Hit>, Vec<&keel::Hit>) =
            hits.iter().partition(|h| h.class == keel::DEPENDENCY);
        let lines: String = others
            .into_iter()
            .take(20)
            .chain(deps)
            .map(|h| format!("- {}: {}\n", h.class, h.at))
            .collect();
        self.card(keel_card(
            &self.b.branch,
            &self.b.item,
            &classes.join(", "),
            &lines,
            &stat.chars().take(1_500).collect::<String>(),
        ));
        let why = format!(
            "it touches the keel ({}), so it waits on the owner's card",
            classes.join(", ")
        );
        self.hold(
            "keel",
            &why,
            json!({"classes": classes, "hits": hits.len()}),
        );
        Ok(false)
    }

    /// Hand the install to its unit, or, where the service runs a build the loop cannot replace,
    /// tell the owner the change is joined and not installed.
    async fn install(&mut self, merge: String) {
        let (p, i, m) = (
            self.place.clone(),
            self.seams.installer.clone(),
            merge.clone(),
        );
        let started = self.start_install(&merge, move || i.start(&p, &m)).await;
        match started {
            Ok(Started::Unit(unit)) => {
                // Its unit copies the build in and restarts the daemon: still under way until then.
                tracing::info!(%unit, "self-join: the install is handed to its unit");
            }
            Ok(Started::NotInstalled(n)) => {
                let told =
                    self.core
                        .self_install_not_installed(&self.b.branch, &self.b.item, &merge, &n);
                match told {
                    Ok(_) => {
                        tracing::info!(%merge, why = %n.why, "self-join: joined, not installed")
                    }
                    Err(e) => tracing::warn!(error = %format!("{e:#}"),
                        "self-join: the not-installed notice was not written"),
                }
            }
            Err(e) => {
                let why = format!("the install did not start: {e:#}");
                self.stop("install", &why, Value::Null);
            }
        }
    }

    /// The install's start, under way in the pipeline's view from here (issue 41: a quiet loop is not
    /// this) until it ends with no restart to come; once its unit is accepted, the state names it and
    /// follows it (`pipeline::installing`).
    async fn start_install(
        &self,
        merge: &str,
        start: impl FnOnce() -> anyhow::Result<Started> + Send + 'static,
    ) -> anyhow::Result<Started> {
        let under_way = crate::rsi::pipeline::Installing {
            item: self.b.item.clone(),
            title: self.b.why.lines().next().unwrap_or("").trim().to_string(),
            branch: self.b.branch.clone(),
            merge: merge.to_string(),
            since_ms: theseus_protocol::now_unix_ms(),
            unit: None,
            root_helper: false,
        };
        let installing = &self.core.rsi.pipeline.installing;
        *installing.lock().unwrap_or_else(PoisonError::into_inner) = Some(under_way);
        let started = blocking(start).await;
        match &started {
            // From here systemd says whether it still runs (`pipeline::installing`).
            Ok(Started::Unit(unit)) => self.core.install_handed_to(unit),
            _ => *installing.lock().unwrap_or_else(PoisonError::into_inner) = None,
        }
        started
    }

    /// Under the lock: the tries. The merge when it reached main.
    async fn joined(&mut self, remote: &str) -> Result<Option<String>> {
        let message = format!(
            "Merge {}: {} ({})\n\nA self-join (theseus-pw1q.9): reviewed by another model, its planted \
             revert failed, main's gate passed on this merge.",
            self.b.branch,
            self.b.why.lines().next().unwrap_or("").trim(),
            self.b.item
        );
        for attempt in 1..=TRIES {
            match self.attempt(remote, &message, attempt).await? {
                Try::Joined(m) => return Ok(Some(m)),
                Try::Stopped => return Ok(None),
                Try::Moved => {}
            }
        }
        let why = format!("main moved before each of {TRIES} pushes");
        self.hold("push", &why, Value::Null);
        Ok(None)
    }

    /// One try: the dry run and the merge, the gate, the signature, the push.
    async fn attempt(&mut self, remote: &str, message: &str, attempt: u32) -> Result<Try> {
        if !self.phase("merge") {
            return Ok(Try::Stopped);
        }
        let Some(m) = self.merge(remote, message).await? else {
            return Ok(Try::Stopped);
        };
        let close = |r: &Self| {
            let _ = tree::close(&r.place, &m.t);
        };
        let Some((run, reran)) = self.gated(&m.t, &m.base).await else {
            close(self);
            return Ok(Try::Stopped);
        };
        if !self.sealed_clear(&m).await {
            close(self);
            return Ok(Try::Stopped);
        }
        let Some(signed) = self.signed(&m.t).await else {
            close(self);
            return Ok(Try::Stopped);
        };
        if !self.phase("push") {
            close(self);
            return Ok(Try::Stopped);
        }
        if let Some(f) = &self.seams.before_push {
            f(attempt);
        }
        let (p, t, r, base, s) = (
            self.place.clone(),
            m.t.clone(),
            remote.to_string(),
            m.base.clone(),
            signed.clone(),
        );
        let pushed = blocking(move || git::push_main(&p, &t, &r, &base, &s)).await;
        close(self);
        match pushed {
            Ok(git::Pushed::Done) => {
                let mut numbers = json!({"tries": attempt, "gate_secs": run.secs, "base": m.base,
                                         "unsigned": m.merge, "reran": reran, "stats": m.stats});
                if m.uncounted != m.merge {
                    numbers["uncounted"] = json!(m.uncounted);
                }
                if let Some(lines) = &m.lines {
                    numbers["lines"] = json!(lines);
                }
                if let Some(skip) = m.skip {
                    numbers["stats_skip"] = json!(skip.key());
                }
                self.core.rec(None).record(&SelfJoined {
                    item: self.b.item.clone(),
                    branch: self.b.branch.clone(),
                    head: self.b.head.clone(),
                    merge: signed.clone(),
                    gate: gate_said(&run, &reran),
                    cost_usd: self.b.cost_usd,
                    why: self.b.why.clone(),
                    numbers,
                });
                self.pass
                    .joined
                    .push((self.b.branch.clone(), signed.clone()));
                Ok(Try::Joined(signed))
            }
            Ok(git::Pushed::Moved(now)) => {
                tracing::info!(branch = %self.b.branch, main = %now, attempt, "self-join: main moved; the dry run again");
                // Said as it happens (issue 36, round 2): a join that main keeps moving under is
                // seen, and bounded by `TRIES`.
                if attempt < TRIES {
                    self.core.rec(None).record(&SelfJoinAgain {
                        item: self.b.item.clone(),
                        branch: self.b.branch.clone(),
                        head: self.b.head.clone(),
                        runs: attempt,
                    });
                }
                Ok(Try::Moved)
            }
            Err(e) => {
                self.hold("push", &format!("{e:#}"), Value::Null);
                Ok(Try::Stopped)
            }
        }
    }

    /// The dry run on the remote's main, then the merge in its tree with the count folded in
    /// (`stats`, issue 45); none (held) when the head is on main already or conflicts.
    async fn merge(&mut self, remote: &str, message: &str) -> Result<Option<Merged>> {
        let (p, r, h, b, m, limit, after_fold) = (
            self.place.clone(),
            remote.to_string(),
            self.b.head.clone(),
            self.b.branch.clone(),
            message.to_string(),
            self.seams.stats_limit,
            self.seams.after_fold.clone(),
        );
        let merged = blocking(move || {
            git::fetch(&p, &r)?;
            let base = git::main_of(&p, &r)?;
            let on_main = p.git(&p.repo, &["merge-base", "--is-ancestor", &h, &base])?;
            if on_main.status.success() {
                return Ok(Err(Refused::OnMain));
            }
            if let Err(why) = git::dry_run(&p, &base, &h)? {
                return Ok(Err(Refused::Conflict(why)));
            }
            let t = git::detached(&p, "join", &b, &base)?;
            let made = git::merge(&p, &t, &h, &m).and_then(|made| {
                let c = stats::count(&p, &t, &base, &made, limit, after_fold.as_deref())?;
                Ok((c, made))
            });
            match made {
                Ok((c, uncounted)) => Ok(Ok(Merged {
                    t,
                    base,
                    merge: c.merge,
                    uncounted,
                    stats: c.said,
                    lines: c.lines,
                    skip: c.skip,
                })),
                Err(e) => {
                    let _ = tree::close(&p, &t);
                    Err(e)
                }
            }
        })
        .await?;
        match merged {
            Ok(m) => Ok(Some(m)),
            // The owner's retry found the head on main: he joined it by hand, so it is done.
            Err(Refused::OnMain) if self.reopened => {
                self.joined_by_hand()?;
                Ok(None)
            }
            Err(Refused::OnMain) => {
                let why = "its head is on main already: there is nothing to join";
                self.hold("merge", why, Value::Null);
                Ok(None)
            }
            Err(Refused::Conflict(why)) => {
                self.conflicted(remote, &why).await?;
                Ok(None)
            }
        }
    }

    /// The owner's retry found the head on main, which he put there by hand: the item is done,
    /// as a join would have left it, and its cause is not forbidden. Only an item still in
    /// progress is closed; one the owner dropped meanwhile stays as he left it. The stop's row
    /// says `by_hand`, which the queues and the pass read as joined, and is no hold.
    fn joined_by_hand(&mut self) -> Result<()> {
        let identity = format!(
            "joined-by-hand:{}@{}",
            self.b.branch,
            &self.b.head[..self.b.head.len().min(12)]
        );
        let _one = self
            .core
            .rsi
            .backlog
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(mut task) = crate::task_graph::get(&self.core.store, &self.b.item)? {
            if task.state == TaskState::InProgress {
                let now = theseus_protocol::now_unix_ms();
                task.state = TaskState::Done;
                task.version += 1;
                task.updated_at_ms = now;
                task.evidence.push(TaskEvidence {
                    node: None,
                    identity,
                    note: Some("its head is on main: the owner joined it by hand".into()),
                    by: "self-join".into(),
                    at_ms: now,
                });
                self.core
                    .store
                    .append(&[crate::task_graph::record(&task)?])?;
            }
        }
        drop(_one);
        let why = "its head is on main already, joined by hand, so it is closed as done";
        self.stop("merge", why, json!({"by_hand": true}));
        Ok(())
    }

    /// The branch conflicts with main: the item goes back to `proposed`, and
    /// its builder gets the change back with main merged into a working copy
    /// of it, to resolve what clashes (`moved`; the owner's decision of
    /// 2026-11-15, message 1571650848512151645). Where that working copy
    /// cannot be made, the next build cuts a fresh branch from the new main
    /// (theseus-o1h3f.6; it used to wait for the owner and was never tried
    /// again). This head is held so no pass joins it again, but the held row
    /// says `conflict`, so the pipeline does not show it as the owner's. A
    /// third conflict of one item drops it instead, with the command back (the
    /// bound on a loop), and the stop's words say so.
    pub(super) async fn conflicted(&mut self, remote: &str, why: &str) -> Result<()> {
        let item = self.b.item.clone();
        let prior = crate::task_graph::get(&self.core.store, &item)?.map_or(0, |t| {
            t.evidence
                .iter()
                .rev()
                .take_while(|e| e.identity != "retried:owner")
                .filter(|e| e.identity.starts_with(CONFLICT))
                .count()
        });
        let identity = format!(
            "{CONFLICT}{}@{}",
            self.b.branch,
            &self.b.head[..self.b.head.len().min(12)]
        );
        let dropped = prior + 1 >= MAX_CONFLICTS;
        let brought = if dropped {
            None
        } else {
            self.main_into_tree(remote).await
        };
        let mut text = match (dropped, &brought) {
            (true, _) => conflict_dropped_card(&self.b.branch, &item, why),
            (false, Some(_)) => moved::moved_card(&self.b.branch, &item, why),
            (false, None) => conflict_card(&self.b.branch, &item, why),
        };
        {
            let _one = self
                .core
                .rsi
                .backlog
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if let Some(mut task) = crate::task_graph::get(&self.core.store, &item)? {
                let now = theseus_protocol::now_unix_ms();
                task.state = if dropped {
                    TaskState::Failed
                } else {
                    TaskState::Proposed
                };
                task.session = None;
                task.version += 1;
                task.updated_at_ms = now;
                task.evidence.push(TaskEvidence {
                    node: None,
                    identity,
                    note: Some(why.to_string()),
                    by: "self-join".into(),
                    at_ms: now,
                });
                task.evidence
                    .extend(brought.as_ref().map(|b| moved::main_merged(b, now)));
                self.core
                    .store
                    .append(&[crate::task_graph::record(&task)?])?;
            }
        }
        let mut numbers = json!({"held": true, "conflict": true, "dropped": dropped,
                                 "main_merged": brought.is_some()});
        if brought.is_none() {
            // The next build must cut a fresh branch from the new main (design
            // item 29): close this tree, so `self_tree_open` opens a new one off
            // origin/main instead of finding this one.
            self.close_stale_tree(&mut text, &mut numbers).await;
        }
        self.card(text);
        let said = match dropped {
            true => format!("{why}; it has conflicted with main {MAX_CONFLICTS} times, so it is failed, not built again"),
            false => why.to_string(),
        };
        self.stop("merge", &said, numbers);
        Ok(())
    }

    /// Main's gate on the merge, with the benches: its run when green, and the tests it passed
    /// only on a second look (none, nearly always).
    ///
    /// A red suite whose failures are all outside what the merge brings is given one second look
    /// (`rerun`, the build step's own code, in this tree and this view). The suite is the last
    /// check but two of main's gate, so a suite that fails stops it before `deny` and the gate's
    /// own-copy check; a second look that passes is therefore followed by main's whole gate once
    /// more, and that run is final: green, or red as any red.
    async fn gated(&mut self, t: &Tree, base: &str) -> Option<(GateRun, Vec<String>)> {
        let mut r = self.gate_ran(t).await?;
        let mut reran = Vec::new();
        if r.verdict == Verdict::Red {
            if let Some(tests) = rerun::second_look(&self.place, t, &r, Some(base)).await {
                reran = tests.iter().map(rerun::Failed::name).collect();
                tracing::info!(branch = %self.b.branch, tests = %reran.join(", "),
                    "self-join: they passed on a second look; main's whole gate once more");
                keep_first_log(&r);
                if !self.phase("gate") {
                    return None;
                }
                r = self.gate_ran(t).await?;
            }
        }
        if r.verdict == Verdict::Green {
            return Some((r, reran));
        }
        let keel = r.verdict == Verdict::Keel;
        if keel {
            self.card(merge_keel_card(
                &self.b.branch,
                &self.b.item,
                &r.log.display().to_string(),
            ));
        }
        let why = red_said(&r, &reran);
        let phase = if keel { "keel" } else { "gate" };
        self.hold(
            phase,
            &why,
            json!({"log": r.log, "secs": r.secs, "reran": reran}),
        );
        None
    }

    /// One run of main's gate on the merge. None when it did not run: the stop is recorded.
    async fn gate_ran(&mut self, t: &Tree) -> Option<GateRun> {
        if !self.phase("gate") {
            return None;
        }
        match tree::gate_with_benches(&self.place, t).await {
            Ok(tree::Gated::Ran(r)) => Some(r),
            Ok(tree::Gated::Stopped(why)) => {
                let why = format!("stopped while main's gate on the merge waited: {why}");
                self.stop("gate", &why, Value::Null);
                None
            }
            Err(e) => {
                let why = format!("main's gate on the merge did not run: {e:#}");
                self.stop("gate", &why, Value::Null);
                None
            }
        }
    }

    /// The merge signed with `[self] repo`'s key; none (the owner's card)
    /// with no key.
    async fn signed(&mut self, t: &Tree) -> Option<String> {
        if !self.phase("sign") {
            return None;
        }
        let Some(key) = git::signing_key(&self.place) else {
            self.card(unsigned_card(&self.b.branch, &self.b.item));
            let why = "no user.signingkey in [self] repo: an unsigned merge never reaches main";
            self.hold("sign", why, Value::Null);
            return None;
        };
        let (p, tt) = (self.place.clone(), t.clone());
        match blocking(move || git::sign(&p, &tt, &key)).await {
            Ok(s) => Some(s),
            Err(e) => {
                let why = format!("the merge could not be signed: {e:#}");
                self.hold("sign", &why, Value::Null);
                None
            }
        }
    }
}

/// The most failed tests a hold names; the count says the rest.
const NAMED_FAILURES: usize = 5;

/// What the row says of main's gate: its verdict, and the tests it passed only on a second look.
fn gate_said(run: &GateRun, reran: &[String]) -> String {
    match reran.is_empty() {
        true => format!("{} (main's gate, blob {})", run.verdict.name(), run.blob),
        false => format!(
            "{}, passed on a second look (main's gate, blob {}): {}",
            run.verdict.name(),
            run.blob,
            reran.join(", ")
        ),
    }
}

/// Why a join is held on a red gate, in plain words: the gate's verdict, which tests failed (at
/// most [`NAMED_FAILURES`], then their count), and, when a second look was given and the gate
/// was then red all the same, which tests had passed it.
pub(super) fn red_said(r: &GateRun, reran: &[String]) -> String {
    let exit = r.exit.map(|c| format!(" (exit {c})")).unwrap_or_default();
    let mut why = format!("main's gate on the merge is {}{exit}", r.verdict.name());
    if let Some(failed) = rerun::failed_in_suite(r) {
        let names: Vec<String> = failed
            .iter()
            .take(NAMED_FAILURES)
            .map(rerun::Failed::name)
            .collect();
        let n = failed.len();
        why.push_str(&match n > NAMED_FAILURES {
            true => format!(
                ": {n} tests failed, the first {NAMED_FAILURES}: {}",
                names.join(", ")
            ),
            false => format!(
                ": {n} test{} failed: {}",
                if n == 1 { "" } else { "s" },
                names.join(", ")
            ),
        });
    }
    if !reran.is_empty() {
        why.push_str(&format!(
            "; {} had failed before and passed alone, so the whole gate was run once more",
            reran.join(", ")
        ));
    }
    why.push_str("; main is untouched");
    why
}

/// The first run's log, kept beside the gate's own, which the next run writes over: the failure
/// that was let go is what shows the flaky test its reason.
fn keep_first_log(r: &GateRun) {
    let kept = r.log.with_extension("first.log");
    if let Err(e) = std::fs::copy(&r.log, &kept) {
        tracing::warn!(error = %e, "self-join: the first gate log was not kept");
    }
}

/// The owner's card for a reviewed branch that touches the hard keel.
pub(crate) fn keel_card(
    branch: &str,
    item: &str,
    classes: &str,
    lines: &str,
    stat: &str,
) -> String {
    let cmd = theseus_protocol::rsi::retry_command(item);
    let body = format!(
        "{lines}The change:\n```\n{stat}\n```\nJoin it by hand when you agree, or drop it; Theseus never \
         joins it on its own. If it was held by mistake, or has changed since, `{cmd}` has Theseus \
         check it again from the first check: it joins only if nothing stops it, and is held here \
         again, with a new card, if it still does."
    );
    crate::owner_note::note(
        crate::owner_note::Kind::Decision,
        &format!("Join or drop self branch {branch}"),
        &format!("its review passed ({item}), but it touches the hard keel ({classes})"),
        &body,
    )
}

/// The owner's card for a merge the keel guard stopped.
pub(crate) fn merge_keel_card(branch: &str, item: &str, log: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Decision,
        &format!("Self branch {branch} needs your yes to join"),
        &format!("main's keel guard stopped its merge ({item})"),
        &format!(
            "Its gate log: {log}. To have Theseus check it again from the first check, run `{}`.",
            theseus_protocol::rsi::retry_command(item)
        ),
    )
}

/// The owner's card for a join with no signing key.
pub(crate) fn unsigned_card(branch: &str, item: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Action,
        &format!("Set a signing key to join self branch {branch}"),
        &format!("it passed its review and main's gate ({item}), but [self] repo names no user.signingkey"),
        &format!(
            "Without one, an unsigned merge never reaches main. Set one for the repo and run `{}` \
             to have Theseus check it again, or join it by hand.",
            theseus_protocol::rsi::retry_command(item)
        ),
    )
}

/// The evidence identity of a join that conflicted with main.
const CONFLICT: &str = "conflict:";

/// Conflicts one item may have before it is dropped, not rebuilt again.
const MAX_CONFLICTS: usize = 3;

/// Why the merge did not go ahead.
enum Refused {
    /// The head is on main already: there is nothing to join.
    OnMain,
    /// The head conflicts with main: the item is built again.
    Conflict(String),
}

/// The owner's note for a branch that conflicted with main.
pub(crate) fn conflict_card(branch: &str, item: &str, why: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        &format!("Self branch {branch} conflicted with main"),
        &format!("its join found a conflict ({item})"),
        &format!(
            "{why}. The item is back on the backlog: its next build cuts a fresh branch from \
             the new main. Nothing waits for you."
        ),
    )
}

/// The owner's note for an item that conflicted with main too often.
pub(crate) fn conflict_dropped_card(branch: &str, item: &str, why: &str) -> String {
    let cmd = theseus_protocol::rsi::retry_command(item);
    crate::owner_note::note(
        crate::owner_note::Kind::Fyi,
        &format!("Self branch {branch} conflicted with main again"),
        &format!("its join found a conflict {MAX_CONFLICTS} times ({item})"),
        &format!(
            "{why}. The item was dropped, not rebuilt again; nothing waits for you. Bring it \
             back with `{cmd}`."
        ),
    )
}

/// The line a conflict's note adds when its tree could not be closed.
pub(crate) fn tree_not_closed_line(item: &str) -> String {
    format!(
        "The tree of {item} could not be closed, so its next build may reuse that stale tree \
         and cut from an old base, not the new main. Remove the tree if a build starts from \
         old code."
    )
}
