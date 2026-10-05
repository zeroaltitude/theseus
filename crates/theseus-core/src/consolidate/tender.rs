//! Consolidation's nightly run (M6 31b; M5's nightly pattern, as the
//! learning tender's): at `[memory] consolidate_hour` local time (4 by
//! default), a missed night once as soon as it may, never within 10 minutes
//! of a start, on a `learning` thread at nice 19 that sleeps 19 times as
//! long as it works (about 5% of a core). Nothing with memory off: the
//! tender is not started.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::time::Instant;

use super::run::LAST_RUN;
use crate::learning::tender::{due, tend};
use crate::rpc::Core;

impl Core {
    /// Consolidation's tender, after serving: nothing with memory off.
    pub fn consolidate_after_serving(self: &Arc<Self>) {
        if !self.runner.memory.on() {
            return;
        }
        let core = Arc::downgrade(self);
        let started = Instant::now();
        tokio::spawn(async move {
            let ran = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let (read, ran_at) = (core.clone(), ran.clone());
            let next = move || -> Option<(Duration, &'static str)> {
                let c = read.upgrade()?;
                let stored = theseus_store::blocking(|| c.store.get_meta::<Value>(LAST_RUN))
                    .ok()
                    .flatten()
                    .and_then(|v| v["at_unix_ms"].as_u64());
                let here = ran_at.load(std::sync::atomic::Ordering::Relaxed);
                let last = stored.max((here > 0).then_some(here));
                let now = theseus_protocol::now_unix_ms();
                let hour = c.runner.memory.cfg().consolidate_hour;
                let (at, trigger) = due(now, hour, last);
                Some((Duration::from_millis(at.saturating_sub(now)), trigger))
            };
            let run = move |_trigger: &'static str| {
                let core = core.clone();
                ran.store(
                    theseus_protocol::now_unix_ms(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                async move {
                    let Some(c) = core.upgrade() else { return };
                    match c.consolidate_now(false, "nightly").await {
                        Ok(r) => tracing::info!(
                            clusters = r.clusters.len(),
                            recalls = r.recalls,
                            spent_today_usd = r.spent_today_usd,
                            "consolidation: the nightly run ran"
                        ),
                        Err(e) => {
                            tracing::warn!(error = %format!("{e:#}"), "consolidation: the nightly run did not run");
                        }
                    }
                }
            };
            tend(started, next, run).await;
        });
    }
}
