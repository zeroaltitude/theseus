//! The self-join's tender (theseus-pw1q.9), after serving and only with
//! `[self] mode = "act"`: with the mode off it is not started, so no task
//! and no timer exists. It first gives the install's notice (once the
//! ledger's index is built), then runs a pass [`FIRST`] after the start and
//! every [`EVERY`] after, one at a time. A pass the gate refuses reads the
//! cached switch and nothing else.

use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::rsi::SelfMode;

use super::Seams;
use crate::rpc::Core;

/// The first pass, after the start.
pub const FIRST: Duration = Duration::from_secs(10 * 60);
/// Between passes.
pub const EVERY: Duration = Duration::from_secs(15 * 60);
/// How often the notice asks again while the index is built, and how many
/// times.
const INDEX_EVERY: Duration = Duration::from_secs(5);
const INDEX_TRIES: u32 = 120;

impl Core {
    /// The tender, after serving: nothing with the mode off.
    pub fn self_join_after_serving(self: &Arc<Self>) {
        if self.cfg.self_improve.mode != SelfMode::Act {
            return;
        }
        let core = Arc::downgrade(self);
        tokio::spawn(async move {
            for _ in 0..INDEX_TRIES {
                let Some(c) = core.upgrade() else { return };
                match theseus_store::blocking(move || c.self_installed_after_serving()) {
                    Ok(Some(merge)) => {
                        tracing::info!(%merge, "self-join: installed, and the owner told");
                        break;
                    }
                    Ok(None) => break,
                    Err(_) => tokio::time::sleep(INDEX_EVERY).await,
                }
            }
            tokio::time::sleep(FIRST).await;
            let seams = Seams::default();
            loop {
                let Some(c) = core.upgrade() else { return };
                match c.self_join_pass(&seams).await {
                    Ok(p)
                        if !p.reviewed.is_empty()
                            || !p.joined.is_empty()
                            || !p.stopped.is_empty() =>
                    {
                        tracing::info!(reviewed = ?p.reviewed, joined = ?p.joined, stopped = ?p.stopped,
                            "self-join: a pass ran");
                    }
                    Ok(_) => {}
                    Err(e) => tracing::warn!(error = %format!("{e:#}"), "self-join: a pass failed"),
                }
                c.sealed_drift_tick(&seams).await;
                drop(c);
                tokio::time::sleep(EVERY).await;
            }
        });
    }

    /// The weekly drift check, when one is due and the gate allows it: the
    /// installed main against the sealed baseline (theseus-pw1q.21.7).
    async fn sealed_drift_tick(self: &Arc<Self>, seams: &Seams) {
        if super::refused(self).is_some() {
            return;
        }
        let now = theseus_protocol::now_unix_ms();
        let drift = async {
            let bins = super::sealing::installed_bins()?;
            self.sealed_drift(&*seams.sealed, bins, now, false).await
        };
        match drift.await {
            Ok(losses) if !losses.is_empty() => {
                tracing::info!(
                    losses = losses.len(),
                    "sealed drift: families fell below the baseline"
                );
            }
            Ok(_) => {}
            Err(e) => tracing::warn!(error = %format!("{e:#}"), "sealed drift: the check failed"),
        }
    }
}
