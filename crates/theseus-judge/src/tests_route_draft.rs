//! `route.v9`'s `draft` question (theseus-kqeih): when the operator lets Jev
//! choose which coding turns a cheaper drafting model makes the first pass of,
//! the route request carries one more question, whose words are the owner's
//! standing orders; otherwise it carries nothing, and the request is the one
//! that was measured without it.

use crate::builders::{prepare, InboundInput, Input};
use crate::client::Question;
use crate::pack::{by_name, Source};

/// `Source::DraftChoice`, by its pack-file name, so a pack without the source fails the
/// test where it asserts, not where it compiles.
fn draft_choice() -> Source {
    serde_json::from_value(serde_json::json!("draft_choice")).expect("a source of that name")
}
use crate::state::NoScrub;

fn input() -> InboundInput {
    serde_json::from_str(include_str!("../fixtures/inputs/inbound.json")).unwrap()
}

/// The input with the operator's switch on, as the core sets it: through the
/// input's own fields, so the test reads the wire shape.
fn letting_jev_choose() -> InboundInput {
    let mut v = serde_json::to_value(input()).unwrap();
    v["draft_choice"] = serde_json::json!(true);
    serde_json::from_value(v).expect("the input carries the operator's switch")
}

fn means(q: &Question) -> String {
    let Question::Choice { options, .. } = q else {
        panic!("a choice")
    };
    options
        .iter()
        .map(|o| format!("{}: {}", o.id, o.means.clone().unwrap_or_default()))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Off, the pack asks what it asked before, and the state is the same bytes;
/// on, `draft` rides the same request, beside `mode` and `reply_effort`.
#[test]
fn the_draft_question_is_asked_only_when_the_operator_lets_jev_choose() {
    let p = by_name("route.v9").unwrap();
    let off = prepare(&p, &Input::Inbound(input()), &NoScrub).unwrap();
    assert!(p.ask(&off.dynamic).iter().all(|a| a.id != "draft"));
    assert!(off.dynamic.items(draft_choice()).is_empty());
    let on = prepare(&p, &Input::Inbound(letting_jev_choose()), &NoScrub).unwrap();
    assert_eq!(on.state.json, off.state.json, "the state does not move");
    assert_eq!(on.dynamic.items(draft_choice()).len(), 1);
    let asked = p.ask(&on.dynamic);
    let ids: Vec<&str> = asked.iter().map(|a| a.id.as_str()).collect();
    assert!(ids.contains(&"draft"), "{ids:?}");
    assert!(
        ids.contains(&"mode") && ids.contains(&"reply_effort"),
        "{ids:?}"
    );
    // Every earlier pack is untouched.
    for old in ["route.v5", "route.v6", "route.v7", "route.v8"] {
        let old = by_name(old).unwrap();
        assert!(old.question("draft").is_none(), "{}", old.name());
    }
}

/// The question's frame: it decides nothing for the pack's promotion, it asks
/// for sureness at the pack's bars, and it fails to the stronger model.
#[test]
fn the_draft_question_decides_nothing_and_fails_to_the_stronger_model() {
    let p = by_name("route.v9").unwrap();
    let q = p.question("draft").expect("route.v9 asks `draft`");
    assert!(!q.decides);
    assert_eq!(q.only_when, Some(draft_choice()));
    assert_eq!(q.no_match.as_deref(), Some("strong"));
    let ids: Vec<&str> = q.options.iter().map(|o| o.id.as_str()).collect();
    assert_eq!(
        ids,
        ["cheap", "strong"],
        "read in order: the stronger one last"
    );
    assert_eq!((q.thresholds.act, q.thresholds.confirm), (0.90, 0.60));
}

/// The owner's standing orders (2026-10-27 23:05 and 23:15), in the question's
/// own words: where the cheaper model's first pass belongs, where it does not,
/// that it is not the default for code, and what a failed draft costs.
#[test]
fn the_standing_orders_are_the_questions_words() {
    let p = by_name("route.v9").unwrap();
    let q = p.question("draft").expect("route.v9 asks `draft`");
    let asked = p.ask(&{
        let mut d = crate::pack::Dynamic::default();
        d.sources.insert(
            draft_choice(),
            vec![crate::pack::Item {
                key: "draft".into(),
                text: "x".into(),
            }],
        );
        d
    });
    let a = asked.iter().find(|a| a.id == "draft").expect("asked");
    let Question::Choice { instructions, .. } = &a.question else {
        panic!("a choice")
    };
    assert_eq!(instructions, &q.instructions);
    let text = format!("{instructions}\n{}", means(&a.question)).to_lowercase();
    for (needle, why) in [
        (
            "not the default",
            "code is not first-pass-on-flash by default",
        ),
        ("repository", "repo-level fixes are a strong point"),
        ("mechanical", "mechanical and automation edits are one"),
        ("tests", "writing tests is one"),
        ("long", "so is long-context mechanical work"),
        ("terminal", "terminal-driven multi-step work is not"),
        ("from scratch", "writing a program from scratch is not"),
        ("review", "anything marked sophisticated or review is not"),
        ("whole context", "the cold-handoff premium is named"),
        ("cheap to", "prefer where a failure is cheap to recover"),
        ("when unsure", "unsure goes to the stronger model"),
    ] {
        assert!(text.contains(needle), "{why}: `{needle}` is not in\n{text}");
    }
    // No vendor, model or company is named: the profile is the operator's.
    for name in [
        "deepseek",
        "flash",
        "claude",
        "sonnet",
        "opus",
        "haiku",
        "anthropic",
    ] {
        assert!(!text.contains(name), "the question names `{name}`:\n{text}");
    }
}
