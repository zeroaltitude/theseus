//! The daemon's route facts (theseus-9f72b): the last reply's model, a task's
//! models, the restricted-model rule, the price period, and which pack reads
//! them (none today: route.v8 asks `mode`, not `model`).

use serde_json::json;
use theseus_judge::builders::{InboundInput, PricePeriod, RestrictedModels, SwitchCost};

use super::route_facts::{asks_model, facts_of};
use crate::node::{Body, Node};

fn said(model: &str, i: u32) -> Node {
    Node::assistant(
        "s",
        "t",
        i,
        Body::AssistantMessage {
            blocks: vec![json!({"type": "text", "text": "ok"})],
            model: model.into(),
            provider: "anthropic".into(),
            stop_reason: None,
            usage: Default::default(),
            cost_usd: None,
            catalog_version: None,
            request_id: None,
            correlation_id: None,
            compilation_id: None,
            request_digest: None,
            effort: None,
        },
    )
}

fn asked(text: &str) -> Node {
    Node::user("s", None, "operator", text)
}

/// 2026-10-27 is a Tuesday: 09:00 UTC is 02:00 in Phoenix.
const TUE_0200_PHOENIX: u64 = 1_793_091_600_000;

#[test]
fn the_last_reply_model_is_the_newest_assistant_nodes() {
    let nodes = [
        asked("one"),
        said("claude-haiku-5-5", 0),
        asked("two"),
        said("claude-sonnet-5-5", 1),
        asked("three"),
    ];
    let f = facts_of(&nodes, false, RestrictedModels::Allowed, TUE_0200_PHOENIX);
    assert_eq!(f.last_reply_model.as_deref(), Some("claude-sonnet-5-5"));
    let none = facts_of(
        &nodes[..1],
        false,
        RestrictedModels::Allowed,
        TUE_0200_PHOENIX,
    );
    assert_eq!(none.last_reply_model, None);
}

#[test]
fn a_tasks_models_are_every_model_that_wrote_once_in_order() {
    let nodes = [
        asked("brief"),
        said("claude-sonnet-5-5", 0),
        said("deepseek-flash", 1),
        said("claude-sonnet-5-5", 2),
    ];
    let task = facts_of(&nodes, true, RestrictedModels::Allowed, TUE_0200_PHOENIX);
    assert_eq!(task.task_models, ["claude-sonnet-5-5", "deepseek-flash"]);
    let chat = facts_of(&nodes, false, RestrictedModels::Allowed, TUE_0200_PHOENIX);
    assert!(chat.task_models.is_empty(), "outside a task: none");
}

#[test]
fn the_restricted_rule_passes_through_and_defaults_closed() {
    for r in [RestrictedModels::Allowed, RestrictedModels::NotAllowed] {
        assert_eq!(facts_of(&[], false, r, 0).restricted_models, r);
    }
    assert_eq!(RestrictedModels::default(), RestrictedModels::NotAllowed);
}

#[test]
fn the_price_period_comes_from_the_instant_given() {
    // Tuesday 02:00 Phoenix is peak; ten hours on, 12:00 Phoenix, is not.
    let at = |ms| facts_of(&[], false, RestrictedModels::Allowed, ms).price_period;
    assert_eq!(at(TUE_0200_PHOENIX), PricePeriod::Peak);
    assert_eq!(at(TUE_0200_PHOENIX + 10 * 3_600_000), PricePeriod::OffPeak);
}

/// The gate is the question: today's packs ask `mode`, and route.v10's will ask `model`.
#[test]
fn only_a_pack_that_asks_model_reads_the_facts() {
    for p in theseus_judge::pack::embedded().as_ref().unwrap() {
        let has = p.questions.iter().any(|q| q.id == "model");
        assert_eq!(asks_model(p), has, "{}", p.name());
        assert!(
            !asks_model(p) || p.name().starts_with("route.v1"),
            "{}",
            p.name()
        );
    }
}

/// Row 2 is inert: the live route pack is route.v8, which asks `mode`, so the
/// state a turn sends it is the one it always was, with none of the facts. The
/// daemon computes them for every ask, and the pack gate keeps them off.
#[tokio::test]
async fn the_live_route_packs_state_carries_no_facts_yet() {
    use crate::tests_route::{mode, rig, turn, until_calls, until_route_rows};
    let jev = theseus_judge::fake::FakeJev::start().unwrap();
    mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    turn(
        &r.core,
        None,
        "Weigh two designs for a crash-safe write-ahead log.",
        None,
    )
    .await;
    until_calls(&jev, 2).await;
    let rows = until_route_rows(&r.core.store, 1).await;
    let digest = rows[0].data["context"]["blob"]
        .as_str()
        .unwrap()
        .to_string();
    let state = std::fs::read_to_string(r.core.store.blobs().path(&digest)).unwrap();
    let state: serde_json::Value = serde_json::from_str(&state).unwrap();
    assert!(state["message"].is_string(), "{state}");
    for k in [
        "price_period",
        "restricted_models",
        "last_reply_model",
        "task_models",
        "current_model",
        "switch_cost",
    ] {
        assert!(state.get(k).is_none(), "{k} in route.v8's state: {state}");
    }
}

fn said_with(input: u64, read: u64, wrote: u64) -> Node {
    let mut n = said("claude-sonnet-5-5", 0);
    if let Body::AssistantMessage { usage, .. } = &mut n.body {
        usage.input_tokens = input;
        usage.cache_read_input_tokens = read;
        usage.cache_creation_input_tokens = wrote;
    }
    n
}

/// theseus-h66v4 row 3, finding B: the table reads "a long context" and "above
/// 100K tokens", so the state carries the conversation's size: the last
/// request's prompt as the provider counted it (new, cached and written), else
/// a quarter of the text's bytes.
#[test]
fn the_estimate_is_the_last_prompt_the_provider_counted() {
    let nodes = [
        asked("one"),
        said_with(300, 90_000, 50_000),
        asked("two"),
        said_with(700, 130_000, 10_000),
        asked("three"),
    ];
    let f = facts_of(&nodes, false, RestrictedModels::Allowed, TUE_0200_PHOENIX);
    assert_eq!(f.est_tokens, Some(700 + 130_000 + 10_000));
}

#[test]
fn without_a_counted_reply_the_estimate_is_a_quarter_of_the_text() {
    let text = "x".repeat(4_000);
    let nodes = [asked(&text), said("claude-sonnet-5-5", 0), asked(&text)];
    let f = facts_of(&nodes, false, RestrictedModels::Allowed, TUE_0200_PHOENIX);
    assert_eq!(f.est_tokens, Some(2_000), "8,002 bytes of text, a quarter");
    let f = facts_of(&[], false, RestrictedModels::Allowed, TUE_0200_PHOENIX);
    assert_eq!(f.est_tokens, Some(0));
}

fn said_on(model: &str, prompt: u64) -> Node {
    let mut n = said(model, 0);
    if let Body::AssistantMessage { usage, .. } = &mut n.body {
        usage.cache_read_input_tokens = prompt;
    }
    n
}

/// theseus-h66v4.1: the warm model is the one that answered last, and what a
/// switch would cost is read off the last prompt's size and the catalog.
#[test]
fn the_warm_model_and_the_switch_cost_come_from_the_last_reply_and_its_prompt() {
    // A thank-you on a 200K Sonnet session: a cheaper model is dearer, stay.
    let nodes = [
        asked("one"),
        said_on("claude-sonnet-5-5", 200_000),
        asked("thanks"),
    ];
    let f = facts_of(&nodes, false, RestrictedModels::Allowed, TUE_0200_PHOENIX);
    assert_eq!(f.current_model.as_deref(), Some("claude-sonnet-5-5"));
    assert_eq!(f.switch_cost, Some(SwitchCost::CheaperToStay));
    // The same model on 40K: a cheaper one is cheaper.
    let nodes = [
        asked("one"),
        said_on("claude-sonnet-5-5", 40_000),
        asked("two"),
    ];
    let f = facts_of(&nodes, false, RestrictedModels::Allowed, TUE_0200_PHOENIX);
    assert_eq!(f.switch_cost, Some(SwitchCost::CheaperToSwitch));
    // A long Haiku session: the cheapest already, stay.
    let nodes = [
        asked("one"),
        said_on("claude-haiku-5-5", 150_000),
        asked("two"),
    ];
    let f = facts_of(&nodes, false, RestrictedModels::Allowed, TUE_0200_PHOENIX);
    assert_eq!(f.current_model.as_deref(), Some("claude-haiku-5-5"));
    assert_eq!(f.switch_cost, Some(SwitchCost::CheaperToStay));
    // The newest reply's model is the warm one, not the oldest.
    let nodes = [
        asked("one"),
        said_on("claude-haiku-5-5", 10_000),
        asked("two"),
        said_on("claude-sonnet-5-5", 200_000),
        asked("three"),
    ];
    let f = facts_of(&nodes, false, RestrictedModels::Allowed, TUE_0200_PHOENIX);
    assert_eq!(f.current_model.as_deref(), Some("claude-sonnet-5-5"));
}

/// A fresh session has no warm model, so nothing is lost by switching: no
/// `current_model`, and the cost reads `about_the_same`.
#[test]
fn a_fresh_session_has_no_warm_model_and_a_switch_costs_about_the_same() {
    let f = facts_of(
        &[asked("what port is postgres on?")],
        false,
        RestrictedModels::Allowed,
        0,
    );
    assert_eq!(f.current_model, None);
    assert_eq!(f.switch_cost, Some(SwitchCost::AboutTheSame));
}

/// The facts reach only a pack that asks `model`: route.v8 is handed the input
/// with no facts at all, whatever the facts say, so its state is the one it
/// always was and its goldens do not move.
#[test]
fn route_v8_is_handed_no_facts_and_its_state_is_unchanged() {
    use theseus_judge::builders::{prepare, Input};
    use theseus_judge::state::NoScrub;
    let facts = facts_of(
        &[
            asked("one"),
            said_on("claude-sonnet-5-5", 200_000),
            asked("two"),
        ],
        false,
        RestrictedModels::Allowed,
        TUE_0200_PHOENIX,
    );
    assert!(facts.current_model.is_some() && facts.switch_cost.is_some());
    let mut with = InboundInput::default();
    with.message = "two".into();
    with.route = Some(facts);
    let mut without = with.clone();
    without.route = None;
    let carried = super::route_facts::Carried::of(with);
    let v8 = theseus_judge::pack::by_name("route.v8").unwrap();
    let v10 = theseus_judge::pack::by_name("route.v10").unwrap();
    let state_of = |p: &theseus_judge::Pack, i: &Input| {
        prepare(p, i, &NoScrub).unwrap().state.value().to_string()
    };
    let expected = state_of(&v8, &Input::Inbound(without));
    assert_eq!(state_of(&v8, carried.for_route(&v8)), expected);
    for field in ["current_model", "switch_cost", "price_period", "est_tokens"] {
        assert!(
            !expected.contains(field),
            "{field} in route.v8's state: {expected}"
        );
    }
    let v10_state = state_of(&v10, carried.for_route(&v10));
    for field in ["current_model", "switch_cost"] {
        assert!(
            v10_state.contains(field),
            "{field} missing from route.v10's: {v10_state}"
        );
    }
}
