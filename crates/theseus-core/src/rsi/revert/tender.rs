//! The watch's tender (theseus-pw1q.10), after serving and only with
//! `[self] mode = "act"`: with the mode off it is not started, so no task and
//! no timer exists. A pass [`FIRST`] after the start and every [`EVERY`] after;
//! a pass the gate refuses (a halt) reads the cached switch and nothing else,
//! and one with no join in the window reads the ledger's few `self.*` rows.

use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::rsi::SelfMode;

use super::known::Known;
use super::Seams;
use crate::rpc::Core;

/// The first pass, after the start.
pub const FIRST: Duration = Duration::from_secs(20 * 60);
/// Between passes.
pub const EVERY: Duration = Duration::from_secs(10 * 60);

impl Core {
    /// The tender, after serving: nothing with the mode off.
    pub fn self_revert_after_serving(self: &Arc<Self>) {
        if self.cfg.self_improve.mode != SelfMode::Act {
            return;
        }
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep(FIRST).await;
            let seams = Seams::default();
            let mut known = Known::new();
            loop {
                let Some(c) = core.upgrade() else { return };
                match c.self_revert_pass(&seams, &mut known).await {
                    Ok(p)
                        if !p.reverted.is_empty()
                            || !p.stopped.is_empty()
                            || !p.left.is_empty() =>
                    {
                        tracing::info!(reverted = ?p.reverted, stopped = ?p.stopped, left = ?p.left,
                            "self-revert: a pass ran");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        tracing::warn!(error = %format!("{e:#}"), "self-revert: a pass failed")
                    }
                }
                drop(c);
                tokio::time::sleep(EVERY).await;
            }
        });
    }
}
