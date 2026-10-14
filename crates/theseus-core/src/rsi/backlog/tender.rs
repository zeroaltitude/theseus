//! The backlog's nightly tender, on the pattern consolidation uses
//! (`consolidate::tender`): `[memory] consolidate_hour` plus one hour, so it
//! reads after the learning loop (`[judge] learning_hour`, 3) and
//! consolidation (4) have written; a missed night once as soon as it may,
//! never within 10 minutes of a start. Nothing at all while `[self] mode` is
//! `"off"`: the tender is not started, so no task and no timer exists.
//!
//! The night's read and writes run on the blocking pool; the first turn of
//! each item it started runs here, after the night has written, in the item's
//! own `"self"` session.

use std::sync::Arc;
use std::time::Duration;

use theseus_protocol::rsi::SelfMode;
use tokio::time::Instant;

use super::state::State;
use super::{gate, Gate, Started};
use crate::learning::tender::{due, tend};
use crate::rpc::Core;
use crate::turn::TurnRequest;

/// The hour of the day the night runs: an hour after consolidation.
pub fn hour_of(consolidate_hour: u8) -> u8 {
    (consolidate_hour + 1) % 24
}

impl Core {
    /// The backlog's tender, after serving: nothing with the mode off.
    pub fn backlog_after_serving(self: &Arc<Self>) {
        if self.cfg.self_improve.mode != SelfMode::Act {
            return;
        }
        let core = Arc::downgrade(self);
        let started = Instant::now();
        tokio::spawn(async move {
            let ran = Arc::new(std::sync::atomic::AtomicU64::new(0));
            let (read, ran_at) = (core.clone(), ran.clone());
            let next = move || -> Option<(Duration, &'static str)> {
                let c = read.upgrade()?;
                let stored = theseus_store::blocking(|| State::read(&c.store))
                    .ok()
                    .and_then(|s| s.read_ms);
                let here = ran_at.load(std::sync::atomic::Ordering::Relaxed);
                let last = stored.max((here > 0).then_some(here));
                let now = theseus_protocol::now_unix_ms();
                let (at, trigger) = due(now, hour_of(c.cfg.memory.consolidate_hour), last);
                Some((Duration::from_millis(at.saturating_sub(now)), trigger))
            };
            let run = move |_trigger: &'static str| {
                let core = core.clone();
                // A night the gate refuses still counts as run: it asks again tomorrow.
                ran.store(
                    theseus_protocol::now_unix_ms(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                async move {
                    let Some(c) = core.upgrade() else { return };
                    let night = {
                        let c = c.clone();
                        theseus_store::blocking(move || {
                            c.backlog_night(theseus_protocol::now_unix_ms())
                        })
                    };
                    match night {
                        Ok(n) => {
                            tracing::info!(
                                gate = %n.gate, candidates = n.candidates, filed = n.filed.len(),
                                updated = n.updated.len(), trimmed = n.trimmed.len(),
                                started = n.started.len(), "self backlog: the night ran"
                            );
                            for s in n.started {
                                let c = c.clone();
                                tokio::spawn(async move { c.backlog_run(s).await });
                            }
                        }
                        Err(e) => {
                            tracing::warn!(error = %format!("{e:#}"), "self backlog: the night did not run");
                        }
                    }
                }
            };
            tend(started, next, run).await;
        });
    }

    /// An item's first turn: its objective, in its own session. Stops short
    /// if the gate no longer allows it; a failed turn is logged, and the item
    /// stays `in_progress` for the owner to see.
    pub(super) async fn backlog_run(self: &Arc<Self>, s: Started) {
        if gate(&self.self_ctx()) != Gate::Allowed {
            return;
        }
        let run = async {
            let session = self
                .store
                .get_session::<crate::session::SessionRecord>(&s.session_id)?
                .ok_or_else(|| anyhow::anyhow!("session {} is gone", s.session_id))?;
            let (live, _) = self.live_profile();
            let target = self.runner.target_for_session(&session, &live)?;
            let sink = crate::bus::EventSink::new(self.bus.clone(), &session.session_id, None);
            self.runner
                .run(TurnRequest {
                    session,
                    input: Some(s.objective.clone()),
                    target,
                    sink,
                    author: "self:backlog".into(),
                    recompile: None,
                    attachments: vec![],
                    arrived: None,
                    reply_to: None,
                    prompt: None,
                    answer_at: None,
                })
                .await
        };
        if let Err(e) = run.await {
            tracing::warn!(item = %s.item, error = %format!("{e:#}"), "self backlog: an item's first turn failed");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_night_runs_an_hour_after_consolidation_and_wraps() {
        assert_eq!(hour_of(4), 5);
        assert_eq!(hour_of(23), 0);
    }
}
