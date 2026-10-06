//! The index tender's gauges (M6 §2.13; theseus-gfi4): its lag, documents
//! and RSS, and its restarts, sampled every `metrics_interval_secs` after
//! serving, only while telemetry has an endpoint and `[index]` is on. A
//! sample is health's block (`health_block`): no socket is asked before a
//! tender runs, and none past [`HEALTH_DEADLINE`](super::HEALTH_DEADLINE).
//! While the tender is down, the gauges keep its last answer and the
//! restarts move; while it is late, its last answer is sampled again.

use std::sync::{Arc, Weak};
use std::time::Duration;

use super::IndexTender;
use crate::telemetry::Telemetry;
use crate::Core;

impl IndexTender {
    /// One sample into `telemetry`. Nothing is asked while it exports
    /// nothing, or while `[index]` is off.
    pub async fn sample(&self, telemetry: &Telemetry) {
        if !telemetry.enabled() || !self.enabled() {
            return;
        }
        telemetry.record_index(&self.health_block().await);
    }
}

impl Core {
    /// The tender's sampler, after serving (theseusd's `after_serving`):
    /// one sample each `metrics_interval_secs`, the exporter's own interval,
    /// while the core lives. None without an endpoint or with `[index]` off:
    /// whether it started.
    pub fn sample_index_after_serving(self: &Arc<Self>) -> bool {
        if self.cfg.telemetry.endpoint().is_none() || !self.index.enabled() {
            return false;
        }
        let every = Duration::from_secs(self.cfg.telemetry.metrics_interval_secs.max(1));
        tokio::spawn(sample_every(Arc::downgrade(self), every));
        true
    }
}

/// Sample every `every`, holding the core only to read its tender and its
/// telemetry (a detached task holds the core by `Weak`).
async fn sample_every(core: Weak<Core>, every: Duration) {
    loop {
        tokio::time::sleep(every).await;
        let Some((index, telemetry)) = core
            .upgrade()
            .map(|c| (c.index.clone(), c.telemetry().clone()))
        else {
            return;
        };
        index.sample(&telemetry).await;
    }
}
