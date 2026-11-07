//! route.v10's effort rule (theseus-h66v4 row 3, design §1.4 step 4): Jev's
//! answer is the effort, with no bounds, no high-effort gate, no floor and no
//! fixed profile; an answer under the bar is the question's own declared
//! `no_match`, `high`, for every model (theseus-h66v4.6.1), read from the pack
//! since theseus-ig13o rather than from a constant in `effort.rs`.

use super::effort::*;
use crate::config::Effort;

/// `effort.v1`'s own declared default for its `effort` question, read out of
/// the pack. Every answer this file builds carries it, as a live one does.
fn declared() -> String {
    theseus_judge::pack::by_name("effort.v1")
        .expect("effort.v1 is embedded")
        .question("effort")
        .expect("effort.v1 asks `effort`")
        .no_match
        .clone()
        .expect("effort.v1 declares a no_match")
}

fn answer(level: &str, confidence: f64, sure: bool) -> EffortAnswer {
    carrying(level, confidence, sure, Some(declared()))
}

fn carrying(level: &str, confidence: f64, sure: bool, no_match: Option<String>) -> EffortAnswer {
    EffortAnswer {
        level: level.into(),
        confidence,
        sure,
        no_match,
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

/// **The red test** (theseus-ig13o). A pack whose declared default differs
/// from the `high` that used to be hand-copied into `effort.rs`: the live path
/// has to run what the pack declares, whatever that is. Before the fix
/// `decide_open` read the constant and ran `high` here, so this failed with
/// `Some(High)` against `Some(Low)` for all three levels.
///
/// It is built the way a live answer is built — the pack is parsed, `ask`ed,
/// and the default carried off the `Asked` — so the whole carry is under test,
/// not just `decide_open`'s last line.
#[test]
fn a_planted_pack_default_is_what_the_live_path_runs() {
    let text = theseus_judge::pack::EMBEDDED
        .iter()
        .find(|(name, _)| *name == "effort.v1")
        .map(|(_, text)| *text)
        .expect("effort.v1 is embedded");
    assert!(
        text.contains("no_match = \"high\""),
        "the pack still declares high"
    );
    let planted =
        theseus_judge::Pack::parse(&text.replace("no_match = \"high\"", "no_match = \"low\""))
            .expect("the planted pack loads");
    let asked = planted.ask(&theseus_judge::pack::Dynamic::default());
    let carried = asked
        .iter()
        .find(|a| a.def == "effort")
        .and_then(|a| a.no_match.clone());
    assert_eq!(
        carried.as_deref(),
        Some("low"),
        "the ask carries the declared default"
    );
    for level in ["max", "high", "medium"] {
        let d = decide_open(
            runs(true),
            &carrying(level, 0.30, false, carried.clone()),
            true,
            false,
        );
        assert_eq!(
            (d.reason, d.ran),
            (EffortReason::NoMatch, Some(Effort::Low)),
            "{level}: the pack's declared default, not a constant in effort.rs"
        );
    }
    // And the shipped pack still says `high`, so nothing above changed what a
    // real turn runs.
    assert_eq!(declared(), "high");
}

/// A question that declares no default at all leaves nothing to run, and the
/// profile's own effort stands. `every_choice_question_declares_a_no_match`
/// holds that no pack of ours is in that state; this says what happens if one
/// ever were, rather than leaving it to a constant.
#[test]
fn an_answer_under_the_bar_with_no_declared_default_stands_on_the_profiles_own() {
    let d = decide_open(runs(true), &carrying("max", 0.30, false, None), true, false);
    // The profile's own `low`, not Jev's `max` and not a level written here.
    assert_eq!(
        (d.reason, d.ran, d.sent),
        (EffortReason::NoMatch, Some(Effort::Low), None)
    );
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
