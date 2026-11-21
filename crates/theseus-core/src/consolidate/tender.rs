//! The nightly run (M6 31b; M5's nightly pattern, as the learning
//! tender's): at `[memory] consolidate_hour` local time (4 by default), a
//! missed night once as soon as it may, never within 10 minutes of a start.
//! It runs the dream's guidance; the synthesis writer it ran first is
//! retired. Nothing with memory off: the tender is not started.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::time::Instant;

use super::LAST_RUN;
use crate::learning::tender::{due, tend};
use crate::rpc::Core;

impl Core {
    /// The nightly tender, after serving: nothing with memory off.
    pub fn dream_after_serving(self: &Arc<Self>) {
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
                    let now = theseus_protocol::now_unix_ms();
                    // The night is marked first, as the writer's run marked it,
                    // so a restart does not run it again.
                    let mark = serde_json::json!({"at_unix_ms": now});
                    if let Err(e) = theseus_store::blocking(|| c.store.put_meta(LAST_RUN, &mark)) {
                        tracing::warn!(error = %format!("{e:#}"), "dream: the night's mark was not written");
                    }
                    // The dream's guidance (theseus-olt3h).
                    match c.dream_guidance(now).await {
                        Ok(d) => tracing::info!(
                            written = d.written.len(),
                            refused = d.refused,
                            waiting = d.waiting,
                            reasons = ?d.waits,
                            spent = d.spent,
                            stopped = d.stopped.as_deref().unwrap_or(""),
                            "dream: the guidance ran"
                        ),
                        Err(e) => {
                            tracing::warn!(error = %format!("{e:#}"), "dream: the guidance did not run");
                        }
                    }
                }
            };
            tend(started, next, run).await;
        });
    }
}
