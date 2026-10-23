//! The nightly run (theseus-slvl.6): at `[judge] learning_hour`, at most once
//! in `[card] every_days`, until the owner says `theseus card done`. The
//! learning tender's `due` and `tend` pace it (a missed night once, never
//! within 10 minutes of a start); the run is `Core::card_run`, which reserves
//! every call on `[card] day_usd` and the day ceiling. Off with `day_usd` or
//! `every_days` at 0, and off once the card is done; `max_open` waiting is an
//! attempt like any run (it records itself, scans nothing, and the next comes
//! on the normal schedule). A run that wrote no mark (a failed one) is not due
//! again at once: the tender keeps its own mark of a start.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::time::Instant;

use super::state::State;
use crate::learning::tender::{due, tend};
use crate::rpc::Core;

const DAY_MS: u64 = theseus_judge::learn::DAY_MS;

/// When the next run is due: the learning tender's `due`, and never before
/// `every_days` after the last run.
pub fn next_at(
    now_ms: u64,
    hour: u8,
    every_days: u32,
    last_run_ms: Option<u64>,
) -> (u64, &'static str) {
    let (at, trigger) = due(now_ms, hour, last_run_ms);
    let floor = last_run_ms.map_or(0, |l| l + u64::from(every_days) * DAY_MS);
    if at >= floor {
        return (at, trigger);
    }
    // The first learning hour after the interval's end.
    let (after, _) = due(floor, hour, Some(floor));
    (after, "weekly")
}

impl Core {
    /// The tender, after serving: nothing when the job is off.
    pub fn card_after_serving(self: &Arc<Self>) {
        let cfg = &self.cfg.card;
        if cfg.day_usd <= 0.0 || cfg.every_days == 0 {
            return;
        }
        let core = Arc::downgrade(self);
        let started = Instant::now();
        tokio::spawn(async move {
            let ran = Arc::new(AtomicU64::new(0));
            let (read, ran_at) = (core.clone(), ran.clone());
            let next = move || -> Option<(Duration, &'static str)> {
                let c = read.upgrade()?;
                let st = theseus_store::blocking(|| State::read(&c.store)).ok()?;
                if st.done {
                    // Done: look again in a day, in case the owner takes it back.
                    return Some((Duration::from_millis(DAY_MS), "done"));
                }
                let here = ran_at.load(Ordering::Relaxed);
                let last = st.last_run_ms.max((here > 0).then_some(here));
                let now = theseus_protocol::now_unix_ms();
                let (at, trigger) =
                    next_at(now, c.cfg.judge.learning_hour, c.cfg.card.every_days, last);
                Some((Duration::from_millis(at.saturating_sub(now)), trigger))
            };
            let run = move |trigger: &'static str| {
                let core = core.clone();
                ran.store(theseus_protocol::now_unix_ms(), Ordering::Relaxed);
                async move {
                    let Some(c) = core.upgrade() else { return };
                    if trigger == "done" {
                        return;
                    }
                    match c.card_run(false).await {
                        Ok(r) => tracing::info!(
                            proposed = r.proposed.len(),
                            spent_usd = r.spent_usd,
                            today_usd = r.spent_today_usd,
                            stopped = r.stopped.as_deref().unwrap_or(""),
                            "card: the run finished"
                        ),
                        Err(e) => {
                            tracing::warn!(error = %format!("{e:#}"), "card: the run did not happen")
                        }
                    }
                }
            };
            tend(started, next, run).await;
        });
    }
}
