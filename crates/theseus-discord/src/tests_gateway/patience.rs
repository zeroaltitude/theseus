//! `Gateway::until`'s deadline (b7-gateway-until-deadline-family).
//!
//! Mechanism: `until` gave up after a fixed 20 s of wall clock, whatever the
//! machine was doing. On a loaded 4-core box a test that waits for a message
//! shares the CPU with a dozen other daemons, so the same work took longer
//! than 20 s and the test failed with "timed out" for no wrong answer.
//! The wait still ends at 20 s of *unstalled* time: each 25 ms nap that
//! overslept is credited with its overshoot (the time the scheduler kept us
//! off the CPU), up to `MAX_CREDIT`, so a quiet machine fails at 20 s as
//! before and a starved one gets its stalls back, and a wait that is wrong
//! still ends (at 20 s plus at most `MAX_CREDIT`).

use std::time::{Duration, Instant};

const BASE: Duration = Duration::from_secs(20);
const NAP: Duration = Duration::from_millis(25);
const MAX_CREDIT: Duration = Duration::from_secs(60);

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

    fn oversleep(&mut self, slept: Duration) {
        self.credit = (self.credit + slept.saturating_sub(NAP * 2)).min(MAX_CREDIT);
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
        p.oversleep(Duration::from_millis(30));
        assert_eq!(p.credit, Duration::ZERO);
        p.oversleep(Duration::from_secs(5));
        assert!(p.credit >= Duration::from_secs(4));
        assert!(!p.spent_at(Duration::from_secs(24)));
        assert!(p.spent_at(Duration::from_secs(26)));
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
