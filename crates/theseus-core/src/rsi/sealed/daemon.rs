//! The daemon's two calls on the sealed sets, and health's view of them.

use std::sync::Arc;

use anyhow::{bail, Result};
use theseus_protocol::rsi::SealedState;

use theseus_kernel::job::L1;

use super::hold::{judge, Kept, Scorer, Whose};
use super::relay::Relay;
use super::runner::{Candidate, Launch, ScratchRunner};
use super::{ItemId, Scores, Sealed};
use crate::fact::rsi_sealed::{SelfExamCounted, SelfSealedDrift, SelfSealedScored};
use crate::rpc::Core;
use crate::rsi::Gate;

/// META: the first score after switch-on, which the drift check holds main to.
const BASELINE: &str = "self.sealed.baseline";
/// META: when the drift check last ran (a number of milliseconds).
const DRIFT_AT: &str = "self.sealed.drift.at";
const DAY_MS: u64 = 86_400_000;

/// The cache's key: one score for each (main commit, set digest).
fn main_key(commit: &str, digest: &str) -> String {
    format!("self.sealed.main.{commit}.{digest}")
}

/// Why sealed scoring may not run now; none when it may.
fn refused(core: &Core) -> Option<String> {
    crate::rsi::join::refused(core)
}

impl Core {
    /// The sealed store, when `[self.sealed] dir` is set.
    pub fn sealed(&self) -> Option<&Arc<Sealed>> {
        self.sealed.as_ref()
    }

    /// Write an item into a sealed set, counted by `reviewer`'s independent
    /// review: its `self.exam.counted` row carries the item's opaque id and
    /// family, never its text. Returns the id and no bytes. self-exams feeds
    /// its sealed half through here.
    pub fn sealed_put(
        &self,
        set: &str,
        family: &str,
        item: &[u8],
        reviewer: &str,
    ) -> Result<ItemId> {
        let sealed = self.sealed.as_ref().ok_or_else(|| {
            anyhow::anyhow!("[self.sealed] dir is not set: there is no sealed set")
        })?;
        let id = sealed.put(set, family, item)?;
        let fact = SelfExamCounted {
            item: id.as_str(),
            family,
            sealed: true,
            reviewer,
        };
        self.store.append(&[crate::fact::row(&fact, None, None)?])?;
        self.rec(None).announce(&fact);
        Ok(id)
    }

    /// Score a candidate's build against a sealed set (theseus-pw1q.21.5): a
    /// scratch daemon of `candidate` answers each item in L1, through the model
    /// relay, and the numbers come back, with one `self.sealed.scored` row for
    /// the (set, branch, `head`). Asks `rsi::gate` first: with the mode off or
    /// the switch halted nothing is read, run or written. The runner's
    /// directory is gone when it returns. Blocking work runs on the blocking
    /// pool.
    pub async fn sealed_score_branch(
        self: &Arc<Self>,
        set: &str,
        candidate: Candidate,
        head: &str,
    ) -> Result<Scores> {
        self.sealed_score_with(set, candidate, head, |view, _| super::runner::in_l1(view))
            .await
    }

    /// [`Core::sealed_score_branch`] with the way a job runs chosen (a test's
    /// seam: the gate, the scoring, the row and the cleanup are the same).
    pub(crate) async fn sealed_score_with(
        self: &Arc<Self>,
        set: &str,
        candidate: Candidate,
        head: &str,
        launch: impl FnOnce(&L1, &Arc<Relay>) -> Box<dyn Launch> + Send + 'static,
    ) -> Result<Scores> {
        match crate::rsi::gate(&self.self_ctx()) {
            Gate::Allowed => {}
            other => bail!(
                "sealed scoring does not run while self-improvement is {}",
                other.name()
            ),
        }
        let sealed = self.sealed.clone().ok_or_else(|| {
            anyhow::anyhow!("[self.sealed] dir is not set: there is no sealed set")
        })?;
        let (core, set, head) = (self.clone(), set.to_string(), head.to_string());
        tokio::task::spawn_blocking(move || {
            let runner = ScratchRunner::with_launch(&core, &candidate, launch)?;
            let scores = sealed.score(&set, &runner)?;
            let fact = SelfSealedScored {
                branch: &candidate.branch,
                head: &head,
                scores: &scores,
                cost_usd: runner.relay().spent_usd(),
            };
            core.store.append(&[crate::fact::row(&fact, None, None)?])?;
            core.rec(None).announce(&fact);
            Ok(scores)
        })
        .await?
    }

    /// Every sealed set that holds an item, scored for `whose` on the live
    /// profile (theseus-pw1q.21.7): one `self.sealed.scored` row each, spent
    /// only through the relay on `whose.branch`'s line. Asks `rsi::gate` first.
    pub async fn sealed_score(self: &Arc<Self>, whose: &Whose) -> Result<Vec<Scores>> {
        let sealed = self.sealed.clone().ok_or_else(|| {
            anyhow::anyhow!("[self.sealed] dir is not set: there is no sealed set")
        })?;
        let mut out = Vec::new();
        for (set, n) in sealed.counts() {
            if n == 0 {
                continue;
            }
            let candidate = Candidate {
                branch: whose.branch.clone(),
                bins: whose.bins.clone(),
                profile: self.live_profile().0,
            };
            out.push(
                self.sealed_score_branch(&set, candidate, &whose.head)
                    .await?,
            );
        }
        Ok(out)
    }

    /// The sets' digest, off the runtime's workers.
    pub(crate) async fn sealed_digest(self: &Arc<Self>) -> Result<String> {
        let sealed = self.sealed.clone().ok_or_else(|| {
            anyhow::anyhow!("[self.sealed] dir is not set: there is no sealed set")
        })?;
        Ok(tokio::task::spawn_blocking(move || sealed.digest()).await??)
    }

    /// Main's score for `whose` (its `head` is main's commit): from the cache
    /// when this commit and these sets were scored, else scored and kept. The
    /// first score ever kept is the drift check's baseline. Whether it was
    /// cached comes with it. Reads and writes nothing while self-improvement
    /// is off or halted.
    pub async fn sealed_main(
        self: &Arc<Self>,
        scorer: &dyn Scorer,
        whose: &Whose,
    ) -> Result<(Vec<Scores>, bool)> {
        if let Some(why) = refused(self) {
            bail!("sealed scoring does not run: {why}");
        }
        let digest = self.sealed_digest().await?;
        let key = main_key(&whose.head, &digest);
        if let Some(kept) = self.store.get_meta::<Kept>(&key)? {
            return Ok((kept.scores, true));
        }
        let scores = scorer.score(self, whose).await?;
        let kept = Kept {
            commit: whose.head.clone(),
            digest,
            at_ms: theseus_protocol::now_unix_ms(),
            scores,
        };
        self.store.put_meta(&key, &kept)?;
        if self.store.get_meta::<Kept>(BASELINE)?.is_none() {
            self.store.put_meta(BASELINE, &kept)?;
        }
        Ok((kept.scores, false))
    }

    /// The weekly drift check (`[self.sealed] drift_every_days`): when it is
    /// due (or `force`), the installed main is scored and held to the
    /// baseline, the first score after switch-on. A family (or a set's total)
    /// that fell is one `self.sealed.drift` row, which the backlog's night
    /// files as one `sealed.drift:<family>` cause; nothing is reverted. The
    /// first score is the baseline, and so is the first after the sets
    /// changed (their digest differs). Returns the losses filed.
    pub async fn sealed_drift(
        self: &Arc<Self>,
        scorer: &dyn Scorer,
        bins: std::path::PathBuf,
        now_ms: u64,
        force: bool,
    ) -> Result<Vec<super::hold::Loss>> {
        if let Some(why) = refused(self) {
            bail!("the drift check does not run: {why}");
        }
        let every = u64::from(self.cfg.self_improve.sealed.drift_every_days) * DAY_MS;
        let last = self.store.get_meta::<u64>(DRIFT_AT)?.unwrap_or(0);
        if self.sealed.is_none() || (!force && last > 0 && now_ms < last + every) {
            return Ok(Vec::new());
        }
        let whose = Whose {
            branch: "sealed.drift".into(),
            head: "installed".into(),
            bins,
        };
        let digest = self.sealed_digest().await?;
        let scores = scorer.score(self, &whose).await?;
        self.store.put_meta(DRIFT_AT, &now_ms)?;
        let now = Kept {
            commit: whose.head,
            digest,
            at_ms: now_ms,
            scores,
        };
        let Some(base) = self
            .store
            .get_meta::<Kept>(BASELINE)?
            .filter(|b| b.digest == now.digest)
        else {
            self.store.put_meta(BASELINE, &now)?;
            return Ok(Vec::new());
        };
        let verdict = judge(&base.scores, &now.scores);
        let (mut frame, mut facts) = (Vec::new(), Vec::new());
        for loss in &verdict.holds {
            let fact = SelfSealedDrift {
                loss,
                baseline: &base.commit,
                now: &now.commit,
            };
            frame.push(crate::fact::row(&fact, None, None)?);
            facts.push(fact);
        }
        if !frame.is_empty() {
            self.store.append(&frame)?;
            let rec = self.rec(None);
            for f in &facts {
                rec.announce(f);
            }
        }
        Ok(verdict.holds)
    }

    /// Health's counts: present when a sealed dir is set.
    pub fn sealed_state(&self) -> Option<SealedState> {
        self.sealed
            .as_ref()
            .map(|s| SealedState { sets: s.counts() })
    }
}
