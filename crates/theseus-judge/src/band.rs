//! The three-band gate (design §2.3), per question, with the pack's own
//! thresholds. Act above the act threshold, confirm in the middle, escalate
//! below. In M5 a live action acts only in the act band; confirm and escalate
//! defer to the baseline, and the band is ledgered either way.
//!
//! - Choice and Score: the top answer's `confidence` c. Act if c ≥ act;
//!   confirm if c ≥ confirm; else escalate.
//! - Noul: p, the probability the statement is true. Act-true if p ≥ act,
//!   act-false if p ≤ 1 − act, confirm if p ≥ confirm or p ≤ 1 − confirm,
//!   else escalate. Noul and Choice thresholds are separate, as Jev's
//!   documentation warns.
//!
//! An edge counts as inside its band: c = act acts. The comparisons allow
//! for float rounding (1e-9), so a Noul of 0.1 against an act of 0.9 is
//! act-false although 1 − 0.9 is 0.09999999999999998 in f64.

use serde::{Deserialize, Serialize};

use crate::client::Answer;

const EPS: f64 = 1e-9;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Band {
    Act,
    Confirm,
    Escalate,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Thresholds {
    pub act: f64,
    pub confirm: f64,
}

impl Thresholds {
    /// Where every pack starts (§3.7: conservative until labels say more).
    pub const CONSERVATIVE: Thresholds = Thresholds {
        act: 0.90,
        confirm: 0.60,
    };
}

/// What an answer leans to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Top {
    /// A Choice's chosen option.
    Choice(String),
    /// A Score's most probable level, 0-based.
    Level(usize),
    /// Which way a Noul leans (p ≥ 0.5 is true).
    Noul(bool),
}

/// One answer's band, what it leans to, and the number that put it there
/// (the confidence, or a Noul's p).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Banded {
    pub band: Band,
    pub top: Top,
    pub value: f64,
}

impl Banded {
    pub fn is_act(&self) -> bool {
        self.band == Band::Act
    }

    /// A Choice in the act band on this option.
    pub fn acts_on(&self, option: &str) -> bool {
        self.is_act() && self.top == Top::Choice(option.to_string())
    }

    /// A Noul in the act band, true.
    pub fn acts_true(&self) -> bool {
        self.is_act() && self.top == Top::Noul(true)
    }

    /// A Noul in the act band, false.
    pub fn acts_false(&self) -> bool {
        self.is_act() && self.top == Top::Noul(false)
    }
}

pub(crate) fn at_least(x: f64, threshold: f64) -> bool {
    x >= threshold - EPS
}

/// The band of a Choice's or a Score's confidence.
pub fn confidence_band(c: f64, t: Thresholds) -> Band {
    if at_least(c, t.act) {
        Band::Act
    } else if at_least(c, t.confirm) {
        Band::Confirm
    } else {
        Band::Escalate
    }
}

/// The band of a Noul's p, and which way it leans.
pub fn noul_band(p: f64, t: Thresholds) -> (Band, bool) {
    if at_least(p, t.act) {
        (Band::Act, true)
    } else if at_least(1.0 - p, t.act) {
        (Band::Act, false)
    } else if at_least(p, t.confirm) {
        (Band::Confirm, true)
    } else if at_least(1.0 - p, t.confirm) {
        (Band::Confirm, false)
    } else {
        (Band::Escalate, p >= 0.5)
    }
}

/// One answer's band under its question's thresholds.
pub fn band(answer: &Answer, t: Thresholds) -> Banded {
    match answer {
        Answer::Choice {
            choice, confidence, ..
        } => Banded {
            band: confidence_band(*confidence, t),
            top: Top::Choice(choice.clone()),
            value: *confidence,
        },
        Answer::Score { confidence, .. } => Banded {
            band: confidence_band(*confidence, t),
            top: Top::Level(answer.score_level().unwrap_or(0)),
            value: *confidence,
        },
        Answer::Noul { noul } => {
            let (band, lean) = noul_band(*noul, t);
            Banded {
                band,
                top: Top::Noul(lean),
                value: *noul,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: Thresholds = Thresholds::CONSERVATIVE;

    fn choice(c: f64) -> Answer {
        Answer::Choice {
            choice: "complete".into(),
            probabilities: vec![("complete".into(), c), ("other".into(), 1.0 - c)],
            confidence: c,
        }
    }

    #[test]
    fn a_choice_acts_at_its_threshold_and_not_just_below() {
        assert_eq!(band(&choice(0.90), T).band, Band::Act);
        assert_eq!(band(&choice(1.0), T).band, Band::Act);
        assert_eq!(band(&choice(0.8999), T).band, Band::Confirm);
        assert_eq!(band(&choice(0.60), T).band, Band::Confirm);
        assert_eq!(band(&choice(0.5999), T).band, Band::Escalate);
        assert_eq!(band(&choice(0.0), T).band, Band::Escalate);
        let b = band(&choice(0.93), T);
        assert!(b.acts_on("complete"));
        assert!(!b.acts_on("other"));
        assert_eq!(b.value, 0.93);
    }

    #[test]
    fn a_score_bands_on_its_confidence_and_leans_to_its_likeliest_level() {
        let a = Answer::Score {
            score: 1.81,
            probabilities: vec![0.01, 0.17, 0.82, 0.0],
            confidence: 0.81,
        };
        let b = band(&a, T);
        assert_eq!(b.band, Band::Confirm);
        assert_eq!(b.top, Top::Level(2));
        let sure = Answer::Score {
            score: 0.0,
            probabilities: vec![1.0, 0.0],
            confidence: 0.9,
        };
        assert_eq!(band(&sure, T).band, Band::Act);
        assert_eq!(band(&sure, T).top, Top::Level(0));
    }

    #[test]
    fn a_noul_acts_true_and_false_at_both_edges() {
        let n = |p: f64| band(&Answer::Noul { noul: p }, T);
        // Act-true at p = act, act-false at p = 1 − act (0.1, despite f64).
        assert!(n(0.90).acts_true());
        assert!(n(1.0).acts_true());
        assert!(n(0.10).acts_false());
        assert!(n(0.0).acts_false());
        // Just inside the edges, confirm.
        assert_eq!(n(0.8999).band, Band::Confirm);
        assert_eq!(n(0.8999).top, Top::Noul(true));
        assert_eq!(n(0.1001).band, Band::Confirm);
        assert_eq!(n(0.1001).top, Top::Noul(false));
        // The confirm band's own edges: p = 0.6 and p = 0.4 confirm.
        assert_eq!(n(0.60).band, Band::Confirm);
        assert_eq!(n(0.40).band, Band::Confirm);
        assert_eq!(n(0.40).top, Top::Noul(false));
        // Strictly between 0.4 and 0.6, escalate.
        assert_eq!(n(0.5999).band, Band::Escalate);
        assert_eq!(n(0.4001).band, Band::Escalate);
        assert_eq!(n(0.5).band, Band::Escalate);
        assert_eq!(n(0.5).top, Top::Noul(true));
    }

    #[test]
    fn noul_and_choice_thresholds_are_separate() {
        // The same 0.7 acts for a question tuned to act at 0.7, and only
        // confirms for one at the conservative start.
        let loose = Thresholds {
            act: 0.70,
            confirm: 0.55,
        };
        assert!(band(&Answer::Noul { noul: 0.7 }, loose).acts_true());
        assert_eq!(band(&Answer::Noul { noul: 0.7 }, T).band, Band::Confirm);
        assert_eq!(band(&choice(0.7), loose).band, Band::Act);
        assert_eq!(band(&choice(0.7), T).band, Band::Confirm);
    }
}
