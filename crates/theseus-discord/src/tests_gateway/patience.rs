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

//! ISSUE 14: ONE BUDGET PER RIG. Everything above describes the budget of ONE wait. A test makes many, each used to
//! start a budget of its own, and the runner's kill (120 s: nextest's `slow-timeout`) comes after at most 30 s of
//! connecting plus one budget and its credit. `Budget` below is what a rig's waits share: the unstalled time spent
//! napping, summed over all of them. Its start is the first nap, not the rig's construction, and the test's own work
//! between waits is not charged.
//!
//! Every LONG wait of the rig goes through `Rig::until` or `Rig::until_on`, except the task family's (the family's
//! `until`), which ends at one deadline of `TASK_WAIT` from the rig's construction, and
//! `tests_process.rs`'s held-create wait, which keeps a `Patience` of its own. A few bounded loops of 10 s or less
//! (`tests_process.rs`, `tests_typing.rs`, `horizon.rs`) sit outside too, and so does the war-room helper
//! `answered`, which ends at its own 20 s or at the rig's deadline, whichever is first. A test that builds a second rig gets a
//! second budget.

use std::sync::Mutex;
use std::time::{Duration, Instant};

const BASE: Duration = Duration::from_secs(20);
/// The task family's one deadline per rig, from the rig's construction (`Rig::task_wait_spent`): the connect wait
/// (30 s), the bind wait (up to `BASE` + `MAX_CREDIT`) and every task wait of the rig all end inside it, and inside
/// the runner's 120 s kill, with ten seconds over. A test that builds a second rig has a second deadline. 100 s per
/// wait, from when each was called, did not fit.
pub(crate) const TASK_WAIT: Duration = Duration::from_secs(110);
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

/// ONE patience budget for the waits of a test's rig (issue 14). Each `until` used to start a `Patience` of its own, so
/// a test of k waits could spend k budgets, and `BASE + MAX_CREDIT` is already 80 s: two starved waits and the
/// runner's hard kill (`a_tests_waits_end_before_the_runners_kill`) took the test with no message of ours.
///
/// What it counts is the time the test spent INSIDE its waits, nap by nap, with each nap's stall credited as in
/// `Patience`: the unstalled waiting of every `until` of the rig adds up to `BASE`, and a wait that gets less than
/// `BASE` is one that came after another had spent some. The test's own work between waits (core calls, `say`,
/// assertions) is not charged, and nothing is charged before the first nap, so a slow start is not a red. It is
/// per RIG: a test that builds a second one (`tests_task_home`'s restart test) has a second budget, and the waits of
/// the task family's `until` are bounded by the rig's deadline instead (`Rig::task_wait_spent`).
pub(crate) struct Budget(Mutex<Spending>);

#[derive(Default)]
struct Spending {
    /// The time spent napping, as the clock saw it.
    waited: Duration,
    /// The stalls among it, credited.
    credit: Duration,
}

impl Budget {
    pub(crate) fn new() -> Self {
        Budget(Mutex::new(Spending::default()))
    }

    /// The unstalled time spent waiting so far: what `BASE` is measured against.
    pub(crate) fn unstalled(&self) -> Duration {
        let s = self.0.lock().unwrap();
        s.waited.saturating_sub(s.credit)
    }

    pub(crate) fn spent(&self) -> bool {
        self.unstalled() >= BASE
    }

    /// Charges one nap that took `slept`: all of it is waiting, and what it overslept is a stall, credited exactly
    /// as `Patience::oversleep` credits it.
    pub(crate) fn account(&self, slept: Duration) {
        let mut s = self.0.lock().unwrap();
        s.waited += slept;
        s.credit = (s.credit + slept.saturating_sub(NAP + SLOP)).min(MAX_CREDIT);
    }

    /// Sleeps one nap and charges it. The lock is not held across the sleep.
    pub(crate) async fn nap(&self) {
        let t = Instant::now();
        tokio::time::sleep(NAP).await;
        self.account(t.elapsed());
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::tests_gateway::{Rig, ANA_DM};

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

    #[test]
    fn a_budget_is_one_for_every_wait_of_a_rig() {
        // Two waits' naps add up: 12 s of unstalled waiting in each is 24 s against one budget of 20 s, where a
        // `Patience` of its own per wait would have been spent by neither.
        let b = Budget::new();
        assert!(!b.spent(), "a fresh budget is not spent");
        for _ in 0..300 {
            b.account(NAP); // the first wait, unstalled: 7.5 s
        }
        assert!(!b.spent(), "7.5 s is under the budget");
        for _ in 0..400 {
            b.account(NAP); // the second wait: 17.5 s of a 20 s budget in all
        }
        assert!(!b.spent());
        for _ in 0..100 {
            b.account(NAP);
        }
        assert!(
            b.spent(),
            "the second wait inherits what the first spent: {:?}",
            b.unstalled()
        );
        assert!(!fresh().spent(), "a Patience of its own begins again");
    }

    #[test]
    fn a_stall_is_credited_to_the_budget_and_a_normal_nap_is_not() {
        let b = Budget::new();
        b.account(NAP + SLOP);
        assert_eq!(
            b.unstalled(),
            NAP + SLOP,
            "the timer's own granularity is waiting, not a stall"
        );
        let b = Budget::new();
        b.account(Duration::from_secs(5));
        assert!(
            b.unstalled() <= NAP + SLOP,
            "a 5 s nap is nearly all stall and is credited: {:?}",
            b.unstalled()
        );
    }

    #[test]
    fn the_budget_charges_the_naps_and_not_the_time_between_waits() {
        let b = Budget::new();
        std::thread::sleep(Duration::from_millis(60)); // the test's own work, between waits
        assert_eq!(
            b.unstalled(),
            Duration::ZERO,
            "nothing is charged before a nap"
        );
        b.account(NAP);
        assert_eq!(b.unstalled(), NAP, "only the nap");
    }

    /// The panic text of a wait that gave up, from a rig whose budget `spend` has already drawn on.
    async fn timed_out(
        f: impl FnOnce(&Rig) -> bool,
        wait: impl FnOnce(Arc<Rig>) -> tokio::task::JoinHandle<()>,
    ) -> String {
        let r = Arc::new(Rig::start(|_| vec![], false).await);
        assert!(f(&r));
        let began = Instant::now();
        let e = wait(r).await.expect_err("the wait gave up");
        assert!(
            began.elapsed() < Duration::from_secs(5),
            "it gave up at once, with the budget spent: {:?}",
            began.elapsed()
        );
        *e.into_panic()
            .downcast::<String>()
            .expect("an assert! message")
    }

    fn spend_the_budget(r: &Rig) -> bool {
        for _ in 0..(BASE.as_millis() / NAP.as_millis()) {
            r.patience.account(NAP);
        }
        r.patience.spent()
    }

    /// The rig's own `until_on` draws on the rig's budget: a wait that comes after 20 s of waiting has none left and
    /// says so at once. With a `Patience` of its own per wait (the old behaviour) this wait would have napped 20 s.
    #[tokio::test]
    async fn a_wait_of_a_rig_that_has_spent_its_budget_gives_up_at_once() {
        let said = timed_out(spend_the_budget, |r| {
            tokio::spawn(async move { r.until_on(&[ANA_DM], "the second wait", || false).await })
        })
        .await;
        assert!(said.starts_with("timed out: the second wait"), "{said}");
    }

    /// A timeout names the surfaces its wait reads, and one that cannot say so says that and marks none: the old
    /// message named `#lab` first for a wait that reads the DM.
    #[tokio::test]
    async fn a_timeout_names_the_surfaces_the_wait_reads_or_says_it_cannot() {
        let named = timed_out(spend_the_budget, |r| {
            tokio::spawn(async move { r.until_on(&[ANA_DM], "named", || false).await })
        })
        .await;
        assert!(named.contains("the DM (read by this wait):"), "{named}");
        assert!(named.contains("#lab (not read by this wait):"), "{named}");
        assert!(
            named.find("the DM").unwrap() < named.find("#lab").unwrap(),
            "{named}"
        );
        let unnamed = timed_out(spend_the_budget, |r| {
            tokio::spawn(async move { r.until("unnamed", || false).await })
        })
        .await;
        assert!(
            unnamed.contains("this wait did not say which surfaces it reads"),
            "{unnamed}"
        );
        assert!(
            !unnamed.contains("read by this wait"),
            "no surface is marked: {unnamed}"
        );
        assert!(
            unnamed.contains("the DM:") && unnamed.contains("#lab:"),
            "{unnamed}"
        );
    }

    /// The runner's kill: `.config/nextest.toml`'s `slow-timeout`, `period` times `terminate-after`.
    fn the_runners_kill() -> Duration {
        let cfg = include_str!("../../../../.config/nextest.toml");
        let line = cfg
            .lines()
            .find(|l| l.trim_start().starts_with("slow-timeout"))
            .expect("nextest.toml sets slow-timeout");
        let num = |key: &str| -> u64 {
            let at = line.find(key).unwrap_or_else(|| panic!("{key} in {line}"));
            line[at + key.len()..]
                .trim_start_matches([' ', '=', '"'])
                .chars()
                .take_while(char::is_ascii_digit)
                .collect::<String>()
                .parse()
                .unwrap_or_else(|_| panic!("{key} in {line}"))
        };
        Duration::from_secs(num("period") * num("terminate-after"))
    }

    /// The most a rig's waits can take (its connect wait, the budget with its credit) must end before the runner's
    /// kill, or the kill takes the test with no word of ours. It counts the waits, not the test's own work between
    /// them, and one rig: see `Budget` for the test that builds two.
    #[test]
    fn a_tests_waits_end_before_the_runners_kill() {
        let kill = the_runners_kill();
        let connect = Duration::from_secs(30);
        assert!(
            connect + BASE + MAX_CREDIT < kill,
            "connect {connect:?} + budget {BASE:?} + credit {MAX_CREDIT:?} reaches the kill at {kill:?}"
        );
    }

    /// The task family's waits end at one deadline per rig, counted from the rig's construction, so its connect
    /// wait and bind wait are inside it; it must end before the kill, or the family's waits (100 s each before
    /// this) can pass it.
    #[test]
    fn the_task_deadline_is_inside_the_runners_kill() {
        let kill = the_runners_kill();
        assert!(
            Duration::from_secs(30) + BASE + MAX_CREDIT <= TASK_WAIT,
            "the connect wait and the bind wait, at their longest, end inside the task deadline"
        );
        assert!(
            TASK_WAIT < kill,
            "the task deadline {TASK_WAIT:?} reaches the kill at {kill:?}"
        );
        assert!(
            TASK_WAIT + Duration::from_secs(5) <= kill,
            "and leaves a margin for the message to be said"
        );
    }

    #[tokio::test]
    async fn the_task_deadline_runs_from_the_rigs_construction() {
        let mut r = Rig::start(|_| vec![], false).await;
        assert!(!r.task_wait_spent(), "a fresh rig has its deadline ahead");
        r.pretend_the_task_deadline_passed();
        assert!(r.task_wait_spent());
    }
}
