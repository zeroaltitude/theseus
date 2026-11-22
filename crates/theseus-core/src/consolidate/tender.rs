//! The nightly run (M6 31b; M5's nightly pattern, as the learning
//! tender's): at `[memory] consolidate_hour` local time (4 by default), a
//! missed night once as soon as it may, never within 10 minutes of a start.
//! It runs the dream's guidance; the synthesis writer it ran first is
//! retired. Nothing with memory off: the tender is not started.

use std::sync::Arc;
use std::time::Duration;

use serde_json::{json, Value};
use theseus_store::{kinds, NewRecord};
use tokio::time::Instant;

use super::LAST_RUN;
use crate::learning::tender::{due, tend};
use crate::rpc::Core;

impl Core {
    /// The night's mark ([`LAST_RUN`]), written first, as the retired writer's
    /// run marked it, so a restart does not run the night again; and, as that
    /// writer wrote it, between turns through the memory pass's writer
    /// handshake, so no frame of the night lands inside a turn.
    pub(crate) async fn mark_night(&self, now: u64) -> anyhow::Result<()> {
        let mark = NewRecord::json(kinds::META, Some(LAST_RUN), &json!({"at_unix_ms": now}))?;
        let w = self.runner.pass.writing().await;
        let written = theseus_store::blocking(|| self.store.append(&[mark]));
        drop(w);
        written.map(|_| ())
    }

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
                    if let Err(e) = c.mark_night(now).await {
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

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::Value;

    use super::LAST_RUN;
    use crate::config::MemoryMode;
    use crate::tests_recall::rig_with;

    /// The night's mark waits for a running turn, as the retired writer's
    /// frames did, and lands once the turn ends, readable as the tender reads
    /// it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_nights_mark_never_lands_inside_a_turn() {
        let r = rig_with(MemoryMode::Live, |_| {});
        let c = r.core.clone();
        let running = c.runner.pass.turns().begin().await;
        let from = c.store.last_position();
        let mark = {
            let c = c.clone();
            tokio::spawn(async move { c.mark_night(1_700_000_000_000).await })
        };
        tokio::time::sleep(Duration::from_millis(1500)).await;
        assert_eq!(
            c.store.last_position(),
            from,
            "nothing written while the turn runs"
        );
        assert!(!mark.is_finished());
        drop(running);
        mark.await.unwrap().unwrap();
        assert!(c.store.last_position() > from);
        let at = c.store.get_meta::<Value>(LAST_RUN).unwrap().unwrap()["at_unix_ms"].as_u64();
        assert_eq!(at, Some(1_700_000_000_000));
    }
}
