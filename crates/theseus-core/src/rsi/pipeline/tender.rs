//! The pipeline's tender (theseus-pw1q.16), after serving and only with
//! `[self] mode = "act"`: with the mode off it is not started, so no task
//! and no timer exists, and the night starts its items itself. It ticks
//! [`FIRST`] after the start, then every [`EVERY`] or at once when kicked (a
//! drop, a move, a halt, a night that filed), and spawns one self-join pass
//! when a branch waits on a review or a join, never two. A tick the gate
//! refuses reads the cached switch (and, once a halt, stops the builds).

use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::rsi::SelfMode;

use super::Seams;
use crate::rpc::Core;

/// The first tick, after the start (the index's shape is built by then).
pub const FIRST: Duration = Duration::from_secs(60);
/// Between ticks, when nothing kicks.
pub const EVERY: Duration = Duration::from_secs(120);

impl Core {
    /// The self-improvement tenders after serving, in their order: the
    /// join's, the revert watch's (theseus-pw1q.10), the minted tools' tender
    /// (theseus-pw1q.21.8) and the pipeline's.
    pub fn self_after_serving(self: &Arc<Self>) {
        self.self_join_after_serving();
        self.self_revert_after_serving();
        self.mint_tender_after_serving();
        self.pipeline_after_serving();
    }

    /// The tender, after serving: nothing with the mode off.
    pub fn pipeline_after_serving(self: &Arc<Self>) {
        if self.cfg.self_improve.mode != SelfMode::Act {
            return;
        }
        self.rsi.pipeline.runs.store(true, Ordering::Relaxed);
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(FIRST).await;
            let seams = Seams::default();
            loop {
                let Some(c) = core.upgrade() else { return };
                match c.pipeline_tick(&seams).await {
                    Ok(t) => {
                        if !t.started.is_empty() || !t.said.is_empty() || !t.stopped.is_empty() {
                            tracing::info!(started = ?t.started, said = ?t.said, stopped = ?t.stopped,
                                hold = ?t.hold, "self pipeline: a tick moved");
                        }
                        if t.join_due && !c.rsi.pipeline.joining.load(Ordering::SeqCst) {
                            let (c2, s2) = (c.clone(), seams.clone());
                            tokio::spawn(async move {
                                match c2.pipeline_join(&s2).await {
                                    Ok(Some(p)) => tracing::info!(reviewed = ?p.reviewed,
                                        joined = ?p.joined, stopped = ?p.stopped,
                                        "self pipeline: a join pass ran"),
                                    Ok(None) => {}
                                    Err(e) => tracing::warn!(error = %format!("{e:#}"),
                                        "self pipeline: a join pass failed"),
                                }
                                c2.rsi.pipeline.kick();
                            });
                        }
                    }
                    Err(e) => {
                        tracing::warn!(error = %format!("{e:#}"), "self pipeline: a tick failed")
                    }
                }
                let kick = c.rsi.pipeline.kick.clone();
                drop(c);
                let kicked = kick.notified();
                tokio::select! {
                    () = tokio::time::sleep(EVERY) => {}
                    () = kicked => {}
                }
            }
        });
    }
}
