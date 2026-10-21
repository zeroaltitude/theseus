//! The data floor (theseus-a1wvq.3): no request reaches a model of restricted
//! origin (`cn` or `unknown`) without a `DataPass` minted for that one request,
//! and nothing mints one yet. The chokepoint (`data_gate.rs`) refuses it with
//! nothing sent and a `data_gate.blocked` row; the route (`turn/data_step.rs`)
//! moves a turn bound for one to the `[model]` default with `data_gate`, or
//! ends it before any model call. The rig is `tests_route`'s: `anthropic`
//! (Claude) and `zai` (GLM, a China-origin endpoint), each a fake that records
//! its requests.

use std::sync::Arc;

use serde_json::{json, Value};
use theseus_judge::fake::{FakeJev, FakeMode};
use theseus_protocol::{SessionKind, TurnSubmitResult};

use crate::bus::EventSink;
use crate::config::{JudgePackConfig, PackMode};
use crate::data::DataPass;
use crate::provider::{Provider, ProviderRequest};
use crate::routing::Routed;
use crate::rpc::Core;
use crate::session::SessionRecord;
use crate::tests_judge::kinds;
use crate::tests_route::{
    call, decided, mode, restricted_placements, rig, rig_parts, turn, until_late, Rig,
};
use crate::turn::TurnRequest;

const PLAIN: &str = "Rename the call sites in the billing module.";

fn request(model: &str, text: &str) -> ProviderRequest {
    ProviderRequest {
        model: model.into(),
        max_tokens: 16,
        system: vec![],
        messages: vec![json!({"role": "user", "content": [{"type": "text", "text": text}]})],
        tools: vec![],
        thinking: None,
        output_config: None,
        cache_control: None,
        betas: vec![],
        extra: Default::default(),
        image_tokens: 0,
        data_pass: None,
    }
}

/// Send `req` through `p`: its error's class and words, if refused.
async fn send(p: &Arc<dyn Provider>, req: &ProviderRequest) -> Result<(), (String, String)> {
    let mut sink = |_: crate::provider::Delta<'_>| {};
    match p.stream_message(req, &mut sink).await {
        Ok(_) => Ok(()),
        Err(e) => Err((
            e.downcast_ref::<crate::provider::ProviderError>()
                .map_or("", |p| p.class())
                .to_string(),
            format!("{e:#}"),
        )),
    }
}

fn blocked(r: &Rig) -> Vec<Value> {
    kinds(&r.core.store, "data_gate.blocked")
        .into_iter()
        .map(|r| r.data)
        .collect()
}

/// The key test: a core with no sensitive names and no `[google]` (the
/// chokepoint wrapped nothing there before) refuses a plain request to GLM
/// with nothing sent and one `data_gate.blocked` row; Claude is sent it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_provider_refuses_a_restricted_model_without_a_pass() {
    let r = rig(None, 1, |_| {});
    let cfg = &r.core.runner.cfg;
    assert!(cfg.privacy.sensitive_names.is_empty() && cfg.google.is_empty());
    let providers = &r.core.runner.providers;
    let (class, said) = send(&providers["zai"], &request("glm-5.3", PLAIN))
        .await
        .unwrap_err();
    assert_eq!(class, "data_gate", "{said}");
    assert!(said.contains("glm-5.3 is of restricted origin"), "{said}");
    assert!(r.zai.requests().is_empty(), "nothing was sent");
    let rows = blocked(&r);
    assert_eq!(rows.len(), 1, "{rows:?}");
    assert_eq!(
        (&rows[0]["provider"], &rows[0]["model"], &rows[0]["origin"]),
        (&json!("zai"), &json!("glm-5.3"), &json!("cn"))
    );
    send(
        &providers["anthropic"],
        &request("claude-sonnet-5-5", PLAIN),
    )
    .await
    .unwrap();
    assert_eq!(r.claude.requests().len(), 1);
    assert_eq!(blocked(&r).len(), 1, "an unrestricted model writes no row");
}

/// A pass minted for request A sends A; on request B, A with one byte
/// changed (a later loop's request), it is refused, and the fake records A
/// alone. A copy of a request carries no pass.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_pass_covers_only_the_request_it_was_minted_for() {
    let r = rig(None, 2, |_| {});
    let zai = &r.core.runner.providers["zai"];
    let mut a = request("glm-5.3", PLAIN);
    a.data_pass = Some(DataPass::for_test(&a.digest()));
    assert!(a.clone().data_pass.is_none(), "a copy is another request");
    send(zai, &a).await.unwrap();
    let mut b = request("glm-5.3", &PLAIN.replace("module", "modulf"));
    b.data_pass = Some(DataPass::for_test(&a.digest()));
    assert_eq!(send(zai, &b).await.unwrap_err().0, "data_gate");
    let sent = r.zai.requests();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].messages, a.messages);
}

/// A pass is never sent: the digest and the body are the same with it.
#[test]
fn data_pass_never_changes_the_request_digest() {
    let plain = request("glm-5.3", PLAIN);
    let mut with = request("glm-5.3", PLAIN);
    with.data_pass = Some(DataPass::for_test(&plain.digest()));
    assert_eq!(with.digest(), plain.digest());
    assert_eq!(
        serde_json::to_string(&with).unwrap(),
        serde_json::to_string(&plain).unwrap()
    );
    assert_eq!(with, plain);
}

/// How a turn comes to be bound for GLM.
#[derive(Debug, Clone, Copy)]
enum Case {
    NoVerdict,
    Late,
    Unsure,
    Shadow,
    Pin,
    Correction,
    Hold,
    RoutedBase,
    TrivialDetour,
    RoutingOff,
}

/// One case's turn, on a fresh rig: its result.
async fn bound_for_glm(case: Case) -> (Rig, TurnSubmitResult) {
    let jev = FakeJev::start().unwrap();
    let verdict = match case {
        Case::Unsure => ("sophisticated", 0.2),
        Case::Hold => ("routine_coding", 0.95),
        Case::TrivialDetour => ("trivial", 0.95),
        _ => ("chat", 0.95),
    };
    mode(&jev, verdict.0, verdict.1);
    let r = match case {
        Case::RoutingOff => rig(None, 2, |_| {}),
        Case::Late => rig(Some(&jev), 2, |c| {
            c.routing.max_wait_ms = 200;
            c.judge.total_secs = 30;
        }),
        Case::Shadow => rig(Some(&jev), 2, |c| {
            let shadow = JudgePackConfig {
                mode: Some(PackMode::Shadow),
                sample: None,
                notices: None,
            };
            let pack = crate::judge::inbound::ROUTE_PACK;
            c.judge.packs.insert(pack.into(), shadow);
        }),
        _ => rig(Some(&jev), 2, restricted_placements),
    };
    match case {
        Case::NoVerdict => jev.set_mode(FakeMode::Down),
        Case::Late => jev.set_mode(FakeMode::Held),
        _ => {}
    }
    let on_glm = !matches!(case, Case::Pin | Case::Correction | Case::RoutedBase);
    if on_glm {
        call(&r.core, "profile.use", json!({"name": "glm"})).await;
    }
    let res = match case {
        Case::Pin => turn(&r.core, None, PLAIN, Some("glm")).await,
        Case::Correction => {
            let one = turn(&r.core, None, PLAIN, None).await;
            assert_eq!(one.profile, "sonnet", "the live profile");
            let words = "hey, that should have been on glm";
            crate::tests_route_correct::ask(&r.core, Some(&one.session_id), words).await
        }
        Case::Hold | Case::RoutedBase => {
            let mut rec = SessionRecord::new(SessionKind::Conversation, None);
            rec.routed = Some(Box::new(match case {
                Case::Hold => Routed {
                    profile: None,
                    from: None,
                    hold: Some(crate::routing::Hold {
                        mode: "routine_coding".into(),
                        profile: "glm".into(),
                        turn: "trn_earlier".into(),
                    }),
                },
                _ => Routed {
                    profile: Some("glm".into()),
                    from: Some("sonnet".into()),
                    hold: None,
                },
            }));
            r.core.store.put_session(&rec.session_id, &rec).unwrap();
            turn(&r.core, Some(&rec.session_id), PLAIN, None).await
        }
        _ => turn(&r.core, None, PLAIN, None).await,
    };
    jev.release();
    if matches!(case, Case::Late) {
        until_late(&r.core, &res.session_id).await;
    }
    (r, res)
}

/// Every way a turn not cleared comes to be bound for a restricted profile
/// runs on the `[model]` default instead, with a `route.decided` row whose
/// reason is `data_gate`, and nothing reaches the restricted model.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_restricted_base_moves_to_the_fallback_with_data_gate() {
    for case in [
        Case::NoVerdict,
        Case::Late,
        Case::Unsure,
        Case::Shadow,
        Case::Pin,
        Case::Correction,
        Case::Hold,
        Case::RoutedBase,
        Case::TrivialDetour,
        Case::RoutingOff,
    ] {
        let (r, res) = bound_for_glm(case).await;
        assert_eq!(
            res.profile, "default",
            "{case:?}: {} {:?}",
            res.output, res.route
        );
        assert_eq!(res.model, "claude-sonnet-5-5", "{case:?}");
        let rows = decided(&r.core.store);
        let last = rows.last().unwrap_or_else(|| panic!("{case:?}: no row"));
        assert_eq!(last["reason"], "data_gate", "{case:?}: {rows:?}");
        assert_eq!(
            res.route.as_ref().map(|r| r.reason.as_str()),
            Some("data_gate"),
            "{case:?}"
        );
        assert!(
            r.zai.requests().is_empty(),
            "{case:?}: GLM was sent nothing"
        );
        assert!(
            blocked(&r).is_empty(),
            "{case:?}: it never reached the chokepoint"
        );
        assert!(!r.claude.requests().is_empty(), "{case:?}");
    }
}

/// A turn on the live profile, run by the runner, whose failure the test
/// reads.
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

/// With the default itself restricted there is nowhere to run: the turn
/// fails before any model call, routed or not, and both fakes record
/// nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn no_usable_fallback_fails_the_turn_and_sends_nothing() {
    for routed in [true, false] {
        let jev = FakeJev::start().unwrap();
        mode(&jev, "chat", 0.95);
        // After the load's check, which refuses the owner-material keys'
        // profiles on the default's provider once it is restricted.
        let cn = |p: &mut crate::rpc::Parts| p.cfg.serve_default_from_cn();
        let r = rig_parts(routed.then_some(&jev), 1, |_| {}, cn);
        let ended = try_turn(&r.core, PLAIN).await;
        let said = format!("{ended:?}");
        assert!(
            said.contains("No model cleared for this request's data is usable"),
            "routed {routed}: {said}"
        );
        assert!(said.contains("data_gate"), "{said}");
        assert!(r.claude.requests().is_empty() && r.zai.requests().is_empty());
    }
}

/// The code's `trivial` list still names `glm` (the routing-composition row
/// changes it): with its first profile unusable, a trivial verdict skips
/// GLM to `cheapest`, which is Haiku's profile, never GLM.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_trivial_default_skips_glm() {
    let jev = FakeJev::start().unwrap();
    mode(&jev, "trivial", 0.95);
    let r = rig_parts(
        Some(&jev),
        1,
        |c| c.routing.classes.trivial = crate::config::RoutingConfig::default().classes.trivial,
        // After the load's check, which refuses an unknown model for the
        // owner-material keys that name `haiku`.
        |p| p.cfg.unprice_profile("haiku"),
    );
    let list = &r.core.runner.cfg.routing.classes.trivial.profiles;
    assert_eq!(list, &["haiku", "glm", "cheapest"], "the code's default");
    let res = turn(&r.core, None, "Hi there!", None).await;
    assert_eq!(
        (res.profile.as_str(), res.model.as_str()),
        ("haikuhi", "claude-haiku-5-5"),
        "{:?}",
        res.route
    );
    assert!(r.zai.requests().is_empty());
}

/// The names' floor holds beside the new one: a named message never runs on
/// a Claude served from a host vouched EU, and a pass for a GLM request
/// never lifts a name.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_named_request_still_needs_a_us_model() {
    let named = "Rename the call sites in Examplecorp's billing module.";
    let jev = FakeJev::start().unwrap();
    mode(&jev, "chat", 0.95);
    let r = rig(Some(&jev), 1, |c| {
        c.privacy.sensitive_names = vec!["Examplecorp".into()];
        c.serve_default_from_eu();
    });
    let origin = r
        .core
        .runner
        .cfg
        .profile_origin("default", &r.core.runner.catalog);
    assert_eq!(origin, crate::catalog::ModelOrigin::Eu, "unrestricted");
    let said = format!("{:?}", try_turn(&r.core, named).await);
    assert!(said.contains("sensitive_names"), "{said}");
    assert!(r.claude.requests().is_empty() && r.zai.requests().is_empty());
    let mut req = request("glm-5.3", named);
    req.data_pass = Some(DataPass::for_test(&req.digest()));
    let zai = &r.core.runner.providers["zai"];
    let (class, said) = send(zai, &req).await.unwrap_err();
    assert_eq!(class, "data_gate");
    assert!(said.contains("sensitive_names"), "{said}");
    assert!(r.zai.requests().is_empty());
}

/// A refusal is never retried into a send: the driver's rule parks a
/// `data_gate` failure on input at once, settled or not, so its one retry of a
/// lasting class (`internal` alone) never asks the model again.
#[test]
fn a_data_gate_failure_is_parked_never_retried() {
    use crate::session::{Failing, Then};
    for settled in [true, false] {
        let (_, then, _) = Failing::after(None, "data_gate", false, settled, 0, true);
        assert_eq!(then, Then::Park, "settled {settled}");
    }
}
