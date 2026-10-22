//! The ontology's nightly sweep (theseus-63tfk, R5 slice one; the design's
//! section 3.2): the writes `Ontology::sweep_plan` makes, written as the
//! sweep's, each in its own frame with the rows that undo it
//! (`Core::ontology_undo`). The deterministic plan calls no model and spends
//! nothing; the kinds `other` answers mint (`mint.rs`) need none either. What
//! the model judges (`judged.rs`: a pair that looks alike, a category too large
//! to say anything) runs after them, under `[ontology] sweep_usd_per_day`.
//!
//! - **When.** At quiet time once a day, `[judge] learning_hour` local, and
//!   once after a start (the learning tender's `due` and `tend`: it never
//!   begins within 10 minutes of the daemon's start). A run that finds
//!   nothing writes nothing, so a repeat is free. `[ontology] sweep = false`
//!   turns it off.
//! - **What.** Decay of a machine's memberships older than 30 days, as one
//!   write, and each merge of a duplicate category as a write of its own, so
//!   the owner undoes one without the rest. Only what a machine wrote is
//!   touched (`machine.rs`'s writer checks hold the line): the operator's and
//!   the import's categories and memberships are never dropped or moved.
//! - **A kind** minted when three sessions say `other` about one name, or
//!   about names Jev calls one kind, named by Jev's pick among them
//!   (`mint.rs`, `renamed.rs`), a pair Jev calls one thing (merged), a large
//!   category Jev splits, a stale name Jev replaces (`judged.rs`,
//!   `renamed.rs`): each a write of its own, undone the same way.
//! - **A step that does not write** (the ontology changed under it, or a
//!   record no longer checks) is warned of and left for the next night.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use theseus_ontology::{Origin, Step};

use super::mint::Hints;
use tokio::time::Instant;

use crate::learning::tender::{due, tend};
use crate::rpc::Core;

/// What a run wrote: the memberships decayed, the categories merged, the
/// steps that did not write, and the id of each write made (the id
/// `ontology undo` takes).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Swept {
    pub decayed: u64,
    pub merged: u64,
    /// The kinds minted from `other` answers, and the clusters that did not
    /// (a name no kind can take, or one the table holds).
    pub kinds: u64,
    pub refused: u64,
    /// The pairs Jev called one thing and the categories it split, each a
    /// write of the sweep's (`judged.rs`), and what its questions cost.
    pub judged_merges: u64,
    pub splits: u64,
    /// The categories Jev renamed, and the kinds it named (the rest of the
    /// kinds took the most shared name).
    pub renamed: u64,
    pub named_kinds: u64,
    pub spent: theseus_judge::price::Micros,
    pub failed: u64,
    pub writes: Vec<String>,
}

impl Core {
    /// The sweep's tender, after serving: nothing when `[ontology] sweep` is
    /// off.
    pub fn onto_sweep_after_serving(self: &Arc<Self>) {
        if !self.runner.cfg.ontology.sweep {
            return;
        }
        let core = Arc::downgrade(self);
        let started = Instant::now();
        tokio::spawn(async move {
            // No mark is stored: a run is idempotent, so the first after a
            // start is due as `missed`, and then the next hour.
            let ran = Arc::new(AtomicU64::new(0));
            let ran_at = ran.clone();
            let read = core.clone();
            let next = move || -> Option<(Duration, &'static str)> {
                let c = read.upgrade()?;
                let here = ran_at.load(Ordering::Relaxed);
                let now = theseus_protocol::now_unix_ms();
                let (at, trigger) = due(
                    now,
                    c.runner.cfg.judge.learning_hour,
                    (here > 0).then_some(here),
                );
                Some((Duration::from_millis(at.saturating_sub(now)), trigger))
            };
            let run = move |_trigger: &'static str| {
                let core = core.clone();
                ran.store(theseus_protocol::now_unix_ms(), Ordering::Relaxed);
                async move {
                    let Some(c) = core.upgrade() else { return };
                    let now = theseus_protocol::now_unix_ms();
                    match c.onto_sweep_night(now).await {
                        Ok(s) => {
                            tracing::info!(
                                decayed = s.decayed,
                                merged = s.merged,
                                kinds = s.kinds,
                                refused = s.refused,
                                judged_merges = s.judged_merges,
                                splits = s.splits,
                                renamed = s.renamed,
                                named_kinds = s.named_kinds,
                                spent = s.spent,
                                failed = s.failed,
                                "ontology: the sweep ran"
                            );
                        }
                        Err(e) => {
                            tracing::warn!(error = %format!("{e:#}"), "ontology: the sweep did not run");
                        }
                    }
                }
            };
            tend(started, next, run).await;
        });
    }

    /// A night's run: what Jev says of the kinds the `other` answers mint,
    /// then the plan with it, then the questions about the categories.
    pub(crate) async fn onto_sweep_night(&self, now_ms: u64) -> anyhow::Result<Swept> {
        let hints = self.onto_mint_hints(now_ms).await.unwrap_or_else(|e| {
            tracing::warn!(error = %format!("{e:#}"), "ontology: the kind questions did not run");
            Hints::default()
        });
        let mut s = theseus_store::blocking(|| self.onto_sweep_with(now_ms, &hints))?;
        s.spent += hints.spent;
        if let Err(e) = self.onto_sweep_judged(now_ms, &mut s).await {
            tracing::warn!(error = %format!("{e:#}"), "ontology: the sweep's questions did not run");
        }
        Ok(s)
    }

    /// One run of the sweep at `now_ms` with no model: the plan over the
    /// ontology as it stands, each step written as a machine write of the
    /// sweep's.
    pub fn onto_sweep(&self, now_ms: u64) -> anyhow::Result<Swept> {
        self.onto_sweep_with(now_ms, &Hints::default())
    }

    fn onto_sweep_with(&self, now_ms: u64, hints: &Hints) -> anyhow::Result<Swept> {
        let o = self.runner.ontology.snapshot(&self.store)?;
        let mut out = Swept::default();
        let steps = o.sweep_plan(now_ms);
        let first = steps.len();
        for (n, step) in steps.into_iter().enumerate() {
            let write = format!("sweep-{now_ms}-{n}-{}", step.word());
            match self.ontology_machine_write(Origin::Sweep, &write, step.records().to_vec()) {
                Ok(()) => {
                    match &step {
                        Step::Decay { dropped, .. } => out.decayed += dropped.len() as u64,
                        Step::Merge { .. } => out.merged += 1,
                        Step::Split { .. } | Step::Rename { .. } => {}
                    }
                    out.writes.push(write);
                }
                Err(e) => {
                    out.failed += 1;
                    tracing::warn!(write, error = %format!("{e:#}"), "ontology: a sweep step was not written");
                }
            }
        }
        self.onto_mint(now_ms, first, hints, &mut out)?;
        Ok(out)
    }
}
