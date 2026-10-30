//! route.v10's effort rule (theseus-h66v4 row 3, design §1.4 step 4): Jev's
//! answer is the effort, with no bounds, no high-effort gate, no floor and no
//! fixed profile; `default` or an answer under the bar is the model's own
//! default effort.

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
        default: Some(Effort::High),
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
fn default_and_an_answer_under_the_bar_run_the_models_own_default() {
    let d = decide_open(runs(true), &answer("default", 0.95, true), true, false);
    assert_eq!(
        (d.reason, d.ran),
        (EffortReason::ModelDefault, Some(Effort::High))
    );
    assert!(
        d.applied(),
        "the request carries the model's default, not the profile's own"
    );
    // Under the confirm bar: the level is not taken, the default runs.
    let d = decide_open(runs(true), &answer("max", 0.30, false), true, false);
    assert_eq!(
        (d.reason, d.ran),
        (EffortReason::ModelDefault, Some(Effort::High))
    );
    // No default in the catalog: the request carries none (the model's own).
    let none = OpenRuns {
        default: None,
        ..runs(true)
    };
    let d = decide_open(none, &answer("default", 0.95, true), true, false);
    assert_eq!((d.reason, d.ran), (EffortReason::ModelDefault, None));
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
    assert_eq!(EffortReason::ModelDefault.as_str(), "model_default");
    assert_eq!(OPEN_QUESTION, "effort");
}
