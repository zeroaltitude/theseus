//! `Gateway::until`'s deadline (b7-gateway-until-deadline-family).
//!
//! Mechanism: `until` gave up after a fixed 20 s of wall clock, whatever the
//! machine was doing. On a loaded 4-core box a test that waits for a message
//! shares the CPU with a dozen other daemons, so the same work took longer
//! than 20 s and the test failed with "timed out" for no wrong answer.
//! The wait ends at 20 s of *unstalled* time: each 25 ms nap that overslept
//! is credited with its overshoot (the time the scheduler kept us off the
//! CPU), up to `MAX_CREDIT`, so a quiet machine fails at 20 s as before and a
//! starved one gets its stalls back, and a wait that is wrong still ends (at
//! 20 s plus at most `MAX_CREDIT`).
//!
//! ROUND 2. The paragraph above is round 1's claim, and round 1's arithmetic
//! did not keep it: it credited `slept - NAP * 2`, forgiving a whole extra
//! `NAP` of real stall on EVERY nap. What makes that fatal is not the width
//! of the deadband but that it is charged PER NAP while the budget it guards
//! is charged ONCE. A `BASE` budget holds `BASE / NAP` = 800 naps, so a
//! deadband of one `NAP` can absorb 800 * 25 ms = 20 s -- the whole budget.
//! In the band a loaded box actually sits in (naps returning in 26-50 ms,
//! i.e. 1x to 2x starvation) the credit is EXACTLY ZERO, so the helper decays
//! to the fixed wall clock it was written to replace, and the family it was
//! meant to steady goes red again for 20 s of wall clock holding as little as
//! 10 s of work.
//!
//! So the deadband is now `SLOP`, a small ABSOLUTE allowance for the timer's
//! own granularity, and its aggregate is stated rather than left to be
//! discovered: at most `BASE / NAP * SLOP` = 1.6 s of the 20 s, 8 %.
//!
//! Known residual, named rather than fixed: the credit is measured from the
//! overshoot of OUR OWN naps, and a sleeping task is the cheapest thing on a
//! starved box to wake -- its timer fires it onto whichever worker is idle,
//! while the gateway's runnable tasks queue behind every other runnable
//! thread. Our overshoot is therefore a lower bound on the starvation the
//! thing we are waiting for suffers, never an upper one.
//!
//! Which way that bias cuts is worth stating exactly, because it is the
//! OPPOSITE of conservative. `spent_at` is
//! `elapsed.saturating_sub(credit) >= BASE`, so LESS credit means the wait
//! gives up SOONER: an under-crediting estimator risks an EARLY red -- a
//! spurious timeout under load, which is precisely the flake this file
//! exists to prevent. (A LATE red is what an OVER-crediting estimator would
//! risk, and that is not the error we have.) It cannot cause an early green,
//! and that holds for a better reason than the bias, one that does not
//! depend on its sign at all: `until` returns ONLY when its predicate is
//! true, and the budget governs nothing but when it panics, so no credit
//! error in either direction can manufacture a pass. So the residual is a
//! real limit of the estimator and the first place to look if this family
//! reddens again -- it is not a margin of safety.

use std::time::{Duration, Instant};

const BASE: Duration = Duration::from_secs(20);
const NAP: Duration = Duration::from_millis(25);
const MAX_CREDIT: Duration = Duration::from_secs(60);
/// What a nap may overshoot by without being called a stall: the timer's own
/// granularity, and nothing else. It is a small ABSOLUTE value and
/// deliberately not a fraction of `NAP` -- see `oversleep` for why that
/// distinction is the whole of this file.
const SLOP: Duration = Duration::from_millis(2);

pub(crate) struct Patience {
    start: Instant,
    credit: Duration,
}

impl Patience {
    pub(crate) fn new() -> Self {
        Patience {
            start: Instant::now(),
            credit: Duration::ZERO,
        }
    }

    pub(crate) fn spent(&self) -> bool {
        self.spent_at(self.start.elapsed())
    }

    fn spent_at(&self, elapsed: Duration) -> bool {
        elapsed.saturating_sub(self.credit) >= BASE
    }

    /// Sleeps one nap and credits what it overslept.
    pub(crate) async fn nap(&mut self) {
        let t = Instant::now();
        tokio::time::sleep(NAP).await;
        self.oversleep(t.elapsed());
    }

    /// The stall is what the nap overshot the nap by -- `slept - NAP` -- less
    /// `SLOP` for the timer's granularity. It is NOT `slept - NAP * 2`: that
    /// discards a second whole `NAP` of real stall every time it is called,
    /// and it is called hundreds of times inside one budget. See the header.
    fn oversleep(&mut self, slept: Duration) {
        self.credit = (self.credit + slept.saturating_sub(NAP + SLOP)).min(MAX_CREDIT);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fresh() -> Patience {
        Patience::new()
    }

    #[test]
    fn a_quiet_machine_gives_up_at_twenty_seconds() {
        let p = fresh();
        assert!(!p.spent_at(Duration::from_secs(19)));
        assert!(p.spent_at(Duration::from_secs(20)));
    }

    #[test]
    fn a_stall_is_credited_but_a_normal_nap_is_not() {
        let mut p = fresh();
        // Was `30ms` in round 1. 30 ms is a 5 ms stall on a 25 ms nap, and the
        // point of this file is that 5 ms stalls are real; only the timer's own
        // granularity is not. Every other assertion here is round 1's, unchanged.
        p.oversleep(NAP + SLOP);
        assert_eq!(p.credit, Duration::ZERO);
        p.oversleep(Duration::from_secs(5));
        assert!(p.credit >= Duration::from_secs(4));
        assert!(!p.spent_at(Duration::from_secs(24)));
        assert!(p.spent_at(Duration::from_secs(26)));
    }

    /// The regime that reddened the gateway family. Every nap returns in 40 ms
    /// -- a 15 ms stall on a 25 ms nap, which is ordinary 1.6x starvation on a
    /// loaded box, not a pathological one.
    ///
    /// Round 1 forgave `NAP * 2` per nap, so a 40 ms nap credited EXACTLY
    /// NOTHING and the helper decayed to round 0's fixed wall clock: it gave up
    /// after 20 s of wall clock holding only 12.5 s of unstalled work. The
    /// deadband is charged per nap and a `BASE` budget holds hundreds of naps,
    /// so its aggregate capacity is the entire budget.
    #[test]
    fn moderate_starvation_is_credited_so_the_budget_is_not_halved() {
        let mut p = fresh();
        let nap = Duration::from_millis(40);
        let naps = 500; // 500 * 40 ms = exactly `BASE` of wall clock.
        for _ in 0..naps {
            p.oversleep(nap);
        }
        let elapsed = nap * naps;
        assert_eq!(
            elapsed, BASE,
            "the loop has burned exactly BASE of wall clock"
        );

        // The credit is the real stall less the slop, and nothing else is
        // forgiven. This pins the arithmetic rather than asserting a bound.
        assert_eq!(p.credit, (nap - NAP) * naps - SLOP * naps);

        // Only 12.5 s of that wall clock was unstalled, so the wait is NOT
        // spent. Round 1 fails here: its credit is zero and `spent_at` is true.
        assert!(
            !p.spent_at(elapsed),
            "20 s of wall clock at 1.6x starvation holds 12.5 s of work; credit was {:?}",
            p.credit,
        );

        // And it IS spent once the unstalled time really does reach `BASE`, so
        // the wait still ends.
        assert!(p.spent_at(elapsed + p.credit));
    }

    /// The other side of the same line: the timer's own granularity must never
    /// become credit, or a wait that is genuinely wrong stretches for free.
    #[test]
    fn the_timers_own_granularity_is_not_a_stall() {
        let mut p = fresh();
        for _ in 0..(BASE.as_millis() / NAP.as_millis()) {
            p.oversleep(NAP + SLOP);
        }
        assert_eq!(p.credit, Duration::ZERO);
        assert!(p.spent_at(BASE));
    }

    #[test]
    fn credit_is_bounded_so_a_wrong_wait_still_ends() {
        let mut p = fresh();
        for _ in 0..100 {
            p.oversleep(Duration::from_secs(30));
        }
        assert_eq!(p.credit, MAX_CREDIT);
        assert!(p.spent_at(BASE + MAX_CREDIT));
    }
}
