//! The join's phases of one accepted branch (theseus-pw1q.9): the keel
//! check, the lock, then each try's dry run and merge, main's gate on it,
//! the signature and the push; the install after.

use anyhow::Result;
use serde_json::{json, Value};

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
        self.card(format!(
            "Self branch {} ({}) is reviewed and waits for you: it touches the hard keel ({}).\n{lines}\
             The change:\n```\n{}\n```\nJoin it by hand when you agree, or drop it; Theseus never \
             joins it on its own.",
            self.b.branch,
            self.b.item,
            classes.join(", "),
            stat.chars().take(1_500).collect::<String>()
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
                return Ok(Err(
                    "its head is on main already: there is nothing to join".to_string()
                ));
            }
            if let Err(why) = git::dry_run(&p, &base, &h)? {
                return Ok(Err(why));
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
            Err(why) => {
                self.hold("merge", &why, Value::Null);
                Ok(None)
            }
        }
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
            self.card(format!(
                "Self branch {} ({}) stopped at the keel on its merge: main's keel guard found \
                 what needs your yes. Its gate log: {}.",
                self.b.branch,
                self.b.item,
                r.log.display()
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
            self.card(format!(
                "Self branch {} ({}) passed its review and main's gate, but [self] repo names no \
                 user.signingkey, and an unsigned merge never reaches main. Set one for the repo, or \
                 join it by hand.",
                self.b.branch, self.b.item
            ));
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
