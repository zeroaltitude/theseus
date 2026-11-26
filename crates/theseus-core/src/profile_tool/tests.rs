//! The wall and the gate of `profile.ask` and `profile.hold`, on the facts alone. The
//! played turns, through the whole core, are `config_change/tests_profile.rs`.

use super::*;
use crate::config_change::wall;

fn started_from_a_file() -> crate::Config {
    let mut cfg = crate::Config::example();
    cfg.config_file = Some("/etc/theseus/theseus.toml".into());
    cfg
}

fn plan() -> Plan {
    Plan {
        summary: "profile".into(),
        ..Default::default()
    }
}

/// A clean turn is told and not asked; one that read text from outside, or was steered, is
/// asked, each in its own words; and only the profile tools are touched.
#[test]
fn a_profile_call_is_free_when_clean_and_asked_after_outside_text() {
    use crate::config_change::tests_hold::{each_cause_is_said, ways};
    for tool in NAMES {
        each_cause_is_said(tool, |o, s, way| held(tool, o, s, way, plan()).authority);
    }
    for other in ["config.show", "proc.run", "profile.other", "look.theseus"] {
        assert!(
            held(other, true, true, &ways()[1], plan())
                .authority
                .is_none(),
            "{other}"
        );
    }
}

/// Every refusal of the wall is said in the tool's own name, and none of them says it
/// changes a setting.
#[test]
fn every_refusal_is_in_the_tools_own_words() {
    let walls = [
        wall::NOT_PRIVATE,
        wall::NOT_ALONE,
        wall::NOT_A_TASK,
        wall::NOT_THE_OWNER,
    ];
    for tool in NAMES {
        for w in walls {
            let said = worded(Some(w), tool).unwrap();
            assert!(said.starts_with(tool), "{said}");
            assert!(!said.contains("config.propose"), "{said}");
        }
        assert_eq!(worded(None, tool), None);
    }
}

/// The tools are in the names `[policy.tools]` accepts, the registry, and a family of their
/// own, and a bare config has neither.
#[test]
fn a_daemon_started_from_a_config_has_the_tools_and_a_bare_config_has_none() {
    let count = |cfg: &crate::Config| {
        let mut r = Registry::default();
        register(cfg, &mut r);
        r.len()
    };
    assert_eq!(count(&crate::Config::example()), 0);
    assert_eq!(count(&started_from_a_file()), 3);
    let mut r = Registry::default();
    register(&started_from_a_file(), &mut r);
    for tool in NAMES {
        let t = r.get(tool).expect(tool);
        assert_eq!(t.family(), FAMILY);
        // The hand-over starts a task, as `task.create` does: a run, and alone in its batch.
        let class = if tool == RUN {
            ToolClass::Run
        } else {
            ToolClass::Read
        };
        assert_eq!(t.class(), class, "{tool}");
        assert!(t
            .description()
            .contains("command line in a shell is not the way"));
    }
}

/// The hold names a profile or lets one go, never both and never neither.
#[test]
fn a_hold_names_a_profile_or_lets_go_and_nothing_else() {
    assert_eq!(
        hold_of(&json!({"profile": " haiku "})),
        Ok(Some("haiku".into()))
    );
    assert_eq!(hold_of(&json!({"release": true})), Ok(None));
    for bad in [
        json!({}),
        json!({"profile": ""}),
        json!({"profile": "haiku", "release": true}),
        json!({"release": false}),
        json!({"profile": "haiku", "session_id": "ses_other"}),
    ] {
        assert!(hold_of(&bad).is_err(), "{bad}");
    }
}

/// What acts after a yes is the plan the card was made from, and a call that is not that plan
/// is not done, whatever the model said after the card.
#[test]
fn what_acts_after_a_yes_is_the_plan_the_card_showed() {
    let card = json!({"profile": "haiku"});
    assert_eq!(
        hold::kept_plan(&card, &json!({"profile": "haiku"})),
        Ok(Some("haiku".into()))
    );
    assert_eq!(
        hold::kept_plan(&json!({"release": true}), &json!({"release": true})),
        Ok(None)
    );
    for (kept, call) in [
        (card.clone(), json!({"profile": "opus"})),
        (card, json!({"release": true})),
        (json!({"release": true}), json!({"profile": "haiku"})),
    ] {
        let why = hold::kept_plan(&kept, &call).unwrap_err();
        assert!(why.contains("not what its card showed"), "{why}");
        assert!(!why.contains("not done"), "said once: {why}");
        assert!(why.ends_with("Nothing was done."), "{why}");
    }
}

/// The owner's yes to a hold is kept for its one call, taken once, and a yes nobody took is
/// dropped when the next is kept.
#[test]
fn a_yes_is_kept_for_its_call_and_taken_once() {
    let board = Board::default();
    let who = crate::test_support::owner_cli();
    board.keep_yes("act_1", "ses_a", "exe_a", &who);
    assert!(board.take_yes("act_2").is_none(), "another call's");
    assert_eq!(board.take_yes("act_1").map(|w| w.label), Some(who.label));
    assert!(board.take_yes("act_1").is_none(), "one hold's: taken once");
    board.keep_yes("act_3", "ses_a", "exe_a", &crate::test_support::owner_cli());
    board.drop_yes("act_3");
    assert!(
        board.take_yes("act_3").is_none(),
        "a failed answer keeps none"
    );
}

/// A session's answers are dropped when its turn ends, and another session's are not.
#[test]
fn a_turns_unposted_answers_are_dropped_with_its_session() {
    let answer = || SubagentAnswer {
        profile: "haiku".into(),
        provider: "anthropic".into(),
        model: "claude-haiku-5-5".into(),
        asked_by: "sonnet".into(),
        text: "words".into(),
        cost_usd: None,
        elapsed_ms: 1,
        output_tokens: 1,
        cut: false,
        call: "act_1".into(),
        outcome: String::new(),
    };
    let board = Board::default();
    board.give("ses_a", "turn_a", answer());
    board.give("ses_b", "turn_b", answer());
    let left = board.sweep("ses_a");
    assert_eq!(left.len(), 1, "handed back, for a faulted turn to post");
    assert!(board.take("turn_a").is_empty());
    assert_eq!(board.take("turn_b").len(), 1);
}
