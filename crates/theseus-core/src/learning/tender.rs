//! The learning tender (M5 25c; design §2.9, §2.16 FAST): the nightly
//! report, after serving and never on the start path.
//!
//! - **When.** At `[judge] learning_hour` local time (3 by default). A night
//!   the daemon missed (no run since the last such hour) runs once, as soon
//!   as it may. Never within [`AFTER_START`] of a start: the start's
//!   aftermath stays quiet.
//! - **Where.** On a thread of its own (`learning`), at nice 19 and in
//!   `SCHED_IDLE` (theseus-tood: it answers no one, so the one-way switch
//!   costs nothing), and at about 5% of a core: after each pack's stretch of
//!   work it sleeps 19 times as long, as the store's history check does.
//!   Before each next pack it also waits while the machine is busy, up to
//!   `theseus_store::pressure::BOUND`. The tender's task holds the core
//!   weakly, and the thread holds it only for its run.
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

/// What the run's thread took: its nice value, and whether it is in
/// `SCHED_IDLE` (theseus-tood).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Low {
    pub nice: i32,
    pub idle: bool,
}

/// Run `f` on a thread of its own, named `learning`, at nice [`NICE`]; what
/// it took (or kept, where a change failed) comes back with `f`'s result.
/// The owner's runs (replay, audit, backfill), whose answers someone waits
/// for.
pub fn on_low_thread<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> std::io::Result<tokio::sync::oneshot::Receiver<(Low, T)>> {
    spawn_low(f, false)
}

/// [`on_low_thread`], the thread also in `SCHED_IDLE`, which it never
/// leaves (theseus-tood): for the nightly run, which answers no one and
/// starts no thread others use.
pub fn on_idle_thread<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> std::io::Result<tokio::sync::oneshot::Receiver<(Low, T)>> {
    spawn_low(f, true)
}

fn spawn_low<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
    idle: bool,
) -> std::io::Result<tokio::sync::oneshot::Receiver<(Low, T)>> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("learning".into())
        .spawn(move || {
            let low = lower_this_thread(idle);
            let _ = tx.send((low, f()));
        })?;
    Ok(rx)
}

/// Lower this thread's priority to [`NICE`] (Linux's nice is per thread),
/// and with `idle` put it in `SCHED_IDLE`. What it has after.
fn lower_this_thread(idle: bool) -> Low {
    // SAFETY: gettid and setpriority/getpriority on this thread's own id
    // read and change nothing but its scheduling priority.
    let nice = unsafe {
        let tid = libc::syscall(libc::SYS_gettid) as libc::id_t;
        if libc::setpriority(libc::PRIO_PROCESS, tid, NICE) != 0 {
            tracing::debug!(error = %std::io::Error::last_os_error(), "learning: the thread's priority was not lowered");
        }
        libc::getpriority(libc::PRIO_PROCESS, tid)
    };
    if idle {
        if let Err(e) = theseus_store::pressure::idle_this_thread() {
            tracing::debug!(error = %e, "learning: the thread did not take SCHED_IDLE");
        }
    }
    Low {
        nice,
        idle: theseus_store::pressure::this_thread_is_idle(),
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
                    let rx = on_idle_thread(move || {
                        use theseus_store::pressure::{quiet_blocking_unless, BOUND};
                        let c = core.upgrade()?;
                        // Each next pack waits while the machine is busy, and
                        // never past a stop (theseus-tood).
                        let stopping = || c.outbox.stopping();
                        let mut yielded = Duration::ZERO;
                        let mut paced = |took: Duration| {
                            std::thread::sleep(took * 19);
                            yielded += quiet_blocking_unless(BOUND, stopping);
                        };
                        let r =
                            c.run_learning(theseus_protocol::now_unix_ms(), trigger, &mut paced);
                        // The ladder's rules again, as the backstop (26a).
                        c.runner.judge.ladder().recheck();
                        // Then the learning loop (25f): each lineage's
                        // proposal, after the report, each next one
                        // paced as the packs are.
                        if r.is_ok() {
                            let props =
                                c.learn_nightly(&rt, theseus_protocol::now_unix_ms(), &mut paced);
                            tracing::info!(
                                proposals = props.iter().filter(|p| p.decision != "none").count(),
                                "learning: the loop ran"
                            );
                        }
                        Some((r, yielded))
                    });
                    match rx {
                        Ok(rx) => match rx.await {
                            Ok((_, Some((Err(e), _)))) => {
                                tracing::warn!(error = %format!("{e:#}"), "learning: the report did not run");
                            }
                            Ok((low, Some((Ok(r), yielded)))) => tracing::info!(
                                date = %r.date, trigger, packs = r.packs.len(),
                                system_labels = r.labels.system_written, nice = low.nice,
                                sched_idle = low.idle, yielded_ms = yielded.as_millis() as u64,
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

    /// The thread's policy, as `/proc` says: field 41 of its `stat`,
    /// counted after the command's closing paren.
    fn proc_policy() -> i32 {
        // SAFETY: gettid has no arguments.
        let tid = unsafe { libc::syscall(libc::SYS_gettid) };
        let stat = std::fs::read_to_string(format!("/proc/self/task/{tid}/stat")).unwrap();
        let after = &stat[stat.rfind(')').unwrap() + 2..];
        after.split(' ').nth(41 - 3).unwrap().parse().unwrap()
    }

    /// The nightly run's thread is named `learning` and runs at nice 19 and
    /// in `SCHED_IDLE`, as `/proc` says of it too (theseus-tood); an owner's
    /// run, which someone waits for, at nice 19 alone; and the caller's
    /// thread is left as it was.
    #[tokio::test]
    async fn the_run_takes_a_low_priority_thread_of_its_own() {
        let name = || std::thread::current().name().map(str::to_string);
        let (low, (named, policy)) = on_idle_thread(move || (name(), proc_policy()))
            .unwrap()
            .await
            .unwrap();
        assert_eq!(
            low,
            Low {
                nice: NICE,
                idle: true
            }
        );
        assert_eq!(policy, libc::SCHED_IDLE, "the thread's policy in /proc");
        assert_eq!(named.as_deref(), Some("learning"));
        let (low, policy) = on_low_thread(proc_policy).unwrap().await.unwrap();
        assert_eq!(
            low,
            Low {
                nice: NICE,
                idle: false
            }
        );
        assert_eq!(policy, libc::SCHED_OTHER);
        assert!(!theseus_store::pressure::this_thread_is_idle());
    }

    /// A blocking task reached through `block_on` from the nightly run's
    /// thread starts the pool's thread on that thread, which inherits its
    /// SCHED_IDLE (theseus-bgg5); spawned on the runtime first, the same call
    /// starts it on a worker, in SCHED_OTHER.
    #[test]
    fn a_pool_thread_started_from_the_idle_thread_keeps_its_policy() {
        let fresh = || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .build()
                .unwrap()
        };
        // The fault, as the loop's `rt.block_on(request)` meets it.
        let rt = fresh();
        let h = rt.handle().clone();
        let direct = rt.block_on(async move {
            on_idle_thread(move || {
                let _g = h.enter();
                h.block_on(async { tokio::task::spawn_blocking(proc_policy).await.unwrap() })
            })
            .unwrap()
            .await
            .unwrap()
            .1
        });
        assert_eq!(
            direct,
            libc::SCHED_IDLE,
            "the pool thread took the idle thread's policy"
        );
        // The fix's shape: the request on the runtime, waited for on the
        // idle thread.
        let rt = fresh();
        let h = rt.handle().clone();
        let spawned = rt.block_on(async move {
            on_idle_thread(move || {
                h.block_on(
                    h.spawn(async { tokio::task::spawn_blocking(proc_policy).await.unwrap() }),
                )
                .unwrap()
            })
            .unwrap()
            .await
            .unwrap()
            .1
        });
        assert_eq!(spawned, libc::SCHED_OTHER, "a worker's child");
    }
}
