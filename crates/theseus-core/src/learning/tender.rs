//! The learning tender (M5 25c; design §2.9, §2.16 FAST): the nightly
//! report, after serving and never on the start path.
//!
//! - **When.** At `[judge] learning_hour` local time (3 by default). A night
//!   the daemon missed (no run since the last such hour) runs once, as soon
//!   as it may. Never within [`AFTER_START`] of a start: the start's
//!   aftermath stays quiet.
//! - **Where.** On a thread of its own (`learning`), at nice 19, and at
//!   about 5% of a core: after each pack's stretch of work it sleeps 19
//!   times as long, as the store's history check does. The tender's task
//!   holds the core weakly, and the thread holds it only for its run.
//! - **Nothing with the judge off**: the tender is not started, and a run
//!   is refused.

use std::sync::Arc;
use std::time::Duration;

use serde_json::Value;
use tokio::time::Instant;

use super::{local_midnight, LAST_RUN};
use crate::rpc::Core;

/// No run sooner than this after a start.
pub const AFTER_START: Duration = Duration::from_secs(10 * 60);

/// The nice value the run's thread takes.
pub const NICE: i32 = 19;

const DAY_MS: u64 = theseus_judge::learn::DAY_MS;

/// When the next run is due, from now (unix ms), the local hour, and the
/// last run's time: a missed night (no run since the latest `hour`) is due
/// now as `missed`; otherwise the next `hour`, `nightly`.
pub fn due(now_ms: u64, hour: u8, last_run_ms: Option<u64>) -> (u64, &'static str) {
    let today = local_midnight(now_ms) + u64::from(hour) * 3_600_000;
    let latest = if today <= now_ms {
        today
    } else {
        today.saturating_sub(DAY_MS)
    };
    if last_run_ms.is_none_or(|l| l < latest) {
        (now_ms, "missed")
    } else {
        (latest + DAY_MS, "nightly")
    }
}

/// The tender's loop, its parts given: `next` says how long until the next
/// run and why (none: stop), `run` runs it. Each run waits until it is due,
/// and never begins before `started + AFTER_START`, on tokio's clock.
pub async fn tend<N, R, F>(started: Instant, mut next: N, mut run: R)
where
    N: FnMut() -> Option<(Duration, &'static str)>,
    R: FnMut(&'static str) -> F,
    F: std::future::Future<Output = ()>,
{
    loop {
        let Some((wait, trigger)) = next() else {
            return;
        };
        let at = (Instant::now() + wait).max(started + AFTER_START);
        tokio::time::sleep_until(at).await;
        run(trigger).await;
    }
}

/// Run `f` on a thread of its own, named `learning`, at nice [`NICE`]; the
/// nice value it took (or the error that kept it) comes back with `f`'s
/// result.
pub fn on_low_thread<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> std::io::Result<tokio::sync::oneshot::Receiver<(i32, T)>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("learning".into())
        .spawn(move || {
            let nice = lower_this_thread();
            let _ = tx.send((nice, f()));
        })?;
    Ok(rx)
}

/// Lower this thread's priority to [`NICE`] (Linux's nice is per thread);
/// the value it has after.
fn lower_this_thread() -> i32 {
    // SAFETY: gettid and setpriority/getpriority on this thread's own id
    // read and change nothing but its scheduling priority.
    unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        if libc::setpriority(libc::PRIO_PROCESS, tid, NICE) != 0 {
            tracing::debug!(error = %std::io::Error::last_os_error(), "learning: the thread's priority was not lowered");
        }
        libc::getpriority(libc::PRIO_PROCESS, tid)
    }
}

impl Core {
    /// The learning tender, after serving (design §2.9): nothing with the
    /// judge off.
    pub fn learn_after_serving(self: &Arc<Self>) {
        if !self.cfg.judge.enabled {
            return;
        }
        let core = Arc::downgrade(self);
        let started = Instant::now();
        tokio::spawn(async move {
            // When this process last ran it: a run that found nothing to
            // report writes no mark, so the tender remembers it here.
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
                let (at, trigger) = due(now, c.cfg.judge.learning_hour, last);
                Some((Duration::from_millis(at.saturating_sub(now)), trigger))
            };
            let run = move |trigger: &'static str| {
                let core = core.clone();
                ran.store(
                    theseus_protocol::now_unix_ms(),
                    std::sync::atomic::Ordering::Relaxed,
                );
                async move {
                    let rt = tokio::runtime::Handle::current();
                    let rx = on_low_thread(move || {
                        let c = core.upgrade()?;
                        let paced = |took: Duration| std::thread::sleep(took * 19);
                        let r = c.run_learning(theseus_protocol::now_unix_ms(), trigger, paced);
                        // The ladder's rules again, as the backstop (26a).
                        c.runner.judge.ladder().recheck();
                        // Then the learning loop (25f): each lineage's
                        // proposal, after the report.
                        if r.is_ok() {
                            let t0 = std::time::Instant::now();
                            let props = c.learn_nightly(&rt, theseus_protocol::now_unix_ms());
                            paced(t0.elapsed());
                            tracing::info!(
                                proposals = props.iter().filter(|p| p.decision != "none").count(),
                                "learning: the loop ran"
                            );
                        }
                        Some(r)
                    });
                    match rx {
                        Ok(rx) => match rx.await {
                            Ok((_, Some(Err(e)))) => {
                                tracing::warn!(error = %format!("{e:#}"), "learning: the report did not run");
                            }
                            Ok((nice, Some(Ok(r)))) => tracing::info!(
                                date = %r.date, trigger, packs = r.packs.len(),
                                system_labels = r.labels.system_written, nice,
                                "learning: the report ran"
                            ),
                            _ => {}
                        },
                        Err(e) => {
                            tracing::warn!(error = %e, "learning: the report's thread did not start")
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
    use super::*;

    /// A missed night is due now; a night that ran waits for the next
    /// hour; the hour today, once passed, is the night that counts.
    #[test]
    fn a_missed_night_is_due_now_and_a_kept_one_waits() {
        let now = 1_791_000_000_000u64;
        let midnight = local_midnight(now);
        let at3 = midnight + 3 * 3_600_000;
        let latest = if at3 <= now { at3 } else { at3 - DAY_MS };
        assert_eq!(due(now, 3, None), (now, "missed"));
        assert_eq!(due(now, 3, Some(latest - 1)), (now, "missed"));
        assert_eq!(due(now, 3, Some(latest)), (latest + DAY_MS, "nightly"));
        assert_eq!(due(now, 3, Some(now)), (latest + DAY_MS, "nightly"));
    }

    /// The tender never runs within 10 minutes of its start, even when a
    /// run is due at once, and runs when it is due after that (tokio's
    /// paused clock).
    #[tokio::test(start_paused = true)]
    async fn the_tender_never_runs_within_ten_minutes_of_a_start() {
        let started = Instant::now();
        let ran = Arc::new(std::sync::Mutex::new(Vec::<(Duration, &str)>::new()));
        let mut plan = vec![
            (Duration::from_secs(3 * 3600), "nightly"),
            (Duration::ZERO, "missed"),
        ];
        let seen = ran.clone();
        let tender = tokio::spawn(tend(
            started,
            move || plan.pop(),
            move |trigger| {
                seen.lock().unwrap().push((started.elapsed(), trigger));
                async {}
            },
        ));
        tokio::time::sleep(AFTER_START - Duration::from_millis(1)).await;
        assert!(ran.lock().unwrap().is_empty(), "nothing before 10 minutes");
        tokio::time::sleep(Duration::from_millis(1)).await;
        tokio::task::yield_now().await;
        assert_eq!(ran.lock().unwrap().clone(), [(AFTER_START, "missed")]);
        tokio::time::sleep(Duration::from_secs(3 * 3600)).await;
        tender.await.unwrap();
        let r = ran.lock().unwrap().clone();
        assert_eq!(
            r[1],
            (AFTER_START + Duration::from_secs(3 * 3600), "nightly")
        );
    }

    /// The run's thread is named `learning` and runs at nice 19.
    #[tokio::test]
    async fn the_run_takes_a_low_priority_thread_of_its_own() {
        let rx = on_low_thread(|| std::thread::current().name().map(str::to_string)).unwrap();
        let (nice, name) = rx.await.unwrap();
        assert_eq!(nice, NICE);
        assert_eq!(name.as_deref(), Some("learning"));
    }
}
