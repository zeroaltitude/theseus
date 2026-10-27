//! The join's phases of one accepted branch (theseus-pw1q.9): the keel
//! check, the lock, then each try's dry run and merge, main's gate on it,
//! the signature and the push; the install after.

use anyhow::Result;
use serde_json::{json, Value};
use theseus_protocol::tasks::{TaskEvidence, TaskState};

use super::{blocking, git, keel, lock, lock_dir, refused, Run, TRIES};
use crate::fact::rsi_join::SelfJoined;
use crate::rsi::tree::{self, GateRun, Tree, Verdict};

/// How a try ended.
enum Try {
    Joined(String),
    Moved,
    Stopped,
}

/// A merge made in its tree: the tree, the main it is on, its commit.
pub(super) struct Merged {
    pub(super) t: Tree,
    pub(super) base: String,
    pub(super) merge: String,
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
        if self.core.cfg.self_improve.install && self.phase("install") {
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
            let base = git::main_of(&p, &r)?;
            let paths: Vec<String> = git::paths(&p, &base, &h)?
                .into_iter()
                .map(|(_, p)| p)
                .collect();
            let diff = git::diff(&p, &base, &h, 0)?;
            let mut hits = keel::classify(&paths, &diff);
            let t = git::detached(&p, "keel", &b, &h)?;
            let guard = keel::keel_guard(&p, &t.dir, &base, &h);
            let _ = tree::close(&p, &t);
            hits.extend(guard?);
            let stat = p.git_ok(&p.repo, &["diff", "--stat", &format!("{base}...{h}")])?;
            Ok((hits, stat))
        })
        .await?;
        if hits.is_empty() {
            return Ok(true);
        }
        let mut classes: Vec<&str> = hits.iter().map(|h| h.class.as_str()).collect();
        classes.dedup();
        let lines: String = hits
            .iter()
            .take(20)
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

    /// Hand the install to its unit.
    async fn install(&mut self, merge: String) {
        let (p, i) = (self.place.clone(), self.seams.installer.clone());
        match blocking(move || i.start(&p, &merge)).await {
            Ok(unit) => tracing::info!(%unit, "self-join: the install is handed to its unit"),
            Err(e) => {
                let why = format!("the install did not start: {e:#}");
                self.stop("install", &why, Value::Null);
            }
        }
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
        let Some(run) = self.gated(&m.t).await else {
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
                self.core.rec(None).record(&SelfJoined {
                    item: self.b.item.clone(),
                    branch: self.b.branch.clone(),
                    head: self.b.head.clone(),
                    merge: signed.clone(),
                    gate: format!("{} (main's gate, blob {})", run.verdict.name(), run.blob),
                    cost_usd: self.b.cost_usd,
                    why: self.b.why.clone(),
                    numbers: json!({"tries": attempt, "gate_secs": run.secs, "base": m.base,
                                    "unsigned": m.merge}),
                });
                self.pass
                    .joined
                    .push((self.b.branch.clone(), signed.clone()));
                Ok(Try::Joined(signed))
            }
            Ok(git::Pushed::Moved(now)) => {
                tracing::info!(branch = %self.b.branch, main = %now, attempt, "self-join: main moved; the dry run again");
                Ok(Try::Moved)
            }
            Err(e) => {
                self.hold("push", &format!("{e:#}"), Value::Null);
                Ok(Try::Stopped)
            }
        }
    }

    /// The dry run on the remote's main, then the merge in its tree; none
    /// (held) when the head is on main already or conflicts.
    async fn merge(&mut self, remote: &str, message: &str) -> Result<Option<Merged>> {
        let (p, r, h, b, m) = (
            self.place.clone(),
            remote.to_string(),
            self.b.head.clone(),
            self.b.branch.clone(),
            message.to_string(),
        );
        let merged = blocking(move || {
            git::fetch(&p, &r)?;
            let base = git::main_of(&p, &r)?;
            let on_main = p.git(&p.repo, &["merge-base", "--is-ancestor", &h, &base])?;
            if on_main.status.success() {
                return Ok(Err(Refused::Held(
                    "its head is on main already: there is nothing to join".to_string(),
                )));
            }
            if let Err(why) = git::dry_run(&p, &base, &h)? {
                return Ok(Err(Refused::Conflict(why)));
            }
            let t = git::detached(&p, "join", &b, &base)?;
            match git::merge(&p, &t, &h, &m) {
                Ok(merge) => Ok(Ok(Merged { t, base, merge })),
                Err(e) => {
                    let _ = tree::close(&p, &t);
                    Err(e)
                }
            }
        })
        .await?;
        match merged {
            Ok(m) => Ok(Some(m)),
            Err(Refused::Held(why)) => {
                self.hold("merge", &why, Value::Null);
                Ok(None)
            }
            Err(Refused::Conflict(why)) => {
                self.conflicted(&why).await?;
                Ok(None)
            }
        }
    }

    /// The branch conflicts with main: the item goes back to `proposed` and
    /// the next build cuts a fresh branch from the new main (theseus-o1h3f.6;
    /// it used to wait for the owner and was never tried again). This head is
    /// held so no pass joins it again, but the held row says `conflict`, so
    /// the pipeline does not show it as the owner's. A third conflict of one
    /// item drops it instead, with the command back (the bound on a loop).
    async fn conflicted(&mut self, why: &str) -> Result<()> {
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
        let mut text = if dropped {
            conflict_dropped_card(&self.b.branch, &item, why)
        } else {
            conflict_card(&self.b.branch, &item, why)
        };
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
            self.core
                .store
                .append(&[crate::task_graph::record(&task)?])?;
        }
        // The next build must cut a fresh branch from the new main (design
        // item 29): close this tree, so `self_tree_open` opens a new one off
        // origin/main instead of finding this one.
        let (p, c) = (self.place.clone(), item.clone());
        let mut numbers = json!({"held": true, "conflict": true, "dropped": dropped});
        if let Err(e) = blocking(move || {
            if let Some(t) = tree::find(&p, &c) {
                tree::close(&p, &t)?;
            }
            Ok(())
        })
        .await
        {
            // The log keeps the error. The owner gets what it means: the next
            // build may find this stale tree and cut from its old base
            // (theseus-51sfw). The conflict's card and stop still go out.
            tracing::warn!(error = %format!("{e:#}"),
                "self-join: the conflicted tree was not closed");
            text.push('\n');
            text.push_str(&tree_not_closed_line(&item));
            numbers["tree_closed"] = json!(false);
        }
        self.card(text);
        self.stop("merge", why, numbers);
        Ok(())
    }

    /// Main's gate on the merge, with the benches: its run when green.
    async fn gated(&mut self, t: &Tree) -> Option<GateRun> {
        if !self.phase("gate") {
            return None;
        }
        let r = match tree::gate_with_benches(&self.place, t).await {
            Ok(r) if r.verdict == Verdict::Green => return Some(r),
            Ok(r) => r,
            Err(e) => {
                let why = format!("main's gate on the merge did not run: {e:#}");
                self.stop("gate", &why, Value::Null);
                return None;
            }
        };
        let keel = r.verdict == Verdict::Keel;
        if keel {
            self.card(merge_keel_card(
                &self.b.branch,
                &self.b.item,
                &r.log.display().to_string(),
            ));
        }
        let why = format!(
            "main's gate on the merge is {} (exit {:?}); main is untouched",
            r.verdict.name(),
            r.exit
        );
        let phase = if keel { "keel" } else { "gate" };
        self.hold(phase, &why, json!({"log": r.log, "secs": r.secs}));
        None
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

/// The owner's card for a reviewed branch that touches the hard keel.
pub(crate) fn keel_card(
    branch: &str,
    item: &str,
    classes: &str,
    lines: &str,
    stat: &str,
) -> String {
    let body = format!(
        "{lines}The change:\n```\n{stat}\n```\nJoin it by hand when you agree, or drop it; Theseus never \
         joins it on its own."
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
        &format!("Its gate log: {log}."),
    )
}

/// The owner's card for a join with no signing key.
pub(crate) fn unsigned_card(branch: &str, item: &str) -> String {
    crate::owner_note::note(
        crate::owner_note::Kind::Action,
        &format!("Set a signing key to join self branch {branch}"),
        &format!("it passed its review and main's gate ({item}), but [self] repo names no user.signingkey"),
        "Without one, an unsigned merge never reaches main. Set one for the repo, or join it by hand.",
    )
}

/// The evidence identity of a join that conflicted with main.
const CONFLICT: &str = "conflict:";

/// Conflicts one item may have before it is dropped, not rebuilt again.
const MAX_CONFLICTS: usize = 3;

/// Why the merge did not go ahead.
enum Refused {
    /// The head is held for the owner (it is on main already).
    Held(String),
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
