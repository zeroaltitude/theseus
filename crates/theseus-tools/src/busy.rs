//! How busy the machine is, measured by the test that has to live with it (theseus-20). It lives in this
//! crate because the tests of the core, the kernel, the simulator and the daemon all reach it (the
//! simulator re-exports it as `theseus_sim::busy_factor`).
//!
//! A test whose bound is a latency the product promises (the brief's "within a second", a 99th
//! percentile of ten milliseconds) holds on a quiet machine and cannot hold on one that gives each
//! thread a third of a core: the promise is about the code, and the wait for a turn on the CPU is
//! the machine's. The gate's benches already answer this with a busy allowance that scales the
//! timing budgets (never the counts) by what the machine measured. [`busy_factor`] is that, for a
//! test: a quiet machine reads 1, and the promise is held exactly; a busy one reads what it is, and
//! the promise is held in proportion. It is for a promise, never for a mechanism: a test of an
//! order, a count or a timer the code owns uses a clock the test controls instead.

use std::time::{Duration, Instant};

/// The CPU time one probe spins for: long enough to span several scheduler slices, short enough
/// that three of them cost a tenth of a second on a quiet machine.
const PROBE_CPU: Duration = Duration::from_millis(20);
/// The most a probe's answer counts, so one stalled sample cannot make a bound meaningless.
const CEILING: u64 = 50;

/// This thread's CPU time (`CLOCK_THREAD_CPUTIME_ID`).
fn thread_cpu() -> Duration {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: clock_gettime writes the timespec it is given, nothing else.
    unsafe { libc::clock_gettime(libc::CLOCK_THREAD_CPUTIME_ID, &mut ts) };
    Duration::new(ts.tv_sec as u64, ts.tv_nsec as u32)
}

/// How many times longer than its own CPU time a short spin of this thread took on the wall clock:
/// 1 when nothing else wanted the core, 3 when two others did.
fn spin_slowdown() -> f64 {
    let (wall, cpu) = (Instant::now(), thread_cpu());
    let mut x = 1u64;
    while thread_cpu() - cpu < PROBE_CPU {
        for _ in 0..2_000 {
            x = std::hint::black_box(x.wrapping_mul(6364136223846793005).wrapping_add(1));
        }
    }
    wall.elapsed().as_secs_f64() / (thread_cpu() - cpu).as_secs_f64()
}

/// How many times longer than asked twenty 1 ms sleeps in a row took: every wake-up waits for a
/// core, and a job that is many wake-ups in a row (a process's exit, a daemon's stop) is slowed by
/// far more than the share of a core it is given.
fn wake_slowdown() -> f64 {
    let t = Instant::now();
    for _ in 0..20 {
        std::thread::sleep(Duration::from_millis(1));
    }
    t.elapsed().as_secs_f64() / 0.020
}

/// How many times slower than a quiet machine this one is giving a thread its turn right now: the
/// worst of three short spins' wall time over their CPU time and three runs of twenty short
/// sleeps over the time asked, to the nearest whole number, never below 1 and never above 50.
/// Multiply a latency promise's bound by it.
pub fn busy_factor() -> u64 {
    let worst = (0..3)
        .flat_map(|_| [spin_slowdown(), wake_slowdown()])
        .fold(1.0, f64::max);
    (worst.round() as u64).clamp(1, CEILING)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    /// Four times as many busy threads as cores make the probe wait for its turn, and the factor
    /// says so; with them gone it falls back. (Not 1: the suite's own tests share the machine.)
    #[test]
    fn a_machine_crowded_with_busy_threads_reads_above_one() {
        let cores = std::thread::available_parallelism().map_or(4, |n| n.get());
        let stop = Arc::new(AtomicBool::new(false));
        let spinners: Vec<_> = (0..cores * 4)
            .map(|_| {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        std::hint::spin_loop();
                    }
                })
            })
            .collect();
        let crowded = busy_factor();
        stop.store(true, Ordering::Relaxed);
        for s in spinners {
            s.join().unwrap();
        }
        assert!(crowded >= 2, "a crowded machine read {crowded}");
        assert!(crowded <= CEILING);
    }

    #[test]
    fn the_factor_is_never_below_one() {
        assert!((1..=CEILING).contains(&busy_factor()));
    }
}
