//! The nightly run (theseus-slvl.6): at `[judge] learning_hour`, at most once
//! in `[card] every_days`, until the owner says `theseus card done`. That is
//! the archive pass; the live pass (`live.rs`, theseus-6f8t0) runs every night
//! at the same hour for as long as `[card] observe` is on, done or not (it
//! keeps its own mark in memory: it has no new state, and a pass that finds
//! nothing open costs a read). The
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
    /// The tender, after serving: nothing when both passes are off.
    pub fn card_after_serving(self: &Arc<Self>) {
        let cfg = &self.cfg.card;
        if cfg.day_usd <= 0.0 || (cfg.every_days == 0 && !cfg.observe) {
            return;
        }
        let core = Arc::downgrade(self);
        let started = Instant::now();
        tokio::spawn(async move {
            // When this process last ran each pass: a run that wrote no mark (a
            // failed one) is not due again at once.
            let ran = Arc::new(AtomicU64::new(0));
            let ran_live = Arc::new(AtomicU64::new(0));
            let (read, ran_at, live_at) = (core.clone(), ran.clone(), ran_live.clone());
            let next = move || -> Option<(Duration, &'static str)> {
                let c = read.upgrade()?;
                let st = theseus_store::blocking(|| State::read(&c.store)).ok()?;
                let now = theseus_protocol::now_unix_ms();
                let hour = c.cfg.judge.learning_hour;
                let archive = (c.cfg.card.every_days > 0 && !st.done).then(|| {
                    let here = ran_at.load(Ordering::Relaxed);
                    let last = st.last_run_ms.max((here > 0).then_some(here));
                    next_at(now, hour, c.cfg.card.every_days, last)
                });
                let live = c.cfg.card.observe.then(|| {
                    let here = live_at.load(Ordering::Relaxed);
                    let (at, trigger) = due(now, hour, (here > 0).then_some(here));
                    (
                        at,
                        if trigger == "missed" {
                            "live missed"
                        } else {
                            "live"
                        },
                    )
                });
                let (at, trigger) = match (archive, live) {
                    (Some(a), Some(l)) => {
                        if l.0 < a.0 {
                            l
                        } else {
                            a
                        }
                    }
                    (Some(a), None) => a,
                    (None, Some(l)) => l,
                    // Done: look again in a day, in case the owner takes it back.
                    (None, None) => return Some((Duration::from_millis(DAY_MS), "done")),
                };
                Some((Duration::from_millis(at.saturating_sub(now)), trigger))
            };
            let run = move |trigger: &'static str| {
                let core = core.clone();
                let (ran, ran_live) = (ran.clone(), ran_live.clone());
                async move {
                    if let Some(c) = core.upgrade() {
                        c.card_tick(trigger, &ran, &ran_live).await;
                    }
                }
            };
            tend(started, next, run).await;
        });
    }

    /// One firing of the nightly tender: the archive's pass when it is due, the
    /// live pass when it is, and the one `trigger` names whatever the clock says.
    async fn card_tick(&self, trigger: &'static str, ran: &AtomicU64, ran_live: &AtomicU64) {
        if trigger == "done" {
            return;
        }
        let now = theseus_protocol::now_unix_ms();
        let hour = self.cfg.judge.learning_hour;
        let cfg = &self.cfg.card;
        let st = theseus_store::blocking(|| State::read(&self.store)).unwrap_or_default();
        let live_trigger = trigger.starts_with("live");
        let archive_on = cfg.every_days > 0 && !st.done;
        let archive_due = archive_on && {
            let here = ran.load(Ordering::Relaxed);
            let last = st.last_run_ms.max((here > 0).then_some(here));
            next_at(now, hour, cfg.every_days, last).0 <= now
        };
        let live_due = cfg.observe && {
            let here = ran_live.load(Ordering::Relaxed);
            due(now, hour, (here > 0).then_some(here)).0 <= now
        };
        if archive_due || (archive_on && !live_trigger) {
            ran.store(now, Ordering::Relaxed);
            self.archive_tick().await;
        }
        if live_due || live_trigger {
            ran_live.store(now, Ordering::Relaxed);
            self.live_tick().await;
        }
    }

    async fn archive_tick(&self) {
        match self.card_run(false).await {
            Ok(r) => tracing::info!(
                proposed = r.proposed.len(),
                spent_usd = r.spent_usd,
                today_usd = r.spent_today_usd,
                stopped = r.stopped.as_deref().unwrap_or(""),
                "card: the run finished"
            ),
            Err(e) => tracing::warn!(error = %format!("{e:#}"), "card: the run did not happen"),
        }
    }

    async fn live_tick(&self) {
        let mut out = theseus_protocol::CardProposeResult::default();
        match self.card_live_run(false, &mut out).await {
            Ok(made) if made.is_empty() => {}
            Ok(made) => tracing::info!(
                proposed = made.len(),
                spent_usd = out.spent_usd,
                "card: the live pass finished"
            ),
            Err(e) => {
                tracing::warn!(error = %format!("{e:#}"), "card: the live pass did not happen")
            }
        }
    }
}
