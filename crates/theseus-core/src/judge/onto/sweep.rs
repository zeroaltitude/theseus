//! The ontology's nightly sweep (theseus-63tfk, R5 slice one; the design's
//! section 3.2): the writes `Ontology::sweep_plan` makes, written as the
//! sweep's, each in its own frame with the rows that undo it
//! (`Core::ontology_undo`). No model is called and nothing is spent.
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
//! - **A step that does not write** (the ontology changed under it, or a
//!   record no longer checks) is warned of and left for the next night.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use theseus_ontology::{Origin, Step};
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
                    match theseus_store::blocking(|| c.onto_sweep(now)) {
                        Ok(s) => tracing::info!(
                            decayed = s.decayed,
                            merged = s.merged,
                            failed = s.failed,
                            "ontology: the sweep ran"
                        ),
                        Err(e) => {
                            tracing::warn!(error = %format!("{e:#}"), "ontology: the sweep did not run");
                        }
                    }
                }
            };
            tend(started, next, run).await;
        });
    }

    /// One run of the sweep at `now_ms`: the plan over the ontology as it
    /// stands, each step written as a machine write of the sweep's.
    pub fn onto_sweep(&self, now_ms: u64) -> anyhow::Result<Swept> {
        let o = self.runner.ontology.snapshot(&self.store)?;
        let mut out = Swept::default();
        for (n, step) in o.sweep_plan(now_ms).into_iter().enumerate() {
            let write = format!("sweep-{now_ms}-{n}-{}", step.word());
            match self.ontology_machine_write(Origin::Sweep, &write, step.records().to_vec()) {
                Ok(()) => {
                    match &step {
                        Step::Decay { dropped, .. } => out.decayed += dropped.len() as u64,
                        Step::Merge { .. } => out.merged += 1,
                    }
                    out.writes.push(write);
                }
                Err(e) => {
                    out.failed += 1;
                    tracing::warn!(write, error = %format!("{e:#}"), "ontology: a sweep step was not written");
                }
            }
        }
        Ok(out)
    }
}
