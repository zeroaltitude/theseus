//! FSRS-6 retention (design M6 §2.7, step 32a), from the published algorithm.
//!
//! Built from FSRS-6's published equations, never from Vestige's source (it is
//! AGPL; design §1.4). A node's retention is a stability `S` (days until its
//! retrievability falls to 0.9) and a difficulty `D` (1 to 10). The forgetting
//! curve is `R(t, S) = (1 + f·t/S)^(−w20)`, with `f = 0.9^(−1/w20) − 1`, so that
//! `R(S, S) = 0.9`. Each review moves `S` and `D` by its grade; exposure
//! without use is no review, so it never moves them (theseus-3nk).
//!
//! Checked against the reference implementation, the `fsrs` crate 6.6.2
//! (open-spaced-repetition's FSRS for Rust, BSD-3-Clause): its defaults,
//! equations, bounds, and clamps, and the numbers its own tests pin
//! (theseus-3ht). Where its workload simulator and its model disagree (the
//! same-day floor), this follows the model, which tracks a card's state.
//!
//! Time is wall-clock days, as a fraction: a review less than a day after the
//! last one takes FSRS-6's same-day step. The published schedulers count whole
//! days; the curve itself is continuous, and so is an agent's day.

use crate::access::{Access, AccessEvent};

/// Milliseconds in a day: the unit of FSRS's `t` and `S`.
pub const MS_PER_DAY: f64 = 86_400_000.0;

/// The bounds a stability is held in, in days. `S_MIN` keeps the curve
/// defined; `S_MAX` is a hundred years.
pub const S_MIN: f64 = 0.001;
pub const S_MAX: f64 = 36_500.0;

/// The bounds a difficulty is held in.
pub const D_MIN: f64 = 1.0;
pub const D_MAX: f64 = 10.0;

/// The published default parameters of FSRS-6, `w0` to `w20`: the data set
/// `fsrs6-default`. `w0`–`w3` are the first stability by grade; `w4`–`w7`
/// shape difficulty; `w8`–`w10` a recall's stability; `w11`–`w14` a lapse's;
/// `w15` and `w16` are Hard's penalty and Easy's bonus; `w17`–`w19` the
/// same-day step; `w20` the curve's decay.
pub const FSRS6_DEFAULT: [f64; 21] = [
    0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796, 1.4835,
    0.0614, 0.2629, 1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658, 0.1542,
];

/// The bounds `w0` to `w20` are clipped into, as the reference's `FSRS::new`
/// clips them: with one relearning step, so `w17` and `w18` are at most 2,
/// and no short-term floor for `w19`. The defaults sit inside every one.
pub const PARAM_BOUNDS: [(f64, f64); 21] = [
    (S_MIN, 100.0),
    (S_MIN, 100.0),
    (S_MIN, 100.0),
    (S_MIN, 100.0),
    (D_MIN, D_MAX),
    (0.001, 4.0),
    (0.001, 4.0),
    (0.001, 0.75),
    (0.0, 4.5),
    (0.0, 0.8),
    (0.001, 3.5),
    (0.001, 5.0),
    (0.001, 0.25),
    (0.001, 0.9),
    (0.0, 4.0),
    (0.0, 1.0),
    (1.0, 6.0),
    (0.0, 2.0),
    (0.0, 2.0),
    (0.0, 0.8),
    (0.1, 0.8),
];

/// A review's grade: FSRS's four answers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Grade {
    Again = 1,
    Hard = 2,
    Good = 3,
    Easy = 4,
}

impl Grade {
    /// The grade as FSRS's `G`, 1 to 4.
    fn g(self) -> f64 {
        f64::from(self as u8)
    }
}

/// A node's retention: the FSRS state after its last review.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Retention {
    /// Days until retrievability falls to 0.9, in [`S_MIN`, `S_MAX`].
    pub stability: f64,
    /// How hard the node is to keep, in [`D_MIN`, `D_MAX`].
    pub difficulty: f64,
    /// The last review's wall-clock time, in ms since the epoch.
    pub last_review_ms: u64,
}

/// A parameter set that FSRS-6's math cannot use.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ParamsError {
    /// The parameter, `w<index>`.
    pub index: usize,
    pub value: f64,
    pub reason: &'static str,
}

impl std::fmt::Display for ParamsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "FSRS-6 parameter w{} = {}: {}",
            self.index, self.value, self.reason
        )
    }
}

impl std::error::Error for ParamsError {}

/// FSRS-6, with one parameter set.
#[derive(Clone, Debug, PartialEq)]
pub struct Fsrs6 {
    w: [f64; 21],
}

impl Default for Fsrs6 {
    /// The data set `fsrs6-default`.
    fn default() -> Self {
        Self { w: FSRS6_DEFAULT }
    }
}

impl Fsrs6 {
    /// FSRS-6 with the parameters `w`, each clipped into [`PARAM_BOUNDS`] as
    /// the reference's `FSRS::new` clips it, so that the same parameters make
    /// the same model. A parameter that is not finite is an error.
    pub fn new(mut w: [f64; 21]) -> Result<Self, ParamsError> {
        for (index, (value, &(low, high))) in w.iter_mut().zip(&PARAM_BOUNDS).enumerate() {
            if !value.is_finite() {
                return Err(ParamsError {
                    index,
                    value: *value,
                    reason: "not finite",
                });
            }
            *value = value.clamp(low, high);
        }
        Ok(Self { w })
    }

    /// The parameters, `w0` to `w20`, as clipped: what an arm's digest names.
    pub fn params(&self) -> &[f64; 21] {
        &self.w
    }

    /// The curve's decay, `w20`.
    pub fn decay(&self) -> f64 {
        self.w[20]
    }

    /// `f = 0.9^(−1/w20) − 1`: the curve's factor, so that `R(S, S) = 0.9`.
    pub fn factor(&self) -> f64 {
        0.9f64.powf(-1.0 / self.decay()) - 1.0
    }

    /// The forgetting curve: the chance of recall `t` days after a review
    /// that left stability `s`. In [0, 1], and 1 at `t = 0`; a negative or
    /// undefined `t` reads as 0.
    pub fn retrievability(&self, t: f64, s: f64) -> f64 {
        (1.0 + self.factor() * t.max(0.0) / s).powf(-self.decay())
    }

    /// A node's retrievability at `now_ms`.
    pub fn retrievability_at(&self, r: &Retention, now_ms: u64) -> f64 {
        self.retrievability(elapsed_days(r.last_review_ms, now_ms), r.stability)
    }

    /// The retention after a node's first review.
    pub fn initial(&self, grade: Grade, at_ms: u64) -> Retention {
        Retention {
            stability: clamp_stability(self.w[grade as usize - 1]),
            difficulty: clamp_difficulty(self.initial_difficulty(grade)),
            last_review_ms: at_ms,
        }
    }

    /// The retention after a review at `at_ms`, given the one before it. The
    /// prior's `S` and `D` are clamped into their bounds first, as the
    /// reference's step clamps them; the new stability reads the prior
    /// difficulty; then the difficulty moves.
    pub fn review(&self, prior: &Retention, grade: Grade, at_ms: u64) -> Retention {
        let s = clamp_stability(prior.stability);
        let d = clamp_difficulty(prior.difficulty);
        let t = elapsed_days(prior.last_review_ms, at_ms);
        let stability = if t < 1.0 {
            self.same_day_stability(s, grade)
        } else {
            let r = self.retrievability(t, s);
            match grade {
                Grade::Again => self.lapse_stability(s, d, r),
                _ => self.recall_stability(s, d, r, grade),
            }
        };
        Retention {
            stability: clamp_stability(stability),
            difficulty: self.next_difficulty(d, grade),
            // A clock that steps back never moves the last review back.
            last_review_ms: prior.last_review_ms.max(at_ms),
        }
    }

    /// One fold step: the retention after `ev`, given the retention before it
    /// (none before a node's first review). An event that is no review leaves
    /// it as it was, and so does a first sight once a node has been reviewed.
    pub fn step(&self, prior: Option<Retention>, ev: &AccessEvent) -> Option<Retention> {
        let Some(grade) = ev.access.grade() else {
            return prior;
        };
        match prior {
            None => Some(self.initial(grade, ev.at_ms)),
            Some(_) if matches!(ev.access, Access::FirstSight(_)) => prior,
            Some(p) => Some(self.review(&p, grade, ev.at_ms)),
        }
    }

    /// A node's retention, folded from its events in position order. A
    /// rebuild from every event equals the incremental fold, step by step.
    pub fn fold<'a>(&self, events: impl IntoIterator<Item = &'a AccessEvent>) -> Option<Retention> {
        events
            .into_iter()
            .fold(None, |prior, ev| self.step(prior, ev))
    }

    /// `D0(G) = w4 − e^(w5·(G−1)) + 1`, unclamped: the mean reversion reads
    /// it so for Easy.
    fn initial_difficulty(&self, grade: Grade) -> f64 {
        self.w[4] - (self.w[5] * (grade.g() - 1.0)).exp() + 1.0
    }

    /// `ΔD = −w6·(G−3)`, damped linearly as `D` nears 10, then reverted
    /// toward `D0(Easy)` by `w7`.
    fn next_difficulty(&self, d: f64, grade: Grade) -> f64 {
        let delta = -self.w[6] * (grade.g() - 3.0);
        let damped = d + delta * (10.0 - d) / 9.0;
        let reverted =
            self.w[7] * self.initial_difficulty(Grade::Easy) + (1.0 - self.w[7]) * damped;
        clamp_difficulty(reverted)
    }

    /// A recall a day or more on (Hard, Good, Easy):
    /// `S·(1 + e^w8·(11−D)·S^(−w9)·(e^(w10·(1−R)) − 1)·w15[Hard]·w16[Easy])`.
    fn recall_stability(&self, s: f64, d: f64, r: f64, grade: Grade) -> f64 {
        let w = &self.w;
        let hard_penalty = if grade == Grade::Hard { w[15] } else { 1.0 };
        let easy_bonus = if grade == Grade::Easy { w[16] } else { 1.0 };
        let growth = w[8].exp()
            * (11.0 - d)
            * s.powf(-w[9])
            * (w[10] * (1.0 - r)).exp_m1()
            * hard_penalty
            * easy_bonus;
        s * (1.0 + growth)
    }

    /// A lapse a day or more on (Again):
    /// `min(w11·D^(−w12)·((S+1)^w13 − 1)·e^(w14·(1−R)), S / e^(w17·w18))`.
    /// The second term holds a lapse under the prior stability.
    fn lapse_stability(&self, s: f64, d: f64, r: f64) -> f64 {
        let w = &self.w;
        let long_term =
            w[11] * d.powf(-w[12]) * ((s + 1.0).powf(w[13]) - 1.0) * (w[14] * (1.0 - r)).exp();
        let ceiling = s / (w[17] * w[18]).exp();
        long_term.min(ceiling)
    }

    /// A review less than a day on: `S·e^(w17·(G−3+w18))·S^(−w19)`, where
    /// Hard, Good, and Easy never lower it; only Again can.
    fn same_day_stability(&self, s: f64, grade: Grade) -> f64 {
        let w = &self.w;
        let mut growth = (w[17] * (grade.g() - 3.0 + w[18])).exp() * s.powf(-w[19]);
        if grade >= Grade::Hard {
            growth = growth.max(1.0);
        }
        s * growth
    }
}

/// Wall-clock days from `from_ms` to `to_ms`; 0 when the clock stepped back.
pub fn elapsed_days(from_ms: u64, to_ms: u64) -> f64 {
    to_ms.saturating_sub(from_ms) as f64 / MS_PER_DAY
}

fn clamp_stability(s: f64) -> f64 {
    s.clamp(S_MIN, S_MAX)
}

fn clamp_difficulty(d: f64) -> f64 {
    d.clamp(D_MIN, D_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::access::{Durability, Label, Outcome};
    use proptest::prelude::*;

    const DAY: u64 = 86_400_000;

    fn at(day: f64) -> u64 {
        (day * MS_PER_DAY) as u64
    }

    fn ev(day: f64, access: Access) -> AccessEvent {
        AccessEvent {
            at_ms: at(day),
            access,
        }
    }

    fn used(day: f64, outcome: Outcome) -> AccessEvent {
        ev(day, Access::Used(outcome))
    }

    /// Equal to 1e-9, relative: the oracle's libm and ours may differ in an
    /// ulp or two.
    fn close(got: f64, want: f64) -> bool {
        (got - want).abs() <= 1e-9 * want.abs().max(1.0)
    }

    #[test]
    fn retrievability_is_ninety_percent_after_one_stability() {
        let f = Fsrs6::default();
        for s in [S_MIN, 0.212, 1.0, 2.3065, 13.8, 365.0, S_MAX] {
            let r = f.retrievability(s, s);
            assert!((r - 0.9).abs() < 1e-12, "R(S, S) = {r} for S = {s}");
        }
    }

    #[test]
    fn retrievability_falls_with_time() {
        let f = Fsrs6::default();
        assert_eq!(f.retrievability(0.0, 2.3065), 1.0);
        let mut last = 1.0;
        for day in 1..=1000 {
            let r = f.retrievability(f64::from(day), 2.3065);
            assert!(r < last, "R did not fall at day {day}: {r} after {last}");
            last = r;
        }
        assert!(last > 0.0);
        // A clock that stepped back reads as no time at all.
        assert_eq!(f.retrievability(-3.0, 2.3065), 1.0);
        assert_eq!(f.retrievability(f64::NAN, 2.3065), 1.0);
    }

    #[test]
    fn good_raises_stability_and_again_lowers_it() {
        let f = Fsrs6::default();
        for first in [Grade::Again, Grade::Hard, Grade::Good, Grade::Easy] {
            let prior = f.initial(first, 0);
            for days in [1, 2, 7, 30, 400] {
                let good = f.review(&prior, Grade::Good, days * DAY);
                let again = f.review(&prior, Grade::Again, days * DAY);
                assert!(
                    good.stability > prior.stability,
                    "Good after {days} days from {first:?}"
                );
                assert!(
                    again.stability < prior.stability,
                    "Again after {days} days from {first:?}"
                );
            }
            // The same day: Good never lowers it, and Again still does.
            let good = f.review(&prior, Grade::Good, DAY / 4);
            let again = f.review(&prior, Grade::Again, DAY / 4);
            assert!(good.stability >= prior.stability);
            assert!(again.stability < prior.stability);
        }
    }

    #[test]
    fn grades_order_stability_and_difficulty() {
        let f = Fsrs6::default();
        let prior = f.initial(Grade::Good, 0);
        let after: Vec<Retention> = [Grade::Again, Grade::Hard, Grade::Good, Grade::Easy]
            .iter()
            .map(|&g| f.review(&prior, g, 5 * DAY))
            .collect();
        for pair in after.windows(2) {
            assert!(pair[0].stability < pair[1].stability);
            assert!(pair[0].difficulty >= pair[1].difficulty);
        }
    }

    #[test]
    fn difficulty_stays_in_one_to_ten() {
        let f = Fsrs6::default();
        // Easy's first difficulty is below 1 before the clamp (−4.77).
        assert_eq!(f.initial(Grade::Easy, 0).difficulty, D_MIN);
        let mut r = f.initial(Grade::Again, 0);
        for i in 1..=50 {
            r = f.review(&r, Grade::Again, i * DAY);
            assert!((D_MIN..=D_MAX).contains(&r.difficulty));
        }
        assert!(
            r.difficulty > 9.9,
            "fifty lapses leave D at {}",
            r.difficulty
        );
        for i in 51..=100 {
            r = f.review(&r, Grade::Easy, i * 3 * DAY);
            assert!((D_MIN..=D_MAX).contains(&r.difficulty));
        }
        assert!(r.difficulty < 1.1, "fifty Easy leave D at {}", r.difficulty);
    }

    /// The golden table, computed apart from this code: by hand for the first
    /// steps, and to full precision by the oracle `fsrs6_golden.py` in the
    /// math lane's report, written from the published equations and corrected
    /// against the reference (theseus-3ht).
    #[test]
    fn golden_table() {
        let f = Fsrs6::default();
        assert!(close(f.factor(), 0.98034649441348));
        for (t, s, want) in [
            (1.0, 2.3065, 0.946847499382546),
            (10.0, 2.3065, 0.774366916761404),
            (100.0, 1.0, 0.492322425582939),
            (365.0, 30.0, 0.673913572424319),
        ] {
            assert!(close(f.retrievability(t, s), want), "R({t}, {s})");
        }
        for (grade, s, d) in [
            (Grade::Again, 0.212, 6.4133),
            (Grade::Hard, 1.2931, 5.11217070560105),
            (Grade::Good, 2.3065, 2.11810397045901),
            (Grade::Easy, 8.2956, 1.0),
        ] {
            let r = f.initial(grade, 0);
            assert!(
                close(r.stability, s) && close(r.difficulty, d),
                "first {grade:?}"
            );
        }
        use Grade::*;
        let sequences: [&[(f64, Grade, f64, f64)]; 3] = [
            &[
                (0.0, Good, 2.3065, 2.11810397045901),
                (3.0, Good, 13.8269036943546, 2.11121423578539),
                (10.0, Hard, 29.0712345519362, 4.74828476159457),
                (30.0, Again, 2.21241917709689, 8.25902528209659),
                (30.5, Good, 2.21241917709689, 8.24599462611133),
                (40.0, Easy, 14.9449162147507, 7.645116136105),
            ],
            &[
                (0.0, Again, 0.212, 6.4133),
                (0.25, Again, 0.083356717110316, 8.80630446885684),
                (1.25, Hard, 0.402452254004551, 9.19279764951225),
                (5.25, Easy, 3.2515267195757, 8.90829660890562),
                // A same-day Hard leaves S as it was (theseus-3ht).
                (5.5, Hard, 3.2515267195757, 9.26050478491036),
            ],
            &[
                (0.0, Easy, 8.2956, 1.0),
                (20.0, Again, 1.56635828798406, 7.02698956929684),
                (21.0, Again, 0.416838525745023, 9.00802005719564),
            ],
        ];
        for (n, seq) in sequences.iter().enumerate() {
            let mut r: Option<Retention> = None;
            for &(day, grade, s, d) in seq.iter() {
                let next = match r {
                    None => f.initial(grade, at(day)),
                    Some(p) => f.review(&p, grade, at(day)),
                };
                assert!(
                    close(next.stability, s) && close(next.difficulty, d),
                    "sequence {n}, day {day}, {grade:?}: got S {} D {}, want S {s} D {d}",
                    next.stability,
                    next.difficulty
                );
                r = Some(next);
            }
        }
    }

    #[test]
    fn exposure_without_use_changes_nothing() {
        let f = Fsrs6::default();
        assert_eq!(f.step(None, &ev(0.0, Access::Shown)), None);
        let prior = f.initial(Grade::Good, 0);
        assert_eq!(f.step(Some(prior), &ev(9.0, Access::Shown)), Some(prior));
        // Shown many times over a year, then used: as if never shown.
        let mut shown = vec![used(0.0, Outcome::Ok)];
        shown.extend((1..=300).map(|d| ev(f64::from(d), Access::Shown)));
        shown.push(used(301.0, Outcome::Ok));
        assert_eq!(
            f.fold(&shown),
            f.fold(&[used(0.0, Outcome::Ok), used(301.0, Outcome::Ok)])
        );
    }

    #[test]
    fn first_sight_only_starts_a_node() {
        let f = Fsrs6::default();
        let sight = ev(0.0, Access::FirstSight(Durability::High));
        assert_eq!(f.step(None, &sight), Some(f.initial(Grade::Easy, 0)));
        let prior = f.initial(Grade::Hard, 0);
        let again = ev(4.0, Access::FirstSight(Durability::Floor));
        assert_eq!(f.step(Some(prior), &again), Some(prior));
    }

    #[test]
    fn labels_and_outcomes_review_by_the_table() {
        let f = Fsrs6::default();
        let prior = f.initial(Grade::Good, 0);
        let cases = [
            (Access::Used(Outcome::Ok), Grade::Good),
            (Access::Used(Outcome::Unknown), Grade::Hard),
            (Access::Used(Outcome::Corrected), Grade::Again),
            (Access::Labeled(Label::Wrong), Grade::Again),
            (Access::Labeled(Label::Stale), Grade::Again),
            (Access::Labeled(Label::Useful), Grade::Easy),
            (Access::Labeled(Label::Remember), Grade::Easy),
        ];
        for (access, grade) in cases {
            let got = f.step(
                Some(prior),
                &AccessEvent {
                    at_ms: 6 * DAY,
                    access,
                },
            );
            assert_eq!(got, Some(f.review(&prior, grade, 6 * DAY)), "{access:?}");
        }
    }

    #[test]
    fn the_clock_never_moves_a_review_back() {
        let f = Fsrs6::default();
        let prior = f.initial(Grade::Good, 10 * DAY);
        let back = f.review(&prior, Grade::Good, 3 * DAY);
        assert_eq!(back.last_review_ms, 10 * DAY);
        // A step back is no time at all: the same-day step.
        assert_eq!(back, f.review(&prior, Grade::Good, 10 * DAY));
    }

    #[test]
    fn parameters_are_checked() {
        assert!(Fsrs6::new(FSRS6_DEFAULT).is_ok());
        for (index, value) in [(9, f64::NAN), (17, f64::INFINITY), (20, f64::NEG_INFINITY)] {
            let mut w = FSRS6_DEFAULT;
            w[index] = value;
            assert_eq!(Fsrs6::new(w).unwrap_err().index, index);
        }
        // A decay of 0 and a negative first stability are clipped, as the
        // reference clips them, not refused (theseus-3ht).
        let mut w = FSRS6_DEFAULT;
        w[20] = 0.0;
        w[2] = -1.0;
        let f = Fsrs6::new(w).unwrap();
        assert_eq!((f.decay(), f.params()[2]), (0.1, 0.001));
    }

    /// `new` clips every parameter into the reference's bounds, as its
    /// `FSRS::new` does (theseus-3ht), so that the same parameters make the
    /// same model. The defaults sit inside every bound.
    #[test]
    fn parameters_are_clipped_into_the_references_bounds() {
        // The reference's table for `FSRS::new` (fsrs 6.6.2,
        // src/parameter_clipper.rs:62-84): one relearning step, so w17 and
        // w18 are at most 2, and no short-term floor for w19.
        let lows = [
            0.001, 0.001, 0.001, 0.001, 1.0, 0.001, 0.001, 0.001, 0.0, 0.0, 0.001, 0.001, 0.001,
            0.001, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.1,
        ];
        let highs = [
            100.0, 100.0, 100.0, 100.0, 10.0, 4.0, 4.0, 0.75, 4.5, 0.8, 3.5, 5.0, 0.25, 0.9, 4.0,
            1.0, 6.0, 2.0, 2.0, 0.8, 0.8,
        ];
        assert_eq!(Fsrs6::new([-1.0; 21]).unwrap().params(), &lows);
        assert_eq!(Fsrs6::new([1e6; 21]).unwrap().params(), &highs);
        assert_eq!(Fsrs6::new(FSRS6_DEFAULT).unwrap().params(), &FSRS6_DEFAULT);
        assert_eq!(Fsrs6::new(FSRS6_DEFAULT).unwrap(), Fsrs6::default());
    }

    /// A same-day Hard never lowers stability: the reference floors the
    /// same-day factor at 1 for Hard, Good, and Easy, in its model's step,
    /// its tensor model, and its optimizer (theseus-3ht). With the defaults,
    /// Hard's factor before the floor is under 1 at every S, so S stays.
    #[test]
    fn same_day_hard_never_lowers_stability() {
        let f = Fsrs6::default();
        for first in [Grade::Again, Grade::Hard, Grade::Good, Grade::Easy] {
            let prior = f.initial(first, 0);
            let hard = f.review(&prior, Grade::Hard, DAY / 4);
            assert_eq!(
                hard.stability, prior.stability,
                "a same-day Hard after {first:?}"
            );
        }
        for s in [S_MIN, 0.5, 1.0, 2.3065, 30.0, 365.0, S_MAX] {
            let prior = Retention {
                stability: s,
                difficulty: 5.0,
                last_review_ms: 0,
            };
            let hard = f.review(&prior, Grade::Hard, DAY / 2);
            assert_eq!(hard.stability, s, "a same-day Hard at S = {s}");
        }
    }

    /// A review first clamps the prior's S and D into their bounds, as the
    /// reference's step does (theseus-3ht): a state from elsewhere, out of
    /// bounds, reviews as its clamped self, and never yields NaN.
    #[test]
    fn a_review_clamps_the_prior_first() {
        let f = Fsrs6::default();
        let state = |stability, difficulty| Retention {
            stability,
            difficulty,
            last_review_ms: 0,
        };
        for (raw, clamped) in [
            (state(0.0, 5.0), state(S_MIN, 5.0)),
            (state(1e9, 5.0), state(S_MAX, 5.0)),
            (state(5.0, 12.0), state(5.0, D_MAX)),
            (state(5.0, -3.0), state(5.0, D_MIN)),
        ] {
            for grade in [Grade::Again, Grade::Hard, Grade::Good, Grade::Easy] {
                for at_ms in [DAY / 2, 5 * DAY] {
                    let got = f.review(&raw, grade, at_ms);
                    assert_eq!(got, f.review(&clamped, grade, at_ms), "{raw:?}, {grade:?}");
                    assert!(got.stability.is_finite() && got.difficulty.is_finite());
                }
            }
        }
    }

    /// The reference's own numbers: the cases its tests pin (fsrs 6.6.2,
    /// src/inference.rs), computed by its f32 code, agree with this f64 code
    /// to f32's rounding. None reviews Hard on the same day.
    #[test]
    #[expect(clippy::too_many_lines, reason = "shape budget: split it")]
    fn matches_the_reference_crates_own_numbers() {
        use Grade::*;
        // Relative: f32's rounding over a few steps comes to 7.9e-7 at most.
        fn near(got: f64, want: f64) -> bool {
            (got - want).abs() <= 2e-6 * want.abs().max(1.0)
        }
        // A history from a new card, each review as (grade, whole days since
        // the review before it), as the reference's tests give it.
        fn walk(f: &Fsrs6, history: &[(Grade, u64)]) -> Retention {
            let mut now = 0;
            let mut r: Option<Retention> = None;
            for &(grade, days) in history {
                now += days * DAY;
                r = Some(match r {
                    None => f.initial(grade, now),
                    Some(p) => f.review(&p, grade, now),
                });
            }
            r.expect("a history of at least one review")
        }
        fn check(label: &str, got: Retention, s: f64, d: f64) {
            assert!(
                near(got.stability, s) && near(got.difficulty, d),
                "{label}: got S {} D {}, want S {s} D {d}",
                got.stability,
                got.difficulty
            );
        }

        // test_current_retrievability (inference.rs:1648-1658): S = 1, decay 0.2.
        let mut w = FSRS6_DEFAULT;
        w[20] = 0.2;
        let f = Fsrs6::new(w).unwrap();
        for (t, want) in [(0.0, 1.0), (1.0, 0.9), (2.0, 0.84028935), (3.0, 0.7985001)] {
            assert!(near(f.retrievability(t, 1.0), want), "R({t}, 1)");
        }

        // next_states for a new card, the doc example (inference.rs:343-351).
        let f = Fsrs6::default();
        for (grade, s, d) in [
            (Again, 0.212, 6.4133),
            (Hard, 1.2931, 5.1121707),
            (Good, 2.3065, 2.118104),
            (Easy, 8.2956, 1.0),
        ] {
            check("a new card", f.initial(grade, 0), s, d);
        }

        // test_memory_state (inference.rs:992-1040), and the same with the
        // same-day step frozen (w17 to w19 at 0).
        let history = [
            (Again, 0),
            (Good, 0),
            (Good, 1),
            (Good, 3),
            (Good, 8),
            (Good, 21),
        ];
        check("the defaults", walk(&f, &history), 53.62691, 6.3574867);
        let mut w = FSRS6_DEFAULT;
        w[17..20].fill(0.0);
        let frozen = Fsrs6::new(w).unwrap();
        check(
            "no same-day step",
            walk(&frozen, &history),
            53.335106,
            6.3574867,
        );

        // A 19-parameter (FSRS-5) set (inference.rs:900-920), filled to 21
        // as the reference fills it: w19 = 0 and w20 = 0.5 (model.rs:346-349).
        let f = Fsrs6::new([
            0.6845422,
            1.6790825,
            4.7349424,
            10.042885,
            7.4410233,
            0.64219797,
            1.071918,
            0.0025195254,
            1.432437,
            0.1544,
            0.8692766,
            2.0696752,
            0.0953,
            0.2975,
            2.4691248,
            0.19542035,
            3.201072,
            0.18046261,
            0.121442534,
            0.0,
            0.5,
        ])
        .unwrap();
        // test_memo_state (inference.rs:937-990).
        let history = [(Again, 0), (Good, 1), (Good, 3), (Good, 8), (Good, 21)];
        check("memo", walk(&f, &history), 31.722992, 7.382128);
        let prior = Retention {
            stability: 20.925528,
            difficulty: 7.005062,
            last_review_ms: 0,
        };
        check(
            "memo, Good",
            f.review(&prior, Good, 21 * DAY),
            40.87456,
            6.9913807,
        );
        // test_next_states (inference.rs:1484-1542).
        let prior = walk(&f, &history[..4]);
        for (grade, s, d) in [
            (Again, 2.9691455, 8.000659),
            (Hard, 17.091452, 7.6913934),
            (Good, 31.722992, 7.382128),
            (Easy, 71.7502, 7.0728626),
        ] {
            let at_ms = prior.last_review_ms + 21 * DAY;
            check("next states", f.review(&prior, grade, at_ms), s, d);
        }
    }

    fn any_access() -> impl Strategy<Value = Access> {
        prop_oneof![
            Just(Access::Shown),
            Just(Access::Used(Outcome::Ok)),
            Just(Access::Used(Outcome::Unknown)),
            Just(Access::Used(Outcome::Corrected)),
            Just(Access::Labeled(Label::Useful)),
            Just(Access::Labeled(Label::Remember)),
            Just(Access::Labeled(Label::Wrong)),
            Just(Access::Labeled(Label::Stale)),
            Just(Access::FirstSight(Durability::High)),
            Just(Access::FirstSight(Durability::Medium)),
            Just(Access::FirstSight(Durability::Low)),
            Just(Access::FirstSight(Durability::Floor)),
        ]
    }

    /// Events in position order: gaps from none to a few years, in ms, and a
    /// clock that sometimes steps back.
    fn any_history() -> impl Strategy<Value = Vec<AccessEvent>> {
        proptest::collection::vec((any_access(), 0u64..(3 * 365 * DAY), any::<bool>()), 0..40)
            .prop_map(|raw| {
                let mut now = 1_700_000_000_000u64;
                raw.into_iter()
                    .map(|(access, gap, back)| {
                        now = if back {
                            now.saturating_sub(gap / 100)
                        } else {
                            now + gap
                        };
                        AccessEvent { at_ms: now, access }
                    })
                    .collect()
            })
    }

    proptest! {
        #![proptest_config(ProptestConfig {
            cases: 2000,
            failure_persistence: None,
            ..ProptestConfig::default()
        })]

        /// R is in [0, 1], 1 at no time, and never rises with time.
        #[test]
        fn retrievability_is_a_falling_chance(
            s in S_MIN..S_MAX,
            t1 in 0.0f64..1e6,
            dt in 0.0f64..1e6,
        ) {
            let f = Fsrs6::default();
            let (r1, r2) = (f.retrievability(t1, s), f.retrievability(t1 + dt, s));
            prop_assert!((0.0..=1.0).contains(&r1) && (0.0..=1.0).contains(&r2));
            prop_assert_eq!(f.retrievability(0.0, s), 1.0);
            // An ulp of slack: libm's pow is faithful, not correctly rounded.
            prop_assert!(r2 <= r1 + 1e-15, "R({}) = {} rose to R({}) = {}", t1, r1, t1 + dt, r2);
        }

        /// Any history keeps S and D in their bounds, and R in [0, 1].
        #[test]
        fn any_history_stays_in_bounds(events in any_history()) {
            let f = Fsrs6::default();
            let mut r = None;
            for e in &events {
                r = f.step(r, e);
                if let Some(x) = r {
                    prop_assert!((S_MIN..=S_MAX).contains(&x.stability), "S = {}", x.stability);
                    prop_assert!((D_MIN..=D_MAX).contains(&x.difficulty), "D = {}", x.difficulty);
                    let now = x.last_review_ms + 30 * DAY;
                    prop_assert!((0.0..=1.0).contains(&f.retrievability_at(&x, now)));
                }
            }
        }

        /// Rebuilt from every event, the fold equals the one kept up to date
        /// event by event, wherever the rebuild resumes.
        #[test]
        fn rebuilt_equals_incremental(events in any_history(), cut in 0usize..40) {
            let f = Fsrs6::default();
            let cut = cut.min(events.len());
            let (head, tail) = events.split_at(cut);
            let kept = tail.iter().fold(f.fold(head), |prior, e| f.step(prior, e));
            prop_assert_eq!(f.fold(&events), kept);
        }

        /// Shown, not used, anywhere in a history: the same retention.
        #[test]
        fn exposure_anywhere_changes_nothing(events in any_history(), spots in proptest::collection::vec(0usize..40, 0..10)) {
            let f = Fsrs6::default();
            let mut with = events.clone();
            for i in spots {
                let i = i.min(with.len());
                let at_ms = with.get(i).map_or(0, |e| e.at_ms);
                with.insert(i, AccessEvent { at_ms, access: Access::Shown });
            }
            prop_assert_eq!(f.fold(&with), f.fold(&events));
        }
    }
}
