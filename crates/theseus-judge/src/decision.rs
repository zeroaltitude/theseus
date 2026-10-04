//! What a pack's deciding questions say together (design §2.4, §2.7).
//!
//! A pack names its deciding questions with `decides = true`. A deciding
//! Noul speaks when it leans true past its bar: its `confirm` threshold, or
//! its own `decide_above` where the pack sets one (`security.v3`'s `steered`
//! decides above 0.75, beside `risky` at 0.60). Past the bar the question
//! says *ask*; at its `act` threshold it says *act*, which a pack in shadow
//! (`action = "none"`) never does. The decision is the strongest of what the
//! deciding questions say, so any one of them passing its bar is enough.
//!
//! A pack with no `decide_above` reads exactly as the band does: a Noul
//! decides to ask in the band's confirm-or-act band, leaning true. Only
//! whole Nouls vote; a deciding Choice or Score is read by its pack's own
//! action, not here.

use serde::{Deserialize, Serialize};

use crate::band::{at_least, Band, Top};
use crate::client::Kind;
use crate::judge::AnswerRecord;
use crate::pack::Pack;

/// What the deciding questions say, weakest first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// No deciding question passed its bar (or none was answered).
    Quiet,
    /// One passed its bar: the operator is asked.
    Ask,
    /// One reached its act threshold.
    Act,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub verdict: Verdict,
    /// The question that made it: the strongest voter, by verdict and then
    /// by p, the pack's order breaking a tie. None when quiet.
    pub by: Option<String>,
    /// That question's p.
    pub value: Option<f64>,
}

/// The pack's decision on one judgment's answers.
pub fn decide(pack: &Pack, answers: &[AnswerRecord]) -> Decision {
    let mut best = Decision {
        verdict: Verdict::Quiet,
        by: None,
        value: None,
    };
    for q in pack
        .questions
        .iter()
        .filter(|q| q.decides && q.kind == Kind::Noul)
    {
        let Some(a) = answers.iter().find(|a| a.about.is_none() && a.def == q.id) else {
            continue;
        };
        let (p, leans_true) = (a.band.value, a.band.top == Top::Noul(true));
        if !leans_true {
            continue;
        }
        let bar = q.decide_above.unwrap_or(q.thresholds.confirm);
        let verdict = if a.band.band == Band::Act {
            Verdict::Act
        } else if at_least(p, bar) {
            Verdict::Ask
        } else {
            Verdict::Quiet
        };
        let stronger = verdict > best.verdict
            || (verdict == best.verdict
                && verdict != Verdict::Quiet
                && best.value.is_some_and(|v| p > v));
        if verdict != Verdict::Quiet && stronger {
            best = Decision {
                verdict,
                by: Some(q.id.clone()),
                value: Some(p),
            };
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::band::band;
    use crate::client::Answer;
    use crate::pack::by_name;

    fn rec(q: &str, p: f64, pack: &Pack) -> AnswerRecord {
        let t = pack.question(q).unwrap().thresholds;
        let answer = Answer::Noul { noul: p };
        AnswerRecord {
            question: q.into(),
            def: q.into(),
            about: None,
            band: band(&answer, t),
            answer,
        }
    }

    fn v3(risky: f64, steered: f64) -> Decision {
        let pack = by_name("security.v3").unwrap();
        decide(
            &pack,
            &[rec("risky", risky, &pack), rec("steered", steered, &pack)],
        )
    }

    #[test]
    fn steered_decides_at_its_bar_and_not_just_below() {
        let d = v3(0.1, 0.749);
        assert_eq!(d.verdict, Verdict::Quiet);
        assert_eq!(d.by, None);
        let d = v3(0.1, 0.75);
        assert_eq!(
            (d.verdict, d.by.as_deref()),
            (Verdict::Ask, Some("steered"))
        );
        assert_eq!(d.value, Some(0.75));
    }

    #[test]
    fn risky_alone_still_asks_at_its_confirm_line() {
        let d = v3(0.60, 0.1);
        assert_eq!((d.verdict, d.by.as_deref()), (Verdict::Ask, Some("risky")));
        assert_eq!(v3(0.5999, 0.1).verdict, Verdict::Quiet);
    }

    /// The finding that made the rule: risky 0.55 and steered 0.83.
    #[test]
    fn the_laundered_file_case_asks_by_steered() {
        let d = v3(0.55, 0.83);
        assert_eq!(
            (d.verdict, d.by.as_deref()),
            (Verdict::Ask, Some("steered"))
        );
    }

    #[test]
    fn when_both_pass_the_stronger_names_the_decision() {
        let d = v3(0.65, 0.80);
        assert_eq!(d.by.as_deref(), Some("steered"), "0.80 over 0.65");
        let d = v3(0.85, 0.80);
        assert_eq!(d.by.as_deref(), Some("risky"));
        // An act beats an ask, whatever the p.
        let d = v3(0.65, 0.95);
        assert_eq!(
            (d.verdict, d.by.as_deref()),
            (Verdict::Act, Some("steered"))
        );
        // A tie goes to the question the pack lists first (risky).
        assert_eq!(v3(0.80, 0.80).by.as_deref(), Some("risky"));
    }

    #[test]
    fn a_question_leaning_false_or_not_asked_says_nothing() {
        assert_eq!(v3(0.01, 0.01).verdict, Verdict::Quiet);
        let pack = by_name("security.v3").unwrap();
        assert_eq!(decide(&pack, &[]).verdict, Verdict::Quiet);
        // `safe` and the other undecided questions never vote.
        let d = decide(
            &pack,
            &[rec("safe", 0.99, &pack), rec("exfiltrates", 0.99, &pack)],
        );
        assert_eq!(d.verdict, Verdict::Quiet);
    }

    /// A pack with no `decide_above` reads as the band does: ask in the
    /// confirm band, act in the act band (security.v2, v1, and the rest).
    #[test]
    fn a_pack_without_decide_above_reads_as_its_confirm_line() {
        for name in ["security.v1", "security.v2"] {
            let pack = by_name(name).unwrap();
            let ask = |risky: f64, steered: f64| {
                decide(
                    &pack,
                    &[rec("risky", risky, &pack), rec("steered", steered, &pack)],
                )
            };
            assert_eq!(ask(0.60, 0.0).verdict, Verdict::Ask, "{name}");
            assert_eq!(ask(0.5999, 0.0).verdict, Verdict::Quiet, "{name}");
            assert_eq!(ask(0.90, 0.0).verdict, Verdict::Act, "{name}");
            // The case v2 missed: steered 0.83, risky 0.55 stays quiet.
            let d = ask(0.55, 0.83);
            assert_eq!(d.verdict, Verdict::Quiet, "{name}");
        }
    }
}
