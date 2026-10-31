//! `theseus profile use` is an operator's pin when route.v10 acts (theseus-h66v4
//! row 3, finding A): someone who runs it has named a model as plainly as one
//! who passes `-P`, so the model is honored, `reason = pinned`, and Jev is not
//! asked the model. Under route.v8 it is what it always was, the baseline Jev
//! routes from.

use serde_json::Value;
use theseus_judge::fake::FakeJev;
use theseus_protocol::{
    method, Id, Message, ProfileUseParams, Request, TurnSubmitParams, TurnSubmitResult,
};

use super::tests::roundtrip;
use crate::tests_route::{decided, rig};
use crate::tests_route_open::{open, says};

const DESIGN: &str = "design the memory layer for a million turns";

fn use_profile(id: u64, name: &str) -> Request {
    Request::new(
        Id::Num(id),
        method::PROFILE_USE,
        ProfileUseParams { name: name.into() },
    )
}

pub(super) fn ask(id: u64, input: &str, profile: Option<&str>) -> Request {
    Request::new(
        Id::Num(id),
        method::TURN_SUBMIT,
        TurnSubmitParams {
            surface: None,
            dir: None,
            carried: false,
            prompt: None,
            session_id: None,
            input: input.into(),
            profile: profile.map(str::to_string),
            provider: None,
            model: None,
            author: None,
            attachments: vec![],
            reply_to: None,
            opened_from: None,
            thread: None,
            speaking: None,
            in_reply_to: None,
            route_at_ms: None,
            deadline_secs: None,
            via: None,
        },
    )
}

/// `profile use`, answered, and only then the turn: the daemon does not order
/// the two on one connection (`profile.use` names no session), so a test that
/// means "after" waits for the answer.
async fn after_use(
    core: &std::sync::Arc<crate::rpc::Core>,
    name: &str,
    turn: Request,
) -> TurnSubmitResult {
    let used = roundtrip(core.clone(), vec![use_profile(1, name)]).await;
    assert!(used
        .iter()
        .any(|m| matches!(m, Message::Response(r) if r.error.is_none())));
    result(&roundtrip(core.clone(), vec![turn]).await, 2)
}

pub(super) fn result(msgs: &[Message], id: u64) -> TurnSubmitResult {
    msgs.iter()
        .find_map(|m| match m {
            Message::Response(r) if r.id == Id::Num(id) => Some(r),
            _ => None,
        })
        .and_then(|r| r.result.clone())
        .map(|v| serde_json::from_value(v).unwrap())
        .unwrap_or_else(|| panic!("no result for {id}: {msgs:?}"))
}

/// W40 as the design words it: `theseus profile use haiku`, then a Fable-shaped
/// question, with Jev saying Fable at 0.97: it stays on Haiku, pinned, and the
/// model question was never put to Jev.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn profile_use_is_a_pin_when_route_v10_acts() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "fable", "max", 0.97);
    let o = open(&jev, 1, |_| {});
    let res = after_use(&o.r.core, "haiku", ask(2, DESIGN, None)).await;
    assert_eq!(
        (res.profile.as_str(), res.model.as_str()),
        ("haiku", "claude-haiku-5-5")
    );
    assert_eq!(res.route.as_ref().unwrap().reason, "pinned");
    assert_eq!(o.r.claude.requests()[0].model, "claude-haiku-5-5");
    crate::tests_route::until_calls(&jev, 2).await;
    let asked: Vec<String> = jev
        .seen()
        .iter()
        .flat_map(|s| {
            s.body["questions"]
                .as_object()
                .map(|q| q.keys().cloned().collect::<Vec<_>>())
                .unwrap_or_default()
        })
        .collect();
    assert!(
        !asked
            .iter()
            .any(|q| q.ends_with("/model") || q.ends_with("/effort")),
        "{asked:?}"
    );
    let d = &decided(&o.r.core.store)[0];
    assert_eq!(d["reason"], "pinned");
    assert_eq!(d["detail"], "profile use haiku");
    assert!(d["pick"].is_null(), "Jev picked nothing: {d}");
}

/// Before any `profile use`, the live profile is the config's: the no-verdict
/// default and nothing more, and Jev routes from it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_configured_live_profile_is_not_a_pin() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "fable", "high", 0.97);
    let o = open(&jev, 1, |_| {});
    assert_eq!(o.r.core.live_profile().1, "config");
    let msgs = roundtrip(o.r.core.clone(), vec![ask(1, DESIGN, None)]).await;
    let res = result(&msgs, 1);
    assert_eq!(res.profile, "fable");
    assert_eq!(res.route.unwrap().reason, "verdict");
}

/// An explicit profile for the turn beats the pin, as it beats the live one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_named_profile_beats_the_live_pin() {
    let jev = FakeJev::start().unwrap();
    says(&jev, "haiku", "low", 0.97);
    let o = open(&jev, 1, |_| {});
    let res = after_use(&o.r.core, "haiku", ask(2, DESIGN, Some("opus"))).await;
    assert_eq!(res.profile, "opus");
    let d = &decided(&o.r.core.store)[0];
    assert_eq!(
        (d["reason"].clone(), d["detail"].clone()),
        (Value::from("pinned"), Value::from("-P opus"))
    );
}

/// Under route.v8 nothing changed: `profile use haiku` is the baseline Jev
/// routes from, and a `sophisticated` verdict still moves the turn to Opus.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn under_route_v8_profile_use_is_still_only_the_baseline() {
    let jev = FakeJev::start().unwrap();
    crate::tests_route::mode(&jev, "sophisticated", 0.95);
    let r = rig(Some(&jev), 1, |_| {});
    let res = after_use(&r.core, "haiku", ask(2, DESIGN, None)).await;
    assert_eq!(res.profile, "opus");
    assert_eq!(res.route.unwrap().reason, "verdict");
}
