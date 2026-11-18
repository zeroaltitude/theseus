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

/// The most a speed promise's bound is ever multiplied by ([`Busy::keeps`]). A bound scaled without a
/// limit promises nothing: at 50 a two-second bound would pass a daemon that took a hundred seconds. Ten
/// is a machine that gives a thread a tenth of a core. Measured on a 32-core box (40 samples each): idle,
/// every one read 1; beside 64 CPU burners, 2 to 4; beside 64 burners and 96 more busy threads, 4 to 8,
/// median 5. So ten holds the worst of those with room, and beyond it the machine is not one a promise
/// about milliseconds can be tested on: the test fails and says so.
pub const BOUND_CAP: u64 = 10;

/// [`busy_factor`] held to [`BOUND_CAP`]: for a test whose limit is an INPUT it scales together with the
/// bound that depends on it (a fake's latency and the margin under it), where [`Busy`] is for a bound
/// alone. 1 on a quiet machine, so there the input is exactly what it was.
pub fn bound_factor() -> u64 {
    busy_factor().min(BOUND_CAP)
}

/// What a test that holds a speed promise asks the machine, and when. Take it with [`Busy::before`] just
/// before the region the promise times (not at the top of a test whose setup takes seconds: the load then
/// is not the load during the region), and ask [`Busy::keeps`] of every bound of that region. On a quiet
/// machine the factor is 1 and every bound is exactly what it says.
pub struct Busy {
    before: u64,
    seen: std::sync::atomic::AtomicU64,
}

impl Busy {
    /// The factor now, before the timed region.
    pub fn before() -> Busy {
        Busy::with(busy_factor())
    }

    /// No reading before the region: the machine is asked only when a bound is passed, so a bound is held
    /// at how slowly a thread got its turn then. For a region too long to sample at its start, or a test
    /// that times many (an acceptance journey's steps).
    pub fn when_late() -> Busy {
        Busy::with(1)
    }

    fn with(before: u64) -> Busy {
        Busy {
            before,
            seen: before.into(),
        }
    }

    /// Whether `elapsed` keeps a promise of `bound` ("within": the bound itself is kept): always when it is
    /// within the bound; when it is over, when it is within the bound times the busier of the factor before
    /// the region and the factor right now (the machine is asked only then, so a loop that checks every few
    /// milliseconds pays nothing), never above [`BOUND_CAP`] times.
    pub fn keeps(&self, elapsed: Duration, bound: Duration) -> bool {
        self.keeps_with(elapsed, bound, busy_factor)
    }

    fn keeps_with(&self, elapsed: Duration, bound: Duration, now: impl FnOnce() -> u64) -> bool {
        use std::sync::atomic::Ordering::Relaxed;
        if elapsed <= bound {
            return true;
        }
        let factor = self.before.max(now()).clamp(1, BOUND_CAP);
        self.seen.fetch_max(factor, Relaxed);
        elapsed <= bound * factor as u32
    }
}

/// For a failure message: the factor a bound was held at (the busiest asked), and the cap.
impl std::fmt::Display for Busy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let seen = self.seen.load(std::sync::atomic::Ordering::Relaxed);
        write!(f, "busy x{seen} of at most x{BOUND_CAP}")
    }
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

    const S: Duration = Duration::from_secs(1);

    #[test]
    fn a_bound_kept_asks_nothing_of_the_machine() {
        let busy = Busy::with(1);
        assert!(busy.keeps_with(S / 2, S, || panic!("asked")));
    }

    #[test]
    fn on_a_quiet_machine_the_bound_is_exactly_the_bound() {
        let busy = Busy::with(1);
        assert!(busy.keeps_with(S, S, || 1));
        assert!(!busy.keeps_with(S + Duration::from_nanos(1), S, || 1));
        assert!(!busy.keeps_with(S * 2, S, || 1));
    }

    #[test]
    fn a_busy_machine_holds_the_bound_in_proportion_and_the_busier_reading_counts() {
        let busy = Busy::with(3);
        assert!(
            busy.keeps_with(S * 2, S, || 1),
            "the factor before the region counts"
        );
        assert!(busy.keeps_with(S * 3, S, || 1));
        assert!(!busy.keeps_with(S * 3 + Duration::from_nanos(1), S, || 1));
        let quiet_before = Busy::with(1);
        assert!(
            quiet_before.keeps_with(S * 4, S, || 5),
            "the factor right now counts"
        );
        assert_eq!(
            quiet_before.to_string(),
            format!("busy x5 of at most x{BOUND_CAP}")
        );
    }

    #[test]
    fn the_factor_is_capped_so_a_real_slowdown_still_fails() {
        let busy = Busy::with(50);
        assert!(busy.keeps_with(S * BOUND_CAP as u32, S, || 50));
        assert!(!busy.keeps_with(S * BOUND_CAP as u32 + Duration::from_nanos(1), S, || 50));
    }
}
