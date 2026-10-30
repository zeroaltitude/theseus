//! The daemon's route facts (theseus-9f72b): the last reply's model, a task's
//! models, the restricted-model rule, the price period, and which pack reads
//! them (none today: route.v8 asks `mode`, not `model`).

use serde_json::json;
use theseus_judge::builders::{PricePeriod, RestrictedModels};

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
    ] {
        assert!(state.get(k).is_none(), "{k} in route.v8's state: {state}");
    }
}
