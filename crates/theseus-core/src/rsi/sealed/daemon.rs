//! The daemon's two calls on the sealed sets, and health's view of them.

use std::sync::Arc;

use anyhow::{bail, Result};
use theseus_protocol::rsi::SealedState;

use theseus_kernel::job::L1;

use super::relay::Relay;
use super::runner::{Candidate, Launch, ScratchRunner};
use super::{ItemId, Scores, Sealed};
use crate::fact::rsi_sealed::{SelfExamCounted, SelfSealedScored};
use crate::rpc::Core;
use crate::rsi::Gate;

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

    /// Health's counts: present when a sealed dir is set.
    pub fn sealed_state(&self) -> Option<SealedState> {
        self.sealed
            .as_ref()
            .map(|s| SealedState { sets: s.counts() })
    }
}
