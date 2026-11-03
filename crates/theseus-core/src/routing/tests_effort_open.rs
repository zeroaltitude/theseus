//! route.v10's effort rule (theseus-h66v4 row 3, design §1.4 step 4): Jev's
//! answer is the effort, with no bounds, no high-effort gate, no floor and no
//! fixed profile; an answer under the bar is the pack's `no_match`, `high`, for
//! every model (theseus-h66v4.6.1).

use super::effort::*;
use crate::config::Effort;

fn answer(level: &str, confidence: f64, sure: bool) -> EffortAnswer {
    EffortAnswer {
        level: level.into(),
        confidence,
        sure,
    }
}

fn runs(takes: bool) -> OpenRuns {
    OpenRuns {
        own: Some(Effort::Low),
        takes,
    }
}

#[test]
fn a_sure_level_is_the_effort_whatever_the_level() {
    for e in [
        Effort::Low,
        Effort::Medium,
        Effort::High,
        Effort::Xhigh,
        Effort::Max,
    ] {
        let d = decide_open(runs(true), &answer(name(e), 0.9, true), true, false);
        assert_eq!(
            (d.reason, d.ran, d.sent),
            (EffortReason::Applied, Some(e), None),
            "{e:?}"
        );
        assert!(d.applied());
    }
}

#[test]
fn xhigh_and_max_need_no_confidence_beyond_the_bar() {
    // v9's gate held an `xhigh` at or under `high_effort_confidence` and an unsure one always;
    // route.v10 has no gate: the answer's own band is the only bar.
    for (level, confidence) in [("xhigh", 0.61), ("max", 0.61), ("max", 0.99)] {
        let d = decide_open(runs(true), &answer(level, confidence, true), true, false);
        assert_eq!(d.reason, EffortReason::Applied, "{level} at {confidence}");
        assert_eq!(d.ran, parse(level));
    }
}

#[test]
fn an_answer_under_the_bar_runs_the_packs_no_match_not_the_models_default() {
    // Under the confirm bar: the level is not taken, `high` runs, whatever the model.
    for level in ["max", "low", "medium"] {
        let d = decide_open(runs(true), &answer(level, 0.30, false), true, false);
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::NoMatch, Some(Effort::High)),
            "{level}"
        );
        assert!(d.applied(), "the request carries it, not the profile's own");
    }
    // A model that takes no effort carries none, sure or not.
    let d = decide_open(runs(false), &answer("max", 0.30, false), true, false);
    assert_eq!((d.reason, d.ran), (EffortReason::NoEffort, None));
}

/// One value in two places: the daemon's `NO_MATCH` is the pack's `no_match`, so the replay's
/// grade and the daemon's act cannot part again.
#[test]
fn the_daemons_no_match_is_the_packs() {
    let pack = theseus_judge::pack::by_name("effort.v1").unwrap();
    let want = pack.question("effort").unwrap().no_match.clone().unwrap();
    assert_eq!(NO_MATCH, parse(&want).unwrap());
}

#[test]
fn a_model_that_takes_no_effort_carries_none() {
    let d = decide_open(runs(false), &answer("high", 0.95, true), true, false);
    assert_eq!((d.reason, d.ran), (EffortReason::NoEffort, None));
    assert!(!d.applied());
}

#[test]
fn a_pinned_or_late_turn_stands_on_the_profiles_own_effort() {
    let d = decide_open(runs(true), &answer("max", 0.95, true), false, false);
    assert_eq!(
        (d.reason, d.ran),
        (EffortReason::Recorded, Some(Effort::Low))
    );
    let d = decide_open(runs(true), &answer("max", 0.95, true), true, true);
    assert_eq!(
        (d.reason, d.ran),
        (EffortReason::Carried, Some(Effort::Low))
    );
    assert!(!d.applied());
}

#[test]
fn the_new_reason_has_its_word() {
    assert_eq!(EffortReason::NoMatch.as_str(), "no_match");
    assert_eq!(OPEN_QUESTION, "effort");
}
