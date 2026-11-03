//! A turn's effort and the cache, through route.v10 (theseus-h66v4.11). route.v8's tests set a
//! turn's effort through the class walk and its `reply_effort` question, which route.v10 retires
//! (`effort.v1` asks `effort` in a pack of its own); the code they tested did not go. On a model
//! that takes per-message effort the level is an effort-only system message before the turn's own
//! message and the top-level effort stays the profile's (`compiler::effort`), and a model whose
//! row takes no effort is sent none. Here each is driven by Jev's `model` and `effort` answers on
//! `tests_route_open`'s rig (route.v10's shape acting), so the level is the only thing that moves.
//!
//! - a turn at Jev's level repeats the request before it, whole, across turns (the cache);
//! - the history's messages come from the answers' records, whatever the top-level effort;
//! - a model whose catalog row takes no effort ignores Jev's answer, and the row says so.

use serde_json::{json, Value};
use theseus_judge::fake::FakeJev;

use crate::compiler::effort::effective;
use crate::compiler::BETA_TURN_EFFORT;
use crate::config::Effort;
use crate::node::Body;
use crate::provider::ProviderRequest;
use crate::tests_route::{decided, turn, Rig};
use crate::tests_route_open::{open, says};

/// The effort-only system message of `level`.
fn level_message(level: &str) -> Value {
    json!({"role": "system", "content": [], "output_config": {"effort": level}})
}

/// `later` begins with every byte `earlier` sent before its cache breakpoint: the same model,
/// tools, system and top-level effort, and `earlier`'s messages.
fn keeps(earlier: &ProviderRequest, later: &ProviderRequest, what: &str) {
    assert_eq!(later.model, earlier.model, "{what}: the model");
    assert_eq!(later.tools, earlier.tools, "{what}: the tools");
    assert_eq!(later.system, earlier.system, "{what}: the system");
    assert_eq!(
        later.output_config, earlier.output_config,
        "{what}: the top-level effort"
    );
    assert_eq!(later.thinking, earlier.thinking, "{what}: the thinking");
    let n = earlier.messages.len();
    assert!(later.messages.len() > n, "{what}: nothing appended");
    assert_eq!(
        serde_json::to_string(&later.messages[..n]).unwrap(),
        serde_json::to_string(&earlier.messages).unwrap(),
        "{what}: the messages before the breakpoint changed"
    );
}

/// The effort each answer of the session recorded.
fn recorded(r: &Rig, session: &str) -> Vec<Option<Effort>> {
    r.core
        .store
        .transcript(session)
        .unwrap()
        .iter()
        .filter_map(|(_, n)| match &n.body {
            Body::AssistantMessage { effort, .. } => Some(*effort),
            _ => None,
        })
        .collect()
}

/// The owner's case on Fable 5.1, the one model that takes per-message effort (its profile leaves
/// effort to the model, `high`): a turn at that level, one Jev puts at `low`, and one at `high`
/// again. Each request repeats the one before it whole across the turns (the first says `high`, the model's own, before its message); the `low` turn's message
/// stays where it went, and the next turn's message puts the level back. Jev picks Fable on every
/// turn, so only the level moves.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_turn_at_jevs_effort_repeats_the_cached_prefix() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "fable", "high", 0.95);
    let o = open(&jev, 3, |c| c.model.live = "fable".into());
    let r = &o.r;
    let one = turn(&r.core, None, "Where do the herons nest?", None).await;
    assert_eq!(one.profile, "fable");
    says(&jev, "fable", "low", 0.95);
    let two = turn(&r.core, Some(&one.session_id), "And the egrets?", None).await;
    assert_eq!(two.profile, "fable", "the level is never a switch");
    assert_eq!(decided(&r.core.store)[1]["effort_ran"], "low");
    says(&jev, "fable", "high", 0.95);
    turn(&r.core, Some(&one.session_id), "Thanks.", None).await;
    let reqs = r.claude.requests();
    assert_eq!(reqs.len(), 3);
    let (r1, r2, r3) = (&reqs[0], &reqs[1], &reqs[2]);
    assert_eq!(
        r1.output_config, None,
        "the profile's own stays at the top: the model's default"
    );
    // Jev's `high` is a level of its own (route.v10 never leaves the model's default: an answer
    // under the bar is `high` too), so the first request says it before its message, once.
    let said: Vec<&Value> = r1
        .messages
        .iter()
        .filter(|m| m["role"] == "system")
        .collect();
    assert_eq!(said, [&level_message("high")], "{:?}", r1.messages);
    keeps(r1, r2, "the low turn");
    let n = r1.messages.len();
    let roles: Vec<&Value> = r2.messages[n..].iter().map(|m| &m["role"]).collect();
    assert_eq!(roles, ["assistant", "system", "user"], "{:?}", r2.messages);
    assert_eq!(r2.messages[n + 1], level_message("low"));
    assert_eq!(effective(r2), Some(Effort::Low));
    assert!(r2.betas.iter().any(|b| b == BETA_TURN_EFFORT));
    keeps(r2, r3, "the turn after it");
    assert_eq!(r3.messages[r2.messages.len() + 1], level_message("high"));
    assert_eq!(effective(r3), Some(Effort::High), "Fable 5.1's default");
    assert_eq!(
        recorded(r, &one.session_id),
        [Some(Effort::High), Some(Effort::Low), Some(Effort::High)]
    );
}

/// The history's messages come from its answers' records, whatever the request's top-level
/// effort: on Fable 5.1 the `low` turn's message is still just before its user message. Opus 5.5
/// and Haiku 5.5, which are not known to take per-message effort, get none (here pinned with `-P`,
/// so no route is asked and the profile's own effort stands), and the beta rides no request of
/// theirs.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_historys_messages_follow_the_records() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "fable", "low", 0.95);
    let o = open(&jev, 4, |c| c.model.live = "fable".into());
    let r = &o.r;
    let one = turn(&r.core, None, "Where do the herons nest?", None).await;
    says(&jev, "fable", "high", 0.95);
    let sid = one.session_id.clone();
    assert_eq!(
        turn(&r.core, Some(&sid), "And now?", None).await.profile,
        "fable"
    );
    for (name, model) in [("opus", "claude-opus-5-5"), ("haiku", "claude-haiku-5-5")] {
        let res = turn(&r.core, Some(&sid), "Thanks.", Some(name)).await;
        assert_eq!(res.profile, name);
        let req = r.claude.requests().pop().unwrap();
        assert_eq!(req.model, model);
        assert!(
            req.messages.iter().all(|m| m["role"] != "system"),
            "{name}: {:?}",
            req.messages
        );
        assert!(!req.betas.iter().any(|b| b == BETA_TURN_EFFORT), "{name}");
    }
    let reqs = r.claude.requests();
    let haiku = reqs.last().unwrap();
    assert_eq!(effective(haiku), Some(Effort::Low), "Haiku's profile's own");
    let fable = &reqs[1];
    assert_eq!(fable.model, "claude-fable-5-1");
    assert_eq!(
        (&fable.messages[0], &fable.messages[1]),
        (&level_message("low"), &reqs[0].messages[1])
    );
    assert_eq!(
        fable.messages[3],
        level_message("high"),
        "Fable 5.1's default puts it back"
    );
}

/// A model whose catalog row takes no effort (Sonnet 5.5 here, with its row's `effort` turned
/// off) is sent none: Jev's `high` is recorded on the row as `no_effort`, not applied, and the
/// request carries no `output_config`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_model_without_effort_ignores_it() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "sonnet", "high", 0.95);
    let o = open(&jev, 1, |c| {
        c.catalog.insert(
            "claude-sonnet-5-5".into(),
            crate::catalog::CatalogRow {
                effort: Some(false),
                ..Default::default()
            },
        );
    });
    let r = &o.r;
    let res = turn(&r.core, None, "Rename these twelve call sites.", None).await;
    assert_eq!(res.profile, "sonnet");
    let req = &r.claude.requests()[0];
    assert_eq!(req.model, "claude-sonnet-5-5");
    assert_eq!(effective(req), None, "{:?}", req.output_config);
    assert_eq!(req.output_config, None);
    let d = &decided(&r.core.store)[0];
    assert_eq!(
        (
            &d["effort"],
            &d["effort_reason"],
            &d["effort_applied"],
            &d["effort_ran"]
        ),
        (
            &json!("high"),
            &json!("no_effort"),
            &json!(false),
            &Value::Null
        ),
        "{d}"
    );
}
