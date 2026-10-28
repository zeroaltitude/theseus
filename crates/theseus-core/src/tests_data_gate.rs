//! The floor under `[privacy] sensitive_names` on the turn's route
//! (theseus-oejh3): a message that names one never runs on a profile of
//! restricted origin. The provider chokepoint has its own tests in
//! `data_gate.rs`. The rig is `tests_route`'s: `anthropic` (sonnet) and `zai`
//! (GLM, a China-origin endpoint), each a fake that records its requests.

use std::sync::Arc;

use theseus_judge::fake::FakeJev;
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::tests_route::{decided, mode, rig, turn, Rig};
use crate::turn::TurnRequest;
use crate::Config;

const NAMED: &str = "Rename the call sites in Examplecorp's billing module.";
const PLAIN: &str = "Rename the call sites in the billing module.";

/// A new conversation's turn on the live profile, as `tests_route::turn`
/// runs one, whose failure the test reads.
async fn try_turn(core: &Arc<Core>, input: &str) -> anyhow::Result<TurnSubmitResult> {
    let rec = SessionRecord::new(SessionKind::Conversation, None);
    core.store.put_session(&rec.session_id, &rec).unwrap();
    let (live, _) = core.live_profile();
    let target = core.runner.resolve_target(&live, None, None, None).unwrap();
    let sink = EventSink::new(core.bus.clone(), &rec.session_id, None);
    core.runner
        .run(TurnRequest {
            prompt: None,
            surface: None,
            session: rec,
            input: Some(input.into()),
            target,
            sink,
            author: "test".into(),
            recompile: None,
            attachments: vec![],
            arrived: None,
            reply_to: None,
            answer_at: None,
        })
        .await
}

/// A GLM profile first in routine coding's list, a Claude one after it, and a
/// name to keep off restricted models.
fn listed(c: &mut Config) {
    named(c);
    list(c);
}

/// The name to keep off restricted models.
fn named(c: &mut Config) {
    c.privacy.sensitive_names = vec!["Examplecorp".into()];
}

/// GLM first in routine coding's list, Claude after it.
fn list(c: &mut Config) {
    c.routing.classes.routine_coding.profiles = vec!["glm53".into(), "sonnet".into()];
}

/// [`listed`] with `glm53` GLM itself, of restricted origin. The load refuses
/// a restricted profile in a mode's list (theseus-a1wvq.4), so the list goes
/// in after its check: the floor still holds on a list that names one.
fn rig_listed(jev: Option<&FakeJev>, n: usize, tweak: impl FnOnce(&mut Config)) -> Rig {
    let tweak = |c: &mut Config| {
        named(c);
        tweak(c);
    };
    crate::tests_route::rig_parts(jev, n, tweak, |p| list(&mut p.cfg))
}

/// [`listed`], with `glm53` a Claude served from a host vouched EU: of no
/// restricted origin, so only the names' floor keeps a turn off it
/// (theseus-a1wvq.3 keeps every request off GLM itself).
fn listed_eu(c: &mut Config) {
    listed(c);
    c.serve_glm_unrestricted();
    c.vouch_zai(crate::catalog::ModelOrigin::Eu);
}

/// A plain message runs on the first profile of the list, an EU one; a
/// message that names a configured name runs on the second, of US origin.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sensitive_name_skips_restricted_profiles() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    let r = rig(Some(&jev), 1, listed_eu);
    let res = turn(&r.core, None, PLAIN, None).await;
    assert_eq!(res.profile, "glm53");
    assert_eq!(r.zai.requests().len(), 1);
    assert!(r.claude.requests().is_empty());

    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    let r = rig(Some(&jev), 1, listed_eu);
    let res = turn(&r.core, None, NAMED, None).await;
    assert_eq!(res.profile, "sonnet");
    assert!(
        r.zai.requests().is_empty(),
        "nothing went to the restricted model"
    );
    assert_eq!(r.claude.requests().len(), 1);
    assert_eq!(decided(&r.core.store).len(), 1);
}

/// A name said earlier in the conversation is in the request a restricted
/// model would be sent, so the next turn keeps off it too, though its own
/// words name nothing (the join's fix: the route read the message alone, and
/// the chokepoint then failed the turn).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_name_said_earlier_keeps_the_next_turn_off_restricted_profiles() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    let r = rig_listed(Some(&jev), 2, |_| {});
    let first = turn(&r.core, None, NAMED, None).await;
    assert_eq!(first.profile, "sonnet");
    let next = turn(&r.core, Some(&first.session_id), PLAIN, None).await;
    assert_eq!(next.profile, "sonnet", "{}", next.output);
    assert!(
        r.zai.requests().is_empty(),
        "nothing went to the restricted model"
    );
    assert_eq!(r.claude.requests().len(), 2);
}

/// The owner's pin of a restricted profile does not carry a named message
/// there either: the `[model]` default runs, and the row says `data_gate`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pin_of_a_restricted_profile_gives_way_to_the_default() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig_listed(Some(&jev), 1, |_| {});
    let res = turn(&r.core, None, NAMED, Some("glm53")).await;
    assert_eq!(res.profile, "default");
    assert!(r.zai.requests().is_empty());
    let d = decided(&r.core.store);
    assert_eq!(d[0]["reason"], "data_gate", "{d:?}");
}

/// With nothing of unrestricted origin to run on, a message that names a
/// configured name ends the turn before any model call (and since
/// theseus-a1wvq.3 so does one that names nothing:
/// `tests_data_floor::no_usable_fallback_fails_the_turn_and_sends_nothing`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sensitive_name_with_only_restricted_profiles_sends_nothing() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    // The default restricted after the load's check, which would refuse the
    // owner-material keys' profiles on its provider.
    let all_cn = |p: &mut crate::rpc::Parts| {
        list(&mut p.cfg);
        p.cfg.serve_default_from_cn();
    };
    let r = crate::tests_route::rig_parts(Some(&jev), 1, named, all_cn);
    let origin = r
        .core
        .runner
        .cfg
        .profile_origin("default", &r.core.runner.catalog);
    assert!(origin.restricted(), "the default is restricted here");
    let ended = try_turn(&r.core, NAMED).await;
    let said = format!("{ended:?}");
    assert!(
        said.contains("data_gate") || said.contains("sensitive_names"),
        "{said}"
    );
    assert!(r.claude.requests().is_empty() && r.zai.requests().is_empty());
}

/// The names go only to a model of US origin (theseus-xs5ah). With Claude
/// served from a host vouched as EU, a named message has nowhere to run: the
/// turn ends before any model call, though EU is not of restricted origin. A
/// message that names nothing runs there as before.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_sensitive_name_never_runs_on_a_model_of_eu_origin() {
    let eu = |c: &mut Config| c.serve_default_from_eu();
    let jev = FakeJev::start().unwrap();
    mode(&jev, "routine_coding", 0.95);
    let r = rig_listed(Some(&jev), 1, eu);
    let cfg = &r.core.runner.cfg;
    let origin = cfg.profile_origin("sonnet", &r.core.runner.catalog);
    assert_eq!(origin, crate::catalog::ModelOrigin::Eu);
    let ended = try_turn(&r.core, NAMED).await;
    let said = format!("{ended:?}");
    assert!(said.contains("data_gate"), "{said}");
    assert!(r.claude.requests().is_empty() && r.zai.requests().is_empty());

    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig_listed(Some(&jev), 1, eu);
    let res = turn(&r.core, None, "Say hello.", None).await;
    assert_eq!(r.claude.requests().len(), 1, "{}", res.output);
}

/// A configured name that an embedded pack's text holds is said at start by
/// the pack and the number of hits, never by the name (theseus-oejh3).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pack_naming_a_sensitive_name_is_reported_at_start() {
    let (_, text) = theseus_judge::pack::EMBEDDED
        .iter()
        .find(|(name, _)| *name == "route.v7")
        .expect("route.v7 is embedded");
    let word = text
        .split(|c: char| !c.is_alphanumeric())
        .find(|w| w.chars().count() > 9)
        .expect("a long word in the pack");
    let word = word.to_string();
    let r = rig(None, 0, |c| c.privacy.sensitive_names = vec![word.clone()]);
    r.core.check_pack_text_after_serving();
    let t0 = std::time::Instant::now();
    let phase = loop {
        let found = r
            .core
            .startup_log
            .snapshot()
            .into_iter()
            .find(|p| p.name == crate::pack_names::PHASE);
        if let Some(p) = found {
            break p;
        }
        assert!(t0.elapsed().as_secs() < 10, "no phase for the hit");
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    };
    let said = phase.detail.to_string();
    assert!(said.contains("route.v7"), "{said}");
    assert!(
        !said.to_lowercase().contains(&word.to_lowercase()),
        "the name is never said: {said}"
    );
}

/// The core wraps every provider it is built with (`data_gate::guard`): a
/// request that names a configured name is refused for the restricted
/// provider's model with nothing recorded, and is sent to the unrestricted one.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_provider_of_the_core_is_behind_the_floor() {
    let r = rig_listed(None, 2, |_| {});
    let req = |model: &str| crate::provider::ProviderRequest {
        model: model.into(),
        max_tokens: 16,
        system: vec![],
        messages: vec![serde_json::json!({"role": "user", "content": [
            {"type": "text", "text": "About Examplecorp's plan."}]})],
        tools: vec![],
        thinking: None,
        output_config: None,
        cache_control: None,
        betas: vec![],
        extra: Default::default(),
        image_tokens: 0,
    };
    let providers = &r.core.runner.providers;
    let mut sink = |_: crate::provider::Delta<'_>| {};
    let cn = req("glm-5.3");
    let refused = providers["zai"]
        .stream_message(&cn, &mut sink)
        .await
        .unwrap_err();
    let class = refused
        .downcast_ref::<crate::provider::ProviderError>()
        .map(|e| e.class());
    assert_eq!(class, Some("data_gate"));
    assert!(r.zai.requests().is_empty(), "nothing was sent");
    let us = req("claude-sonnet-5-5");
    providers["anthropic"]
        .stream_message(&us, &mut sink)
        .await
        .unwrap();
    assert_eq!(r.claude.requests().len(), 1);
}
